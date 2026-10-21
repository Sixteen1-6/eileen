//! The OS's machine id, for the fleet's machine hash.

use crate::PlatformError;

/// The OS's own machine id. On Windows `eileen-shell-win` reads
/// `HKLM\SOFTWARE\Microsoft\Cryptography\MachineGuid`; on Linux
/// [`crate::machine::FileMachineIdentity`] reads `/etc/machine-id`; macOS later reads the
/// platform UUID. The runtime hashes it with the install id at every start.
pub trait MachineIdentity: Send + Sync {
    /// The id, or `None` when the OS has no usable one.
    fn os_machine_id(&self) -> Result<Option<String>, PlatformError>;
}

#[cfg(any(test, feature = "fake"))]
pub use fake::FakeMachineIdentity;

#[cfg(any(test, feature = "fake"))]
mod fake {
    use super::MachineIdentity;
    use crate::PlatformError;

    /// A fixed answer.
    #[derive(Debug, Clone, Default)]
    pub struct FakeMachineIdentity {
        /// What [`MachineIdentity::os_machine_id`] answers.
        pub id: Option<String>,
    }

    impl MachineIdentity for FakeMachineIdentity {
        fn os_machine_id(&self) -> Result<Option<String>, PlatformError> {
            Ok(self.id.clone())
        }
    }
}
