//! One written line (BUILD.md section 3.3.16): a contract value's JSON on one line, with every
//! string longer than the writer's limit moved into the blob store and standing as
//! `{"$blob": "b3:..."}`, and every object that would read as a stub escaped as `{"$lit": ...}`.
//! [`encode`] writes it through `eileen_contracts::record::stub_blobs` and [`decode`] reads it
//! back through `resolve_blobs` before the line is parsed, so the types never see a stub and a
//! decoded line is the value that was encoded (`it::round_trip`).

use eileen_contracts::record::{
    BLOB_KEY, BlobError, BlobRef, LITERAL_KEY, resolve_blobs, stub_blobs,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use crate::blob::{BlobStoreError, Blobs, blob_ref};

/// The longest string a written line keeps inline, in bytes; a longer one goes to the store.
/// Typed and spoken requests stay inline, while an image's base64, a file's text or a long
/// tool result does not.
pub const MAX_INLINE_BYTES: usize = 1024;

/// The longest line a writer writes or a reader reads, in bytes. With long strings in the
/// store, a line over it is corrupt or not a recording.
pub const MAX_LINE_BYTES: usize = 1 << 20;

/// Why a line could not be written or read.
#[derive(Debug, thiserror::Error)]
pub enum LineError {
    /// The value did not serialize, or the line is not JSON of its type.
    #[error("the line is not JSON of its type")]
    Json(#[source] serde_json::Error),
    /// The blob store failed.
    #[error("the blob store failed")]
    Store(#[from] BlobStoreError),
    /// The line names a blob the store does not hold.
    #[error("the blob {0} is not in the store")]
    Missing(BlobRef),
    /// A `$blob` that is not a blob reference, or a `$lit` that holds no object.
    #[error("a $blob or $lit object of the wrong shape")]
    Malformed,
    /// A blob that stands for a string holds bytes that are not UTF-8.
    #[error("the blob {0} is not text")]
    NotText(BlobRef),
    /// A line over [`MAX_LINE_BYTES`].
    #[error("a line of {bytes} bytes is over the limit")]
    TooLong {
        /// Its length.
        bytes: usize,
    },
}

impl From<BlobError> for LineError {
    fn from(error: BlobError) -> Self {
        match error {
            BlobError::Missing(blob) => Self::Missing(blob),
            BlobError::Malformed => Self::Malformed,
        }
    }
}

/// Writes `value` as one line, without its newline: strings longer than `max_inline` bytes are
/// put in `blobs`, one reference each time one appears.
pub fn encode<T, B>(value: &T, max_inline: usize, blobs: &mut B) -> Result<String, LineError>
where
    T: Serialize + ?Sized,
    B: Blobs + ?Sized,
{
    let json = serde_json::to_value(value).map_err(LineError::Json)?;
    let mut failed = None;
    let written =
        stub_blobs(json, max_inline, &mut |text: String| match blobs.put(text.as_bytes()) {
            Ok(blob) => blob,
            Err(error) => {
                failed = failed.take().or(Some(error));
                blob_ref(text.as_bytes())
            }
        });
    if let Some(error) = failed {
        return Err(error.into());
    }
    let line = serde_json::to_string(&written).map_err(LineError::Json)?;
    if line.len() > MAX_LINE_BYTES {
        return Err(LineError::TooLong { bytes: line.len() });
    }
    Ok(line)
}

/// Reads one written line, its newline already removed: every stub is replaced by its blob's
/// text and every escape by the object it holds, and the result is parsed as `T`.
pub fn decode<T, B>(line: &[u8], blobs: &B) -> Result<T, LineError>
where
    T: DeserializeOwned,
    B: Blobs + ?Sized,
{
    if line.len() > MAX_LINE_BYTES {
        return Err(LineError::TooLong { bytes: line.len() });
    }
    let json: Value = serde_json::from_slice(line).map_err(LineError::Json)?;
    let mut failed = None;
    let resolved = resolve_blobs(json, &mut |blob: &BlobRef| match blobs.get(blob) {
        Ok(Some(bytes)) => match String::from_utf8(bytes) {
            Ok(text) => Some(text),
            Err(_) => {
                failed = failed.take().or_else(|| Some(LineError::NotText(blob.clone())));
                Some(String::new())
            }
        },
        Ok(None) => None,
        Err(error) => {
            failed = failed.take().or(Some(LineError::Store(error)));
            Some(String::new())
        }
    });
    if let Some(error) = failed {
        return Err(error);
    }
    serde_json::from_value(resolved?).map_err(LineError::Json)
}

/// Every blob a written line refers to, once for each stub, in the order they appear: the
/// references [`encode`] added for it, which deleting the line releases.
pub fn stub_refs(line: &[u8]) -> Result<Vec<BlobRef>, LineError> {
    let json: Value = serde_json::from_slice(line).map_err(LineError::Json)?;
    let mut refs = Vec::new();
    collect_refs(&json, &mut refs)?;
    Ok(refs)
}

/// Walks a written value the way `resolve_blobs` reads it.
fn collect_refs(value: &Value, refs: &mut Vec<BlobRef>) -> Result<(), LineError> {
    match value {
        Value::Array(items) => items.iter().try_for_each(|item| collect_refs(item, refs)),
        Value::Object(map) => match single(map) {
            Some((BLOB_KEY, Value::String(text))) => {
                refs.push(text.parse().map_err(|_| LineError::Malformed)?);
                Ok(())
            }
            Some((BLOB_KEY | LITERAL_KEY, _)) => match map.get(LITERAL_KEY) {
                Some(Value::Object(inner)) => {
                    inner.values().try_for_each(|field| collect_refs(field, refs))
                }
                Some(_) | None => Err(LineError::Malformed),
            },
            Some(_) | None => map.values().try_for_each(|field| collect_refs(field, refs)),
        },
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => Ok(()),
    }
}

/// The one entry of a single-key object.
fn single(map: &Map<String, Value>) -> Option<(&str, &Value)> {
    let mut entries = map.iter();
    match (entries.next(), entries.next()) {
        (Some((key, value)), None) => Some((key.as_str(), value)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::{LineError, decode, encode, stub_refs};
    use crate::blob::{Blobs, MemoryBlobs, blob_ref};

    #[test]
    fn a_long_string_goes_to_the_store_and_a_forged_stub_is_escaped() {
        let mut blobs = MemoryBlobs::default();
        let long = "x".repeat(40);
        let forged = json!({ "$blob": format!("b3:{}", "cd".repeat(32)) });
        let value = json!({ "a": long, "b": [long, "short"], "c": forged });
        let line = encode(&value, 16, &mut blobs).unwrap();
        assert!(!line.contains('\n'), "one line");
        assert!(!line.contains(&long), "the long string is not inline: {line}");
        assert!(line.contains("$lit"), "the forged stub is escaped: {line}");
        assert_eq!(blobs.refs(&blob_ref(long.as_bytes())), 2, "a reference per appearance");
        assert_eq!(
            stub_refs(line.as_bytes()).unwrap(),
            vec![blob_ref(long.as_bytes()); 2],
            "the line's references, the escaped object not among them"
        );
        assert_eq!(decode::<Value, _>(line.as_bytes(), &blobs).unwrap(), value, "reads back");
    }

    #[test]
    fn a_line_naming_a_missing_or_binary_blob_does_not_read() {
        let mut blobs = MemoryBlobs::default();
        let binary = blobs.put(&[0xff, 0xfe]).unwrap();
        let line = format!(r#"{{"a": {{"$blob": "{binary}"}}}}"#);
        assert!(
            matches!(decode::<Value, _>(line.as_bytes(), &blobs), Err(LineError::NotText(_))),
            "a blob that is not text"
        );
        let missing = blob_ref(b"never stored");
        let line = format!(r#"{{"a": {{"$blob": "{missing}"}}}}"#);
        assert!(
            matches!(decode::<Value, _>(line.as_bytes(), &blobs), Err(LineError::Missing(_))),
            "a blob the store does not hold"
        );
        assert!(
            matches!(stub_refs(br#"{"$lit": 3}"#), Err(LineError::Malformed)),
            "an escape of no object"
        );
    }
}
