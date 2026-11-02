//! Blob reference counts through recordings: every stub a writer wrote holds one reference, and
//! deleting a recording releases them, so a blob no other recording names is deleted with it.
//! Releasing never releases a reference twice and never stops part way: it reads the whole
//! recording first, and a line a writer stopped part way through gives up its complete stubs.

use std::io::{BufReader, Cursor, Read, Write};

use eileen_contracts::record::RecordLine;
use eileen_record::blob::{BlobStore, Blobs, MemoryBlobs, blob_ref};
use eileen_record::line;
use eileen_record::reader::{Reader, ReleaseReport, release_blobs};
use eileen_record::writer::{WriteError, Writer};
use serde_json::json;

use crate::support::{Scratch, pre_tool_use, typed};

fn write(lines: &[RecordLine], store: &mut BlobStore) -> Vec<u8> {
    let mut writer = Writer::new(Vec::new(), store).with_max_inline(64);
    for line in lines {
        writer.write(line).unwrap();
    }
    writer.finish().unwrap().0
}

/// Every file under the store's two-hex folders.
fn files(store: &BlobStore) -> Vec<String> {
    let mut files = Vec::new();
    for folder in std::fs::read_dir(store.root()).unwrap() {
        let folder = folder.unwrap().path();
        if !folder.is_dir() {
            continue;
        }
        for file in std::fs::read_dir(folder).unwrap() {
            files.push(file.unwrap().file_name().to_string_lossy().into_owned());
        }
    }
    files.sort();
    files
}

/// A report of `released` references and `deleted` blobs, nothing else.
const fn report(released: u64, deleted: u64) -> ReleaseReport {
    ReleaseReport { released, deleted, absent: 0, damaged: 0, failed: 0 }
}

#[test]
fn deleting_a_recording_deletes_the_blobs_no_other_recording_names() {
    let scratch = Scratch::new("refcount");
    let mut store = BlobStore::open(scratch.path().join("blobs")).unwrap();
    let shared = "a long answer both recordings quote, ".repeat(4);
    let own = "a long answer only the first recording holds, ".repeat(4);
    let first = write(&[typed(1, &shared), typed(2, &own), typed(3, &shared)], &mut store);
    let second = write(&[typed(1, &shared)], &mut store);
    let (shared_blob, own_blob) = (blob_ref(shared.as_bytes()), blob_ref(own.as_bytes()));
    assert_eq!(store.refs(&shared_blob).unwrap(), 3, "twice in the first, once in the second");
    assert_eq!(store.refs(&own_blob).unwrap(), 1, "once in the first");

    let released = release_blobs(Cursor::new(&first), &mut store).unwrap();
    assert_eq!(released, report(3, 1), "the first deleted");
    assert_eq!(store.get(&own_blob).unwrap(), None, "the unreferenced blob is gone");
    assert_eq!(store.refs(&shared_blob).unwrap(), 1, "the second's reference remains");
    let read = Reader::new(Cursor::new(&second)).read_all(&store).unwrap();
    assert_eq!(read, [typed(1, &shared)], "the second still reads");

    let released = release_blobs(Cursor::new(&second), &mut store).unwrap();
    assert_eq!(released, report(1, 1), "the second deleted");
    assert_eq!(files(&store), Vec::<String>::new(), "no blob and no count left");

    let again = release_blobs(Cursor::new(&second), &mut store).unwrap();
    assert_eq!(again, ReleaseReport { absent: 1, ..report(1, 0) }, "nothing to release");
}

/// Gives `head`, then fails, as a disk that stops part way through a file would.
struct FailsAfter<'a> {
    head: &'a [u8],
}

impl Read for FailsAfter<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.head.is_empty() {
            return Err(std::io::Error::other("the disk stopped"));
        }
        self.head.read(buf)
    }
}

#[test]
fn a_recording_that_does_not_read_to_its_end_releases_nothing() {
    let scratch = Scratch::new("refcount-io");
    let mut store = BlobStore::open(scratch.path().join("blobs")).unwrap();
    let (one, two) = ("the first long answer, ".repeat(4), "the second long answer, ".repeat(4));
    let text = write(&[typed(1, &one), typed(2, &two), typed(3, &one)], &mut store);
    let first_line = text.iter().position(|byte| *byte == b'\n').unwrap() + 1;
    let input = BufReader::new(FailsAfter { head: text.get(..first_line + 20).unwrap() });
    assert!(release_blobs(input, &mut store).is_err(), "the read failed");
    assert_eq!(store.refs(&blob_ref(one.as_bytes())).unwrap(), 2, "no count changed");
    assert_eq!(store.refs(&blob_ref(two.as_bytes())).unwrap(), 1, "no count changed");

    let released = release_blobs(Cursor::new(&text), &mut store).unwrap();
    assert_eq!(released, report(3, 2), "a second call, once the file reads, releases each once");
    assert_eq!(files(&store), Vec::<String>::new(), "no blob left");
}

#[test]
fn a_damaged_middle_line_gives_up_its_whole_stubs_and_another_recording_still_reads() {
    let scratch = Scratch::new("refcount-damaged");
    let mut store = BlobStore::open(scratch.path().join("blobs")).unwrap();
    let shared = "a long answer both recordings quote, ".repeat(4);
    let first = write(&[typed(1, "hello"), typed(2, &shared), typed(3, "bye")], &mut store);
    let second = write(&[typed(1, &shared)], &mut store);
    let text = String::from_utf8(first).unwrap();
    let damaged = text.replacen(r#""mono":2000000,"#, r#""mono":2000000,,"#, 1);
    assert_ne!(damaged, text, "line 2 no longer parses");

    let released = release_blobs(Cursor::new(damaged.as_bytes()), &mut store).unwrap();
    assert_eq!(released, ReleaseReport { damaged: 1, ..report(1, 0) }, "the stub in line 2");
    assert_eq!(store.refs(&blob_ref(shared.as_bytes())).unwrap(), 1, "the second's reference");
    let read = Reader::new(Cursor::new(&second)).read_all(&store).unwrap();
    assert_eq!(read, [typed(1, &shared)], "the second still reads");
}

#[test]
fn a_last_line_cut_after_its_stub_gives_the_stub_up() {
    let scratch = Scratch::new("refcount-cut");
    let mut store = BlobStore::open(scratch.path().join("blobs")).unwrap();
    let long = "a long answer the writer stopped part way through, ".repeat(4);
    let text = String::from_utf8(write(&[typed(1, "hello"), typed(2, &long)], &mut store)).unwrap();
    let stub = format!(r#"{{"$blob":"{}"}}"#, blob_ref(long.as_bytes()));
    let cut = text.find(&stub).unwrap() + stub.len() + 2;
    let released = release_blobs(Cursor::new(text.get(..cut).unwrap()), &mut store).unwrap();
    assert_eq!(released, ReleaseReport { damaged: 1, ..report(1, 1) }, "the cut line's stub");
    assert_eq!(files(&store), Vec::<String>::new(), "no blob left");
}

/// Takes `room` bytes, then refuses the rest.
struct Full {
    taken: Vec<u8>,
    room: usize,
}

impl Write for Full {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let room = self.room - self.taken.len();
        if room == 0 {
            return Err(std::io::Error::other("the disk is full"));
        }
        let n = buf.len().min(room);
        self.taken.extend_from_slice(buf.get(..n).unwrap());
        Ok(n)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn a_line_the_output_refuses_part_way_releases_the_stubs_it_did_not_take() {
    let scratch = Scratch::new("refcount-full");
    let mut store = BlobStore::open(scratch.path().join("blobs")).unwrap();
    let (took, refused) = ("the part the disk took, ".repeat(4), "the part it refused, ".repeat(4));
    let line = pre_tool_use(1, 1, json!({ "a": took, "b": refused }));
    let text = line::encode(&line, 64, &mut MemoryBlobs::default()).unwrap();
    let stub = format!(r#"{{"$blob":"{}"}}"#, blob_ref(took.as_bytes()));
    let room = text.find(&stub).unwrap() + stub.len() + 2;
    let mut writer = Writer::new(Full { taken: Vec::new(), room }, &mut store).with_max_inline(64);
    assert!(matches!(writer.write(&line), Err(WriteError::Io(_))), "the disk is full");
    let out = writer.finish().unwrap().0;
    assert_eq!(store.refs(&blob_ref(took.as_bytes())).unwrap(), 1, "the stub the disk took");
    assert_eq!(store.refs(&blob_ref(refused.as_bytes())).unwrap(), 0, "the one it did not");

    let released = release_blobs(Cursor::new(&out.taken), &mut store).unwrap();
    assert_eq!(released, ReleaseReport { damaged: 1, ..report(1, 1) }, "the stub it took");
    assert_eq!(files(&store), Vec::<String>::new(), "nothing counted twice or left behind");
}
