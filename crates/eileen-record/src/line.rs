//! One written line (BUILD.md section 3.3.16): a contract value's JSON on one line, with every
//! string longer than the writer's limit moved into the blob store and standing as
//! `{"$blob": "b3:..."}`, and every object that would read as a stub escaped as `{"$lit": ...}`.
//! [`encode`] writes it through `eileen_contracts::record::stub_blobs` and [`decode`] reads it
//! back through `resolve_blobs` before the line is parsed, so the types never see a stub and a
//! decoded line is the value that was encoded (`it::round_trip`).
//!
//! Each stub holds one reference in the store. [`encode`] adds them and keeps none when it
//! fails, and [`stub_refs`] lists them through `resolve_blobs` itself, so the references a line
//! is released by are the ones its reader resolves.

use eileen_contracts::record::{BlobError, BlobRef, resolve_blobs, stub_blobs};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::blob::{BlobStoreError, Blobs, blob_ref};

/// The longest string a written line keeps inline, in bytes; a longer one goes to the store.
/// Typed and spoken requests stay inline, while an image's base64, a file's text or a long
/// tool result does not.
pub const MAX_INLINE_BYTES: usize = 1024;

/// The longest line a writer writes or a reader reads, in bytes. With long strings in the
/// store, a line over it is corrupt or not a recording.
pub const MAX_LINE_BYTES: usize = 1 << 20;

/// The deepest nesting of arrays and objects a written line may hold. `serde_json`'s parser
/// refuses a 128th level, and escapes can double the nesting of a value, so [`encode`] refuses
/// a line [`decode`] could not parse rather than write one that never reads.
pub const MAX_DEPTH: usize = 127;

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
    /// A written value nested deeper than [`MAX_DEPTH`].
    #[error("a line nested {depth} levels deep is over the limit")]
    TooDeep {
        /// The first level over the limit.
        depth: usize,
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
/// put in `blobs`, one reference each time one appears. On an error the references it added are
/// released again, so a line that was not written holds none.
pub fn encode<T, B>(value: &T, max_inline: usize, blobs: &mut B) -> Result<String, LineError>
where
    T: Serialize + ?Sized,
    B: Blobs + ?Sized,
{
    let json = serde_json::to_value(value).map_err(LineError::Json)?;
    let mut added = Vec::new();
    let mut failed = None;
    let written = stub_blobs(json, max_inline, &mut |text: String| {
        if failed.is_some() {
            return blob_ref(text.as_bytes());
        }
        match blobs.put(text.as_bytes()) {
            Ok(blob) => {
                added.push(blob.clone());
                blob
            }
            Err(error) => {
                failed = Some(error);
                blob_ref(text.as_bytes())
            }
        }
    });
    let line = match failed {
        Some(error) => Err(LineError::Store(error)),
        None => match depth_over(&written, MAX_DEPTH) {
            Some(depth) => Err(LineError::TooDeep { depth }),
            None => serde_json::to_string(&written).map_err(LineError::Json).and_then(|line| {
                if line.len() > MAX_LINE_BYTES {
                    Err(LineError::TooLong { bytes: line.len() })
                } else {
                    Ok(line)
                }
            }),
        },
    };
    if line.is_err() {
        release_all(blobs, &added);
    }
    line
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
    let resolved = resolve_blobs(json, &mut |blob: &BlobRef| {
        if failed.is_some() {
            return Some(String::new());
        }
        match blobs.get(blob) {
            Ok(Some(bytes)) => {
                let text = String::from_utf8(bytes).ok();
                if text.is_none() {
                    failed = Some(LineError::NotText(blob.clone()));
                }
                Some(text.unwrap_or_default())
            }
            Ok(None) => None,
            Err(error) => {
                failed = Some(LineError::Store(error));
                Some(String::new())
            }
        }
    });
    if let Some(error) = failed {
        return Err(error);
    }
    serde_json::from_value(resolved?).map_err(LineError::Json)
}

/// Every blob a written line refers to, once for each stub, in the order `resolve_blobs` meets
/// them: the references [`encode`] added for it, which deleting the line releases.
pub fn stub_refs(line: &[u8]) -> Result<Vec<BlobRef>, LineError> {
    let json: Value = serde_json::from_slice(line).map_err(LineError::Json)?;
    let mut refs = Vec::new();
    resolve_blobs(json, &mut |blob: &BlobRef| {
        refs.push(blob.clone());
        Some(String::new())
    })?;
    Ok(refs)
}

/// A stub as `serde_json::to_string` writes it, up to its hash.
const STUB_OPEN: &[u8] = br#"{"$blob":"b3:"#;

/// What follows a stub's hash.
const STUB_CLOSE: &[u8] = br#""}"#;

/// The key before an escaped object.
const ESCAPE_KEY: &[u8] = br#""$lit":"#;

/// The length of a hash in hex.
const HASH_HEX: usize = 64;

/// The blobs of a line that does not parse, such as one a writer stopped part way through: each
/// complete stub in its bytes, written `{"$blob":"b3:<64 hex>"}`, that does not follow a
/// `"$lit":` key. Inside a string the quotes would be escaped, so every such stub is one
/// [`encode`] wrote, and a forged one in the value is always escaped right after `"$lit":`.
/// A real stub that is the value of a `$lit` key is skipped too, and a stub cut in two is not
/// found, so the list can miss references but never holds one the line did not add.
#[must_use]
pub fn scan_refs(line: &[u8]) -> Vec<BlobRef> {
    let mut refs = Vec::new();
    for start in 0..line.len() {
        let Some(after) = line.get(start..).and_then(|rest| rest.strip_prefix(STUB_OPEN)) else {
            continue;
        };
        let (Some(hex), Some(tail)) = (after.get(..HASH_HEX), after.get(HASH_HEX..)) else {
            continue;
        };
        let escaped = line.get(..start).is_some_and(|before| before.ends_with(ESCAPE_KEY));
        if escaped || !tail.starts_with(STUB_CLOSE) {
            continue;
        }
        let blob = std::str::from_utf8(hex).ok().and_then(|hex| format!("b3:{hex}").parse().ok());
        if let Some(blob) = blob {
            refs.push(blob);
        }
    }
    refs
}

/// Releases references added for a line that is not written. A release that fails leaves its
/// blob counted too high, which keeps it longer and never deletes it early.
pub(crate) fn release_all<B: Blobs + ?Sized>(blobs: &mut B, refs: &[BlobRef]) {
    for blob in refs {
        let _kept_if_failed = blobs.release(blob);
    }
}

/// The first nesting level of `value` over `max`, counting each array and object as a level,
/// or `None` when it is within it.
fn depth_over(value: &Value, max: usize) -> Option<usize> {
    let mut pending = vec![(value, 0_usize)];
    while let Some((value, above)) = pending.pop() {
        let children: Box<dyn Iterator<Item = &Value>> = match value {
            Value::Array(items) => Box::new(items.iter()),
            Value::Object(map) => Box::new(map.values()),
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => continue,
        };
        let depth = above.saturating_add(1);
        if depth > max {
            return Some(depth);
        }
        pending.extend(children.map(|child| (child, depth)));
    }
    None
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::{LineError, MAX_DEPTH, MAX_LINE_BYTES, decode, encode, scan_refs, stub_refs};
    use crate::blob::{BlobStoreError, Blobs, MemoryBlobs, Released, blob_ref};
    use eileen_contracts::record::BlobRef;

    /// A memory store whose puts fail from the `fail_from`th on, counting from 1.
    struct Failing {
        inner: MemoryBlobs,
        puts: usize,
        fail_from: usize,
    }

    impl Blobs for Failing {
        fn put(&mut self, bytes: &[u8]) -> Result<BlobRef, BlobStoreError> {
            self.puts += 1;
            if self.puts >= self.fail_from {
                return Err(BlobStoreError::TooLarge { bytes: bytes.len() });
            }
            self.inner.put(bytes)
        }

        fn get(&self, blob: &BlobRef) -> Result<Option<Vec<u8>>, BlobStoreError> {
            self.inner.get(blob)
        }

        fn release(&mut self, blob: &BlobRef) -> Result<Released, BlobStoreError> {
            self.inner.release(blob)
        }
    }

    /// `leaf` inside `levels` arrays.
    fn nested(levels: usize, leaf: Value) -> Value {
        (0..levels).fold(leaf, |inner, _| Value::Array(vec![inner]))
    }

    #[test]
    fn a_long_string_goes_to_the_store_and_a_forged_stub_is_escaped() {
        let mut blobs = MemoryBlobs::default();
        let long = "x".repeat(120);
        let forged = json!({ "$blob": format!("b3:{}", "cd".repeat(32)) });
        let value = json!({ "a": long, "b": [long, "short"], "c": forged });
        let line = encode(&value, 100, &mut blobs).unwrap();
        assert!(!line.contains('\n'), "one line");
        assert!(!line.contains(&long), "the long string is not inline: {line}");
        assert!(line.contains("$lit"), "the forged stub is escaped: {line}");
        assert_eq!(blobs.refs(&blob_ref(long.as_bytes())), 2, "a reference per appearance");
        assert_eq!(
            stub_refs(line.as_bytes()).unwrap(),
            vec![blob_ref(long.as_bytes()); 2],
            "the line's references, the escaped object not among them"
        );
        assert_eq!(
            scan_refs(line.as_bytes()),
            vec![blob_ref(long.as_bytes()); 2],
            "the scan finds the same, and skips the escaped one"
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

    #[test]
    fn a_value_nested_past_what_the_parser_reads_is_refused_and_keeps_no_reference() {
        let mut blobs = MemoryBlobs::default();
        // The long string's stub is an object, one level below the arrays.
        let deepest = nested(MAX_DEPTH - 1, json!("a long string the store keeps"));
        let line = encode(&deepest, 8, &mut blobs).unwrap();
        assert_eq!(decode::<Value, _>(line.as_bytes(), &blobs).unwrap(), deepest, "the deepest");
        assert_eq!(blobs.len(), 1, "its string stored");

        let mut blobs = MemoryBlobs::default();
        let over = nested(MAX_DEPTH, json!("a long string the store keeps"));
        assert!(
            matches!(
                encode(&over, 8, &mut blobs),
                Err(LineError::TooDeep { depth }) if depth == MAX_DEPTH + 1
            ),
            "one level more"
        );
        assert!(blobs.is_empty(), "the refused line's string released");

        // 65 escapes deep reads as JSON, and writing it doubles the nesting.
        let chain = format!("{}0{}", r#"{"$lit":"#.repeat(65), "}".repeat(65));
        let value: Value = serde_json::from_str(&chain).unwrap();
        assert!(
            matches!(encode(&value, 8, &mut blobs), Err(LineError::TooDeep { .. })),
            "an escape chain whose written form is too deep"
        );
    }

    #[test]
    fn a_line_that_fails_to_encode_leaves_the_store_as_it_was() {
        let value = json!({ "a": "x".repeat(40), "b": "y".repeat(40), "c": "z".repeat(40) });
        let mut blobs = Failing { inner: MemoryBlobs::default(), puts: 0, fail_from: 2 };
        assert!(
            matches!(encode(&value, 8, &mut blobs), Err(LineError::Store(_))),
            "the second put failed"
        );
        assert_eq!(blobs.puts, 2, "no put after the first failure");
        assert!(blobs.inner.is_empty(), "the first string released");

        let mut blobs = MemoryBlobs::default();
        let many: Vec<String> = (0..MAX_LINE_BYTES / 8).map(|n| format!("{n:08}")).collect();
        let value = json!({ "long": "q".repeat(40), "many": many });
        assert!(
            matches!(encode(&value, 8, &mut blobs), Err(LineError::TooLong { .. })),
            "short strings that add up to more than a line"
        );
        assert!(blobs.is_empty(), "the long string released");
    }

    #[test]
    fn a_cut_line_gives_up_only_its_complete_unescaped_stubs() {
        let mut blobs = MemoryBlobs::default();
        // The forged reference, 67 bytes, stays inline under a limit of 70.
        let (first, second) = ("f".repeat(80), "s".repeat(80));
        let forged = json!({ "$blob": format!("b3:{}", "cd".repeat(32)) });
        let value = json!({ "a": first, "b": forged, "c": second });
        let line = encode(&value, 70, &mut blobs).unwrap();
        let cut = line.get(..line.len() - 10).unwrap();
        assert!(stub_refs(cut.as_bytes()).is_err(), "the cut line does not parse");
        assert_eq!(scan_refs(cut.as_bytes()), [blob_ref(first.as_bytes())], "the whole stub");
    }
}
