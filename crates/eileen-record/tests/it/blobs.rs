//! Blob reference counts through recordings: every stub a writer wrote holds one reference, and
//! deleting a recording releases them, so a blob no other recording names is deleted with it.

use std::io::Cursor;

use eileen_contracts::record::RecordLine;
use eileen_record::blob::{BlobStore, Blobs, blob_ref};
use eileen_record::reader::{Reader, ReleaseReport, release_blobs};
use eileen_record::writer::Writer;

use crate::support::{Scratch, typed};

fn write(lines: &[RecordLine], store: &mut BlobStore) -> Vec<u8> {
    let mut writer = Writer::new(Vec::new(), store).with_max_inline(64);
    for line in lines {
        writer.write(line).unwrap();
    }
    writer.finish().unwrap().0
}

/// Every file under the store's folder.
fn files(store: &BlobStore) -> Vec<String> {
    let mut files = Vec::new();
    for folder in std::fs::read_dir(store.root()).unwrap() {
        for file in std::fs::read_dir(folder.unwrap().path()).unwrap() {
            files.push(file.unwrap().file_name().to_string_lossy().into_owned());
        }
    }
    files.sort();
    files
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

    let report = release_blobs(Cursor::new(&first), &mut store).unwrap();
    assert_eq!(report, ReleaseReport { released: 3, deleted: 1, absent: 0 }, "the first deleted");
    assert_eq!(store.get(&own_blob).unwrap(), None, "the unreferenced blob is gone");
    assert_eq!(store.refs(&shared_blob).unwrap(), 1, "the second's reference remains");
    let read = Reader::new(Cursor::new(&second)).read_all(&store).unwrap();
    assert_eq!(read, [typed(1, &shared)], "the second still reads");

    let report = release_blobs(Cursor::new(&second), &mut store).unwrap();
    assert_eq!(report, ReleaseReport { released: 1, deleted: 1, absent: 0 }, "the second deleted");
    assert_eq!(files(&store), Vec::<String>::new(), "no blob and no count left");

    let again = release_blobs(Cursor::new(&second), &mut store).unwrap();
    assert_eq!(again, ReleaseReport { released: 1, deleted: 0, absent: 1 }, "nothing to release");
}
