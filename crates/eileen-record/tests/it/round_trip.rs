//! The crate doc's first invariant: `record::stub_blobs` and `record::resolve_blobs` are exact
//! inverses, so a line written through the disk store reads back to the line that was written,
//! whatever its strings hold and whatever objects in it look like stubs.

use std::io::Cursor;

use eileen_contracts::core_io::{Caller, Content, Input, ToolCallRequest, ToolResult};
use eileen_contracts::ids::{ToolName, Untrusted};
use eileen_contracts::jobs::CallOrigin;
use eileen_contracts::record::RecordLine;
use eileen_record::blob::{BlobStore, Blobs, MemoryBlobs, blob_ref};
use eileen_record::line::{self, MAX_INLINE_BYTES};
use eileen_record::reader::{ReadError, Reader};
use eileen_record::writer::Writer;
use proptest::prelude::*;
use serde_json::{Value, json};

use crate::support::{Scratch, answer, input, pre_tool_use, typed};

/// JSON with long strings, and objects keyed like stubs and escapes.
fn json_value() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        any::<i64>().prop_map(Value::from),
        any::<f64>().prop_filter("finite", |x| x.is_finite()).prop_map(Value::from),
        "[a-z:0-9]{0,12}".prop_map(Value::String),
        r"\PC{10,40}".prop_map(Value::String),
        Just(Value::String(format!("b3:{}", "ab".repeat(32)))),
    ];
    leaf.prop_recursive(4, 32, 4, |inner| {
        let key = prop::sample::select(vec!["$blob", "$lit", "a", "b"]);
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..4).prop_map(Value::Array),
            prop::collection::vec((key, inner), 0..3).prop_map(|pairs| {
                Value::Object(pairs.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
            }),
        ]
    })
}

/// Text of every length around the inline limit, newlines, quotes and other scripts included.
fn text() -> impl Strategy<Value = String> {
    prop_oneof![
        "[a-z ]{0,40}",
        r"\PC{0,80}",
        prop::collection::vec(r#"[a-z"\\\n\t]"#, MAX_INLINE_BYTES - 2..MAX_INLINE_BYTES + 3)
            .prop_map(|chars| chars.concat()),
    ]
}

/// A recording: typed text, and hook inputs with arguments of any shape, each answered.
fn recording() -> impl Strategy<Value = Vec<RecordLine>> {
    prop::collection::vec((text(), json_value()), 0..6).prop_map(|items| {
        let mut lines = Vec::new();
        for (n, (said, args)) in (0_u64..).zip(items) {
            lines.push(typed(3 * n + 1, &said));
            lines.push(pre_tool_use(3 * n + 2, n, args));
            lines.push(answer(3 * n + 2, n, 3 * n + 3));
        }
        lines
    })
}

fn write(lines: &[RecordLine], store: &mut BlobStore, max_inline: usize) -> Vec<u8> {
    let mut writer = Writer::new(Vec::new(), store).with_max_inline(max_inline);
    for line in lines {
        writer.write(line).unwrap();
    }
    assert_eq!(writer.lines(), u64::try_from(lines.len()).unwrap(), "one line each");
    writer.finish().unwrap().0
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    #[test]
    fn stubs_resolve_to_exactly_the_value_written_through_the_disk_store(value in json_value()) {
        let scratch = Scratch::new("value");
        let mut store = BlobStore::open(scratch.path()).unwrap();
        let written = line::encode(&value, 16, &mut store).unwrap();
        prop_assert!(!written.contains('\n'), "one line: {}", written);
        let read: Value = line::decode(written.as_bytes(), &store).unwrap();
        prop_assert_eq!(read, value);
    }

    #[test]
    fn a_written_recording_reads_back_line_for_line(lines in recording(), small in any::<bool>()) {
        let scratch = Scratch::new("recording");
        let mut store = BlobStore::open(scratch.path()).unwrap();
        let max_inline = if small { 8 } else { MAX_INLINE_BYTES };
        let text = write(&lines, &mut store, max_inline);
        prop_assert_eq!(text.split(|byte| *byte == b'\n').count(), lines.len() + 1);
        let read = Reader::new(Cursor::new(&text)).read_all(&store).unwrap();
        prop_assert_eq!(read, lines);
    }
}

#[test]
fn an_image_is_written_as_a_stub_and_its_blob_reads_back() {
    let scratch = Scratch::new("image");
    let mut store = BlobStore::open(scratch.path()).unwrap();
    let base64 = "iVBOR".repeat(2000);
    let result = ToolResult {
        content: vec![Content::Image { mime: "image/png".to_owned(), base64: base64.clone() }],
        is_error: false,
        verification: None,
        output: None,
        duration_ms: 40,
        rung: None,
    };
    let call = "call_01K8ZQ3V2X4Y5Z6A7B8C9D0E1F".parse().unwrap();
    let forged = json!({ "$blob": format!("b3:{}", "cd".repeat(32)) });
    let request = ToolCallRequest {
        caller: Caller::Internal,
        origin: CallOrigin::Core,
        tool: ToolName::new("apps_open_path").unwrap(),
        args: Untrusted::new(json!({ "path": forged })),
        paths: Vec::new(),
        urls: Vec::new(),
        traceparent: None,
    };
    let lines = vec![input(1, Input::ToolResult(call, result)), input(2, Input::ToolCall(request))];
    let text = write(&lines, &mut store, MAX_INLINE_BYTES);
    let written = String::from_utf8(text.clone()).unwrap();
    let image = blob_ref(base64.as_bytes());
    assert!(!written.contains(&base64), "the image is not inline");
    assert!(written.contains(&format!(r#"{{"$blob":"{image}"}}"#)), "a stub: {written}");
    assert!(written.contains(r#"{"$lit":{"$blob":"#), "the forged stub is escaped: {written}");
    assert_eq!(store.get(&image).unwrap().unwrap(), base64.as_bytes(), "the blob");
    assert_eq!(Reader::new(Cursor::new(&text)).read_all(&store).unwrap(), lines, "reads back");
}

#[test]
fn a_recording_with_crlf_endings_and_blank_lines_reads_and_a_cut_last_line_is_reported() {
    let scratch = Scratch::new("endings");
    let mut store = BlobStore::open(scratch.path()).unwrap();
    let lines = vec![typed(1, "open notepad"), typed(2, "and the calculator")];
    let text = String::from_utf8(write(&lines, &mut store, MAX_INLINE_BYTES)).unwrap();
    let crlf = format!("\r\n{}", text.replace('\n', "\r\n\r\n"));
    let read = Reader::new(Cursor::new(crlf.as_bytes())).read_all(&store).unwrap();
    assert_eq!(read, lines, "CRLF endings and blank lines");

    let cut = text.get(..text.len() - 10).unwrap();
    let mut reader = Reader::new(Cursor::new(cut.as_bytes()));
    assert_eq!(reader.next_line(&store).unwrap().unwrap(), lines[0], "the whole first line");
    assert!(
        matches!(reader.next_line(&store), Some(Err(ReadError::Truncated { line: 2 }))),
        "the cut second line"
    );
    assert!(reader.next_line(&store).is_none(), "the end");
}

#[test]
fn a_line_over_the_limit_stops_the_reader() {
    let store = MemoryBlobs::default();
    let long = vec![b'x'; line::MAX_LINE_BYTES + 1];
    let mut reader = Reader::new(Cursor::new(long));
    assert!(
        matches!(reader.next_line(&store), Some(Err(ReadError::TooLong { line: 1 }))),
        "over the limit"
    );
    assert!(reader.next_line(&store).is_none(), "stopped");
}
