//! The fleet's machine identity (owner answer 6), one derivation for every OS.
//!
//! The hash is BLAKE3 over [`SALT`], the normalised OS machine id and the install id, each
//! written as an 8-byte little-endian length and then its bytes; with no usable OS id, over
//! the salt and the install id alone. The length prefixes make the encoding unambiguous: the
//! OS id is 32 bytes and the install id 16, so the two cases never hash the same bytes. The
//! salt is public, but the install id never leaves the PC, so nobody can recompute a
//! machine's hash from its OS id. Never hardware serials, user names or accounts.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use eileen_contracts::ids::{InstallId, MachineHash};

use crate::{MachineIdentity, PlatformError};

/// A fixed, public domain-separation string; it changes only with a new hash version.
pub const SALT: &str = "eileen fleet machine hash v1";

/// The OS id in one form for every OS: lower-case hex digits only, with braces and hyphens
/// dropped, so Windows' GUID form and Linux's plain 32-character form follow one rule.
/// Surrounding whitespace (the newline a machine-id file ends with) is ignored. `None` unless
/// exactly 32 hex digits remain, and for an all-zero or `uninitialized` value.
#[must_use]
pub fn normalise_os_id(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.eq_ignore_ascii_case("uninitialized") {
        return None;
    }
    let digits: String = trimmed
        .chars()
        .filter(|c| !matches!(c, '{' | '}' | '-'))
        .map(|c| c.to_ascii_lowercase())
        .collect();
    let usable = digits.len() == 32
        && digits.bytes().all(|b| b.is_ascii_hexdigit())
        && digits.bytes().any(|b| b != b'0');
    usable.then_some(digits)
}

/// The machine hash for an OS id (raw, as the OS gave it) and the install id. An OS id that
/// [`normalise_os_id`] rejects counts as none.
#[must_use]
pub fn machine_hash(os_id: Option<&str>, install: &InstallId) -> MachineHash {
    let mut hasher = blake3::Hasher::new();
    let mut part = |bytes: &[u8]| {
        let len = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        hasher.update(&len.to_le_bytes());
        hasher.update(bytes);
    };
    part(SALT.as_bytes());
    if let Some(os_id) = os_id.and_then(normalise_os_id) {
        part(os_id.as_bytes());
    }
    part(install.as_bytes());
    MachineHash::from_digest(*hasher.finalize().as_bytes())
}

/// [`MachineIdentity`] from `<root>/etc/machine-id`, then `<root>/var/lib/dbus/machine-id`
/// (root `/` on Linux; a temporary folder in tests on every OS).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileMachineIdentity {
    /// The file system root the two paths are under.
    pub root: PathBuf,
}

impl FileMachineIdentity {
    /// The first file, relative to the root.
    pub const PRIMARY: &'static str = "etc/machine-id";
    /// The fallback, relative to the root.
    pub const FALLBACK: &'static str = "var/lib/dbus/machine-id";

    /// The identity under the real root, `/`.
    #[must_use]
    pub fn system() -> Self {
        Self { root: PathBuf::from("/") }
    }
}

/// Reads one machine-id file: `Ok(None)` when it is missing or holds no usable id.
fn read_id(path: &Path) -> Result<Option<String>, PlatformError> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(normalise_os_id(&text)),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        // A file that is not UTF-8 is not a usable id; anything else is a real read failure,
        // which the runtime reports instead of silently changing the machine's hash.
        Err(e) if e.kind() == ErrorKind::InvalidData => Ok(None),
        Err(e) => Err(PlatformError::io("machine-id", &e)),
    }
}

impl MachineIdentity for FileMachineIdentity {
    fn os_machine_id(&self) -> Result<Option<String>, PlatformError> {
        if let Some(id) = read_id(&self.root.join(Self::PRIMARY))? {
            return Ok(Some(id));
        }
        read_id(&self.root.join(Self::FALLBACK))
    }
}
