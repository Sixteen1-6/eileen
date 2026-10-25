//! `eileen_platform::machine` (owner answer 6).

use std::path::{Path, PathBuf};

use eileen_contracts::ids::InstallId;
use eileen_platform::MachineIdentity;
use eileen_platform::machine::{FileMachineIdentity, machine_hash, normalise_os_id};

/// The golden vector's OS id, in Linux's plain form.
const OS_ID: &str = "0123456789abcdef0123456789abcdef";

/// The golden vector's install id: the bytes 0 to 15.
fn install() -> InstallId {
    InstallId::from_bytes([0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15])
}

/// A fresh, empty folder for one test, removed when dropped.
struct TempRoot(PathBuf);

impl TempRoot {
    fn new(name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("eileen-platform-it-{}-{name}", std::process::id()));
        let _ignored = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn write(&self, relative: &str, text: &str) {
        let path = self.0.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ignored = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn the_golden_vector_is_stable() {
    // Computed independently, with a from-scratch BLAKE3 over the documented encoding (each
    // part an 8-byte little-endian length, then its bytes), and checked on Windows and Linux.
    assert_eq!(
        machine_hash(Some(OS_ID), &install()).as_str(),
        "f8c2418263ede4074d5c19f04e922c1545e1c6228dafc237d5747e018cfe5b08",
        "with an OS id",
    );
    assert_eq!(
        machine_hash(None, &install()).as_str(),
        "12e043770ee8b1c80d39337bce5a9b6ae9caae505f492bc4cd2add1062bd6da7",
        "the install id alone",
    );
}

#[test]
fn windows_guid_form_and_linux_form_give_one_hash() {
    let guid = "{01234567-89AB-CDEF-0123-456789ABCDEF}";
    assert_eq!(normalise_os_id(guid).as_deref(), Some(OS_ID), "normalised");
    assert_eq!(
        machine_hash(Some(guid), &install()),
        machine_hash(Some(OS_ID), &install()),
        "one hash for both forms",
    );
    assert_eq!(
        normalise_os_id(" 0123456789ABCDEF0123456789abcdef\n").as_deref(),
        Some(OS_ID),
        "trimmed"
    );
}

#[test]
fn a_cloned_image_with_two_install_ids_gives_two_hashes() {
    let other = InstallId::from_bytes([9; 16]);
    assert_ne!(
        machine_hash(Some(OS_ID), &install()),
        machine_hash(Some(OS_ID), &other),
        "cloned images differ",
    );
}

#[test]
fn an_unusable_os_id_gives_the_install_id_only_hash() {
    let install_only = machine_hash(None, &install());
    for bad in [
        "",
        "not-a-machine-id",
        "0123456789abcdef0123456789abcde",
        "0123456789abcdef0123456789abcdef0",
        "0123456789abcdeg0123456789abcdef",
        "00000000-0000-0000-0000-000000000000",
        "uninitialized",
        "UNINITIALIZED\n",
    ] {
        assert_eq!(normalise_os_id(bad), None, "{bad:?} is unusable");
        assert_eq!(machine_hash(Some(bad), &install()), install_only, "{bad:?} counts as none");
    }
    assert_eq!(machine_hash(None, &install()), install_only, "stable across calls");
    assert_ne!(machine_hash(Some(OS_ID), &install()), install_only, "differs from an OS-id hash");
}

#[test]
fn file_identity_reads_etc_then_falls_back_to_dbus() {
    let root = TempRoot::new("fallback");
    let identity = FileMachineIdentity { root: root.path().to_path_buf() };
    assert_eq!(identity.os_machine_id().unwrap(), None, "neither file");

    root.write(FileMachineIdentity::FALLBACK, "fedcba9876543210fedcba9876543210\n");
    assert_eq!(
        identity.os_machine_id().unwrap().as_deref(),
        Some("fedcba9876543210fedcba9876543210"),
        "the fallback when the first is missing",
    );

    root.write(FileMachineIdentity::PRIMARY, "uninitialized\n");
    assert_eq!(
        identity.os_machine_id().unwrap().as_deref(),
        Some("fedcba9876543210fedcba9876543210"),
        "the fallback when the first is unusable",
    );

    root.write(FileMachineIdentity::PRIMARY, &format!("{OS_ID}\n"));
    assert_eq!(identity.os_machine_id().unwrap().as_deref(), Some(OS_ID), "the first when usable");

    root.write(FileMachineIdentity::PRIMARY, "");
    root.write(FileMachineIdentity::FALLBACK, "garbage");
    assert_eq!(identity.os_machine_id().unwrap(), None, "neither usable");
}
