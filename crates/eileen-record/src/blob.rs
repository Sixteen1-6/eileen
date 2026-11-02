//! The content-addressed blob store (BUILD.md sections 2.15 and 3.3.16): `blobs/<2 hex>/<hash>.zst`
//! under the data directory, shared by recordings, incidents and trajectories. A blob is named by
//! the BLAKE3 hash of its bytes ([`blob_ref`]), stored zstd-compressed, and counted: every
//! reference a writer adds is one count in `<hash>.refs` beside it, and the blob is deleted when
//! the last reference is released, so a forget leaves no unreferenced blob behind.
//!
//! This is the only module of the crate that uses vendored C (zstd, check 24). Its calls block on
//! the disk, so async callers run them on the runtime's blocking pool.
//!
//! A count is read, changed and written back, so one writer at a time: [`BlobStore::open`]
//! takes an exclusive lock on `<root>/.lock` that lasts as long as the store, and a second open
//! of the same folder, from this process or another, fails with [`BlobStoreError::Locked`].
//! Callers in one process share one store (behind a mutex, say), and its calls take
//! `&mut self`, so counts change one at a time.
//!
//! Each file is written as `<name>.part`, synced, and renamed into place, and on Linux the
//! folder is synced after the rename, so a crash or a power cut leaves the old file or the new
//! one (on Windows the rename rests on NTFS's journal). A `.part` file left by a crash is
//! replaced by the next write of its name and removed with its blob. A file damaged all the
//! same is repaired, never trusted: a blob that does not decompress to its hash is written again
//! by the next `put` of its bytes, and a count that does not read is taken as
//! [`UNKNOWN_COUNT`], so the blob is kept until a sweep recounts its references. A count may be
//! too high, which keeps a blob too long, but never too low, which would delete one a recording
//! still names.

use std::collections::BTreeMap;
use std::fs::{File, OpenOptions, TryLockError};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use eileen_contracts::record::BlobRef;

/// The largest blob the store keeps or reads back, in bytes before compression.
pub const MAX_BLOB_BYTES: usize = 64 << 20;

/// The zstd level blobs are written at: zstd's default, fast to write on the core's blocking
/// pool, with most of the gain on text and base64.
const LEVEL: i32 = 3;

/// The longest reference count file the store reads: a decimal `u64` and a newline.
const MAX_REFS_FILE_BYTES: u64 = 32;

/// The count a damaged count file is read as: more references than any data directory holds,
/// so releasing them one by one never reaches 0 and the blob stays until a sweep recounts it.
pub const UNKNOWN_COUNT: u64 = u64::MAX / 2;

/// The lock file of a store's folder.
const LOCK_FILE: &str = ".lock";

/// The name of `bytes` in the store: `b3:` and their BLAKE3 hash.
#[must_use]
pub fn blob_ref(bytes: &[u8]) -> BlobRef {
    BlobRef::from_digest(*blake3::hash(bytes).as_bytes())
}

/// Why a blob store call failed.
#[derive(Debug, thiserror::Error)]
pub enum BlobStoreError {
    /// A file of the store could not be read, written, renamed or removed.
    #[error("could not {op} {path}")]
    Io {
        /// What was being done.
        op: &'static str,
        /// The file or folder.
        path: PathBuf,
        /// The error.
        #[source]
        source: std::io::Error,
    },
    /// A blob over [`MAX_BLOB_BYTES`].
    #[error("a blob of {bytes} bytes is over the store's limit")]
    TooLarge {
        /// Its size.
        bytes: usize,
    },
    /// A stored blob that does not decompress to bytes with its own hash.
    #[error("the blob {0} is corrupt")]
    Corrupt(BlobRef),
    /// A reference was added to a blob the store does not hold.
    #[error("the blob {0} is not in the store")]
    Missing(BlobRef),
    /// Another store, in this process or another, holds the folder's lock.
    #[error("another writer holds the blob store at {0}")]
    Locked(PathBuf),
}

/// What releasing one reference did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Released {
    /// Other references remain: this many.
    Kept(u64),
    /// That was the last reference, and the blob is deleted.
    Deleted,
    /// The store did not hold the blob (a forget deleted it already).
    Absent,
}

/// Where a writer puts long strings and a reader finds them: the disk store, or the in-memory
/// one tests and the fuzz target use.
pub trait Blobs {
    /// Keeps `bytes` and adds one reference to them, returning their name.
    fn put(&mut self, bytes: &[u8]) -> Result<BlobRef, BlobStoreError>;

    /// The bytes of a blob, or `None` when the store does not hold it.
    fn get(&self, blob: &BlobRef) -> Result<Option<Vec<u8>>, BlobStoreError>;

    /// Releases one reference, deleting the blob with its last one.
    fn release(&mut self, blob: &BlobRef) -> Result<Released, BlobStoreError>;
}

impl<T: Blobs + ?Sized> Blobs for &mut T {
    fn put(&mut self, bytes: &[u8]) -> Result<BlobRef, BlobStoreError> {
        (**self).put(bytes)
    }

    fn get(&self, blob: &BlobRef) -> Result<Option<Vec<u8>>, BlobStoreError> {
        (**self).get(blob)
    }

    fn release(&mut self, blob: &BlobRef) -> Result<Released, BlobStoreError> {
        (**self).release(blob)
    }
}

/// The blob store of a data directory, rooted at its `blobs` folder.
#[derive(Debug)]
pub struct BlobStore {
    root: PathBuf,
    /// The open lock file, whose lock is released when the store is dropped.
    _lock: File,
}

/// The files of one blob.
struct Files {
    folder: PathBuf,
    blob: PathBuf,
    refs: PathBuf,
}

impl BlobStore {
    /// Opens the store at `root`, creating the folder when it is not there, and takes the
    /// exclusive lock on its `.lock` file that it keeps until it is dropped. The runtime opens
    /// a data directory's store once and shares it; a second open while the first is alive
    /// fails with [`BlobStoreError::Locked`] rather than wait.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, BlobStoreError> {
        let root = root.into();
        std::fs::create_dir_all(&root).map_err(io("create", &root))?;
        let path = root.join(LOCK_FILE);
        let lock = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&path)
            .map_err(io("open", &path))?;
        match lock.try_lock() {
            Ok(()) => Ok(Self { root, _lock: lock }),
            Err(TryLockError::WouldBlock) => Err(BlobStoreError::Locked(root)),
            Err(TryLockError::Error(error)) => Err(io("lock", &path)(error)),
        }
    }

    /// The store's folder.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Where a blob's files sit: `<root>/<2 hex>/<hash>.zst` and `<hash>.refs`.
    fn files(&self, blob: &BlobRef) -> Files {
        let hex = blob.as_str().strip_prefix("b3:").unwrap_or(blob.as_str());
        let folder = self.root.join(hex.get(..2).unwrap_or(hex));
        Files {
            blob: folder.join(format!("{hex}.zst")),
            refs: folder.join(format!("{hex}.refs")),
            folder,
        }
    }

    /// How many references the blob has; 0 when the store does not hold it, and
    /// [`UNKNOWN_COUNT`] or more when its count file was damaged.
    pub fn refs(&self, blob: &BlobRef) -> Result<u64, BlobStoreError> {
        let files = self.files(blob);
        if !exists(&files.blob)? {
            return Ok(0);
        }
        read_count(&files.refs)
    }

    /// Adds a reference to a blob the store holds, returning the new count.
    pub fn retain(&mut self, blob: &BlobRef) -> Result<u64, BlobStoreError> {
        let files = self.files(blob);
        if !exists(&files.blob)? {
            return Err(BlobStoreError::Missing(blob.clone()));
        }
        let count = read_count(&files.refs)?.saturating_add(1);
        write_atomic(&files.refs, format!("{count}\n").as_bytes())?;
        Ok(count)
    }

    /// Writes a blob's compressed bytes under its name.
    fn write_blob(files: &Files, bytes: &[u8]) -> Result<(), BlobStoreError> {
        std::fs::create_dir_all(&files.folder).map_err(io("create", &files.folder))?;
        let packed = zstd::bulk::compress(bytes, LEVEL).map_err(io("compress", &files.blob))?;
        write_atomic(&files.blob, &packed)
    }
}

impl Blobs for BlobStore {
    fn put(&mut self, bytes: &[u8]) -> Result<BlobRef, BlobStoreError> {
        if bytes.len() > MAX_BLOB_BYTES {
            return Err(BlobStoreError::TooLarge { bytes: bytes.len() });
        }
        let blob = blob_ref(bytes);
        let files = self.files(&blob);
        match self.get(&blob) {
            Ok(Some(_)) => {}
            Ok(None) => {
                Self::write_blob(&files, bytes)?;
                // A count left beside a lost blob, or by a release cut part way, is counted on
                // from rather than reset: recordings may still name the blob, and a count too
                // high only keeps it longer.
                let count = if exists(&files.refs)? {
                    read_count(&files.refs)?.saturating_add(1)
                } else {
                    1
                };
                write_atomic(&files.refs, format!("{count}\n").as_bytes())?;
                return Ok(blob);
            }
            // Damaged on disk: written again, its count kept, since recordings still name it.
            Err(BlobStoreError::Corrupt(_)) => Self::write_blob(&files, bytes)?,
            Err(error) => return Err(error),
        }
        self.retain(&blob)?;
        Ok(blob)
    }

    fn get(&self, blob: &BlobRef) -> Result<Option<Vec<u8>>, BlobStoreError> {
        let path = self.files(blob).blob;
        let packed = match std::fs::read(&path) {
            Ok(packed) => packed,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(io("read", &path)(error)),
        };
        let bytes = zstd::bulk::decompress(&packed, MAX_BLOB_BYTES)
            .map_err(|_| BlobStoreError::Corrupt(blob.clone()))?;
        if blob_ref(&bytes) != *blob {
            return Err(BlobStoreError::Corrupt(blob.clone()));
        }
        Ok(Some(bytes))
    }

    fn release(&mut self, blob: &BlobRef) -> Result<Released, BlobStoreError> {
        let files = self.files(blob);
        if !exists(&files.blob)? {
            remove_leftovers(&files);
            return Ok(Released::Absent);
        }
        let count = read_count(&files.refs)?;
        if count > 1 {
            let left = count - 1;
            write_atomic(&files.refs, format!("{left}\n").as_bytes())?;
            return Ok(Released::Kept(left));
        }
        remove_if_there(&files.blob)?;
        remove_leftovers(&files);
        Ok(Released::Deleted)
    }
}

/// An in-memory store with the same names and counts as [`BlobStore`], for tests, the
/// simulator and the fuzz target.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MemoryBlobs {
    blobs: BTreeMap<BlobRef, (Vec<u8>, u64)>,
}

impl MemoryBlobs {
    /// How many references the blob has; 0 when the store does not hold it.
    #[must_use]
    pub fn refs(&self, blob: &BlobRef) -> u64 {
        self.blobs.get(blob).map_or(0, |(_, count)| *count)
    }

    /// How many blobs it holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.blobs.len()
    }

    /// Whether it holds no blob.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.blobs.is_empty()
    }
}

impl Blobs for MemoryBlobs {
    fn put(&mut self, bytes: &[u8]) -> Result<BlobRef, BlobStoreError> {
        if bytes.len() > MAX_BLOB_BYTES {
            return Err(BlobStoreError::TooLarge { bytes: bytes.len() });
        }
        let blob = blob_ref(bytes);
        let entry = self.blobs.entry(blob.clone()).or_insert_with(|| (bytes.to_vec(), 0));
        entry.1 = entry.1.saturating_add(1);
        Ok(blob)
    }

    fn get(&self, blob: &BlobRef) -> Result<Option<Vec<u8>>, BlobStoreError> {
        Ok(self.blobs.get(blob).map(|(bytes, _)| bytes.clone()))
    }

    fn release(&mut self, blob: &BlobRef) -> Result<Released, BlobStoreError> {
        let Some((_, count)) = self.blobs.get_mut(blob) else { return Ok(Released::Absent) };
        if *count > 1 {
            *count -= 1;
            return Ok(Released::Kept(*count));
        }
        self.blobs.remove(blob);
        Ok(Released::Deleted)
    }
}

/// An IO error's mapping, naming what was done to which file.
fn io<'a>(op: &'static str, path: &'a Path) -> impl FnOnce(std::io::Error) -> BlobStoreError + 'a {
    move |source| BlobStoreError::Io { op, path: path.to_path_buf(), source }
}

fn exists(path: &Path) -> Result<bool, BlobStoreError> {
    path.try_exists().map_err(io("look for", path))
}

/// A blob's reference count. A missing file counts 1, since the blob file is written first; a
/// file that does not hold a count above 0 counts [`UNKNOWN_COUNT`], which can only keep the
/// blob too long.
fn read_count(path: &Path) -> Result<u64, BlobStoreError> {
    let text = match std::fs::metadata(path) {
        Ok(meta) if meta.len() > MAX_REFS_FILE_BYTES => return Ok(UNKNOWN_COUNT),
        Ok(_) => std::fs::read(path).map_err(io("read", path))?,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(1),
        Err(error) => return Err(io("read", path)(error)),
    };
    let count = std::str::from_utf8(&text).ok().and_then(|text| text.trim_end().parse().ok());
    Ok(count.filter(|count| *count > 0).unwrap_or(UNKNOWN_COUNT))
}

/// The file a write of `path` goes to before it is renamed into place.
fn part(path: &Path) -> PathBuf {
    let mut part = path.as_os_str().to_owned();
    part.push(".part");
    PathBuf::from(part)
}

/// Writes `bytes` beside `path`, syncs them, and renames the file into place; on Linux the
/// folder is synced too, so the rename survives a power cut.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), BlobStoreError> {
    let part = part(path);
    let mut file = File::create(&part).map_err(io("create", &part))?;
    file.write_all(bytes).map_err(io("write", &part))?;
    file.sync_all().map_err(io("sync", &part))?;
    drop(file);
    std::fs::rename(&part, path).map_err(io("rename", path))?;
    sync_folder(path)
}

/// Syncs the folder that holds `path`, which makes a rename in it durable on Linux. Windows
/// cannot open a folder as a file to sync it, and NTFS journals the rename.
fn sync_folder(path: &Path) -> Result<(), BlobStoreError> {
    let Some(folder) = path.parent().filter(|_| !cfg!(windows)) else { return Ok(()) };
    File::open(folder).and_then(|folder| folder.sync_all()).map_err(io("sync", folder))
}

/// Removes what is left of a blob whose `.zst` is gone: its count and any `.part` file a crash
/// left. Each is tried; one that stays is harmless, since the next `put` of the blob replaces
/// a `.part` file and counts on from a count, so the blob is reported deleted all the same.
fn remove_leftovers(files: &Files) {
    for path in [files.refs.clone(), part(&files.blob), part(&files.refs)] {
        let _left_if_failed = remove_if_there(&path);
    }
}

fn remove_if_there(path: &Path) -> Result<(), BlobStoreError> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io("remove", path)(error)),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use eileen_contracts::record::BlobRef;

    use super::{BlobStore, BlobStoreError, Blobs, MemoryBlobs, Released, UNKNOWN_COUNT, blob_ref};
    use crate::test_support::Scratch;

    #[test]
    fn a_blob_is_named_by_the_blake3_hash_of_its_bytes() {
        // The reference vector of the BLAKE3 specification for the empty input.
        assert_eq!(
            blob_ref(b"").as_str(),
            "b3:af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262",
            "the empty input"
        );
    }

    #[test]
    fn a_blob_is_written_compressed_under_its_hash_and_reads_back() {
        let scratch = Scratch::new("blob-layout");
        let mut store = BlobStore::open(scratch.path().join("blobs")).unwrap();
        let text = "iVBOR".repeat(4000);
        let blob = store.put(text.as_bytes()).unwrap();
        let hex = blob.as_str().strip_prefix("b3:").unwrap();
        let file = store.root().join(hex.get(..2).unwrap()).join(format!("{hex}.zst"));
        let on_disk = std::fs::read(&file).unwrap();
        assert!(on_disk.len() < text.len() / 10, "compressed: {} bytes", on_disk.len());
        assert_eq!(store.get(&blob).unwrap().as_deref(), Some(text.as_bytes()), "reads back");
        assert_eq!(store.refs(&blob).unwrap(), 1, "one reference");
    }

    #[test]
    fn a_blob_whose_file_was_changed_reads_as_corrupt() {
        let scratch = Scratch::new("blob-corrupt");
        let mut store = BlobStore::open(scratch.path()).unwrap();
        let blob = store.put(b"the first text").unwrap();
        let other = zstd::bulk::compress(b"another text", 3).unwrap();
        let hex = blob.as_str().strip_prefix("b3:").unwrap();
        std::fs::write(store.root().join(hex.get(..2).unwrap()).join(format!("{hex}.zst")), other)
            .unwrap();
        assert!(store.get(&blob).is_err(), "a hash mismatch is an error, never other bytes");
    }

    #[test]
    fn releasing_the_last_reference_deletes_the_blob() {
        let scratch = Scratch::new("blob-release");
        let mut store = BlobStore::open(scratch.path()).unwrap();
        let blob = store.put(b"shared by two recordings").unwrap();
        assert_eq!(store.put(b"shared by two recordings").unwrap(), blob, "the same name");
        assert_eq!(store.refs(&blob).unwrap(), 2, "two references");
        assert_eq!(store.release(&blob).unwrap(), Released::Kept(1), "one left");
        assert!(store.get(&blob).unwrap().is_some(), "still held");
        assert_eq!(store.release(&blob).unwrap(), Released::Deleted, "the last one");
        assert_eq!(store.get(&blob).unwrap(), None, "deleted");
        let hex = blob.as_str().strip_prefix("b3:").unwrap();
        let folder = store.root().join(hex.get(..2).unwrap());
        assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 0, "no file left in {folder:?}");
        assert_eq!(store.release(&blob).unwrap(), Released::Absent, "nothing to release");
        assert!(store.retain(&blob).is_err(), "no reference to a blob that is gone");
    }

    /// A blob's `.zst` and `.refs` paths.
    fn paths(store: &BlobStore, blob: &BlobRef) -> (PathBuf, PathBuf) {
        let hex = blob.as_str().strip_prefix("b3:").unwrap();
        let folder = store.root().join(hex.get(..2).unwrap());
        (folder.join(format!("{hex}.zst")), folder.join(format!("{hex}.refs")))
    }

    #[test]
    fn a_second_store_on_the_same_folder_is_refused_while_the_first_is_open() {
        let scratch = Scratch::new("blob-lock");
        let store = BlobStore::open(scratch.path()).unwrap();
        assert!(
            matches!(BlobStore::open(scratch.path()), Err(BlobStoreError::Locked(_))),
            "one writer at a time"
        );
        drop(store);
        assert!(BlobStore::open(scratch.path()).is_ok(), "free once the first is dropped");
    }

    #[test]
    fn a_damaged_blob_is_written_again_by_the_next_put_of_its_bytes() {
        let scratch = Scratch::new("blob-empty");
        let mut store = BlobStore::open(scratch.path()).unwrap();
        let text = b"a blob a power cut emptied";
        let blob = store.put(text).unwrap();
        let (zst, _) = paths(&store, &blob);
        std::fs::write(&zst, b"").unwrap();
        assert!(matches!(store.get(&blob), Err(BlobStoreError::Corrupt(_))), "empty reads corrupt");
        assert_eq!(store.put(text).unwrap(), blob, "the same name");
        assert_eq!(store.get(&blob).unwrap().as_deref(), Some(text.as_slice()), "repaired");
        assert_eq!(store.refs(&blob).unwrap(), 2, "the first reference kept");
    }

    #[test]
    fn a_lost_blob_written_again_keeps_counting_from_its_count() {
        let scratch = Scratch::new("blob-lost");
        let mut store = BlobStore::open(scratch.path()).unwrap();
        let text = b"named by two recordings, then lost";
        let blob = store.put(text).unwrap();
        store.put(text).unwrap();
        let (zst, _) = paths(&store, &blob);
        std::fs::remove_file(&zst).unwrap();
        assert_eq!(store.put(text).unwrap(), blob, "written again");
        assert_eq!(store.refs(&blob).unwrap(), 3, "the two earlier references kept");
    }

    #[test]
    fn a_damaged_count_keeps_its_blob_rather_than_deleting_it() {
        let scratch = Scratch::new("blob-count");
        let mut store = BlobStore::open(scratch.path()).unwrap();
        let blob = store.put(b"named by recordings we cannot count").unwrap();
        let (_, refs) = paths(&store, &blob);
        std::fs::write(&refs, b"").unwrap();
        assert_eq!(store.refs(&blob).unwrap(), UNKNOWN_COUNT, "read as unknown");
        assert_eq!(store.retain(&blob).unwrap(), UNKNOWN_COUNT + 1, "a reference added");
        assert_eq!(store.put(b"named by recordings we cannot count").unwrap(), blob, "put works");
        assert_eq!(store.release(&blob).unwrap(), Released::Kept(UNKNOWN_COUNT + 1), "kept");
        assert!(store.get(&blob).unwrap().is_some(), "never deleted on a guess");
    }

    #[test]
    fn releasing_the_last_reference_removes_a_part_file_a_crash_left() {
        let scratch = Scratch::new("blob-part");
        let mut store = BlobStore::open(scratch.path()).unwrap();
        let blob = store.put(b"the user's words, compressed").unwrap();
        let (zst, refs) = paths(&store, &blob);
        for path in [&zst, &refs] {
            let mut part = path.as_os_str().to_owned();
            part.push(".part");
            std::fs::write(PathBuf::from(part), b"left by a crash").unwrap();
        }
        assert_eq!(store.release(&blob).unwrap(), Released::Deleted, "the last one");
        let folder = zst.parent().unwrap();
        assert_eq!(std::fs::read_dir(folder).unwrap().count(), 0, "no file left in {folder:?}");
    }

    #[test]
    fn the_memory_store_counts_like_the_disk_store() {
        let mut store = MemoryBlobs::default();
        let blob = store.put(b"text").unwrap();
        assert_eq!(store.put(b"text").unwrap(), blob, "the same name");
        assert_eq!(blob, blob_ref(b"text"), "named by its hash");
        assert_eq!(store.refs(&blob), 2, "two references");
        assert_eq!(store.release(&blob).unwrap(), Released::Kept(1), "one left");
        assert_eq!(store.release(&blob).unwrap(), Released::Deleted, "the last one");
        assert!(store.is_empty(), "dropped");
        assert_eq!(store.release(&blob).unwrap(), Released::Absent, "nothing to release");
    }
}
