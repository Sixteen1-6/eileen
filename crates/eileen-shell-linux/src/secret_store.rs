//! [`LinuxSecretStore`]: `eileen-platform`'s `SecretStore` on Linux, over the Secret Service
//! (BUILD.md sections 2.12 and 3.4; docs/LINUX.md section 4). It is thin: each method checks the
//! name, then makes one call of this crate's client on `eileen-dbus-linux`'s [`Bridge`], on the
//! shared session bus, within the settings' deadline, past which it answers
//! `PlatformError::DeadlineExceeded`. The client's doc (`secrets`) says what it sends and why it
//! never prompts: a locked secret is `PlatformError::Refused`, "locked", and so is a change the
//! service would ask the user to confirm, "needs the user's confirmation".

use eileen_dbus_linux::Bridge;
use eileen_platform::{Deadline, PlatformError, SecretStore, SecretString};
use secrecy::ExposeSecret;

use crate::secrets::SecretService;

/// What the calls' errors name.
const GET: &str = "secret lookup";
const SET: &str = "secret store";
const DELETE: &str = "secret removal";
const NAME: &str = "secret name";

/// What a [`LinuxSecretStore`] needs besides its bridge, given by the composition root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretSettings {
    /// The `service` lookup attribute of every item, such as `eileen`; each secret's name is its
    /// `username` attribute.
    pub service: String,
    /// How long each method may take: `eileen_dbus_linux::DEFAULT_DEADLINE`, as for any trait
    /// method that takes none, unless a caller, such as a test, sets a shorter one.
    pub deadline: Deadline,
}

/// `SecretStore` on Linux, over a [`Bridge`]. It holds no secret, so its `Debug` shows none.
#[derive(Debug)]
pub struct LinuxSecretStore {
    bridge: Bridge,
    settings: SecretSettings,
}

impl LinuxSecretStore {
    /// A store on `bridge`, a clone of the process's one bridge, as `settings` say. It calls
    /// nothing yet.
    #[must_use]
    pub const fn new(bridge: Bridge, settings: SecretSettings) -> Self {
        Self { bridge, settings }
    }
}

impl SecretStore for LinuxSecretStore {
    /// The first unlocked item's secret; `Refused` when only locked items match.
    fn get(&self, name: &str) -> Result<Option<SecretString>, PlatformError> {
        let name = checked(name)?;
        let service = self.settings.service.clone();
        self.bridge.call(GET, self.settings.deadline, move |buses| async move {
            let session = buses.session().await?;
            Ok(SecretService::new(&session, &service).get(&name).await?)
        })
    }

    /// Stores in the default collection, replacing the item with the same name there. The value
    /// is copied into the job as a `SecretString`, zeroized when it drops.
    fn set(&self, name: &str, value: &SecretString) -> Result<(), PlatformError> {
        let name = checked(name)?;
        let service = self.settings.service.clone();
        let value = SecretString::from(value.expose_secret());
        self.bridge.call(SET, self.settings.deadline, move |buses| async move {
            let session = buses.session().await?;
            Ok(SecretService::new(&session, &service).set(&name, &value).await?)
        })
    }

    /// Removes every unlocked item with the name; `Refused`, removing none, when any is locked.
    fn delete(&self, name: &str) -> Result<(), PlatformError> {
        let name = checked(name)?;
        let service = self.settings.service.clone();
        self.bridge.call(DELETE, self.settings.deadline, move |buses| async move {
            let session = buses.session().await?;
            Ok(SecretService::new(&session, &service).delete(&name).await?)
        })
    }
}

/// `name`, owned for the job, unless it is empty or holds a NUL, which no D-Bus string can.
fn checked(name: &str) -> Result<String, PlatformError> {
    let detail = if name.is_empty() {
        "empty"
    } else if name.contains('\0') {
        "holds a NUL character"
    } else {
        return Ok(name.to_owned());
    };
    Err(PlatformError::Invalid { what: NAME, detail: detail.to_owned() })
}
