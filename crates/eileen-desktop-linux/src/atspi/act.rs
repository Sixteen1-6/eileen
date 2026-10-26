//! What the client changes in an app: an object's actions (`Action`), its text (`EditableText`)
//! and its number (`Value`), as UI Automation's Invoke and Value patterns change them on Windows.
//!
//! These calls write whatever object they are given. The rules on what may be written (no
//! password field, nothing read-only) are their caller's, `Automation::set_value`'s, which
//! applies them to the facts [`Atspi::accessible`] read before any write.

use eileen_contracts::desktop::sanitize;
use eileen_contracts::ids::Untrusted;
use eileen_dbus_linux::DbusError;

use super::proxies::{self, ActionProxy, EditableTextProxy, ValueProxy};
use super::{Atspi, ObjectRef, object_failure};

/// What errors of an action's calls name.
const ACTION: &str = "accessible action";
/// What errors of a text's write name.
const TEXT_WRITE: &str = "accessible text write";
/// What errors of a number's read or write name.
const VALUE: &str = "accessible value";
/// The most characters of an action's name kept.
const ACTION_CHARS: usize = 100;

impl Atspi {
    /// The names of `object`'s first `at_most` actions, in order: `NActions`, then `GetName` of
    /// each, sanitized and kept to 100 characters. A negative count reads none. An object
    /// without `Action`, or whose app has gone, is [`DbusError::NotFound`].
    pub async fn actions(
        &self,
        object: &ObjectRef,
        at_most: u32,
    ) -> Result<Vec<Untrusted<String>>, DbusError> {
        let failed = |error: zbus::Error| object_failure(ACTION, &error);
        let action: ActionProxy<'_> = proxies::of(&self.conn, object).await.map_err(failed)?;
        let count = action.n_actions().await.map_err(failed)?;
        let count = count.clamp(0, i32::try_from(at_most).unwrap_or(i32::MAX));
        let mut names = Vec::new();
        for index in 0..count {
            names.push(sanitize(&action.get_name(index).await.map_err(failed)?, ACTION_CHARS));
        }
        Ok(names)
    }

    /// Runs `object`'s action at `index` (`DoAction`): whether its app says it ran it. An object
    /// without `Action`, or whose app has gone, is [`DbusError::NotFound`].
    pub async fn do_action(&self, object: &ObjectRef, index: i32) -> Result<bool, DbusError> {
        let failed = |error: zbus::Error| object_failure(ACTION, &error);
        let action: ActionProxy<'_> = proxies::of(&self.conn, object).await.map_err(failed)?;
        action.do_action(index).await.map_err(failed)
    }

    /// Replaces `object`'s whole text with `text` (`EditableText.SetTextContents`): whether its
    /// app says it took it. An object without `EditableText`, or whose app has gone, is
    /// [`DbusError::NotFound`].
    pub async fn set_text(&self, object: &ObjectRef, text: &str) -> Result<bool, DbusError> {
        let failed = |error: zbus::Error| object_failure(TEXT_WRITE, &error);
        let editable: EditableTextProxy<'_> =
            proxies::of(&self.conn, object).await.map_err(failed)?;
        editable.set_text_contents(text).await.map_err(failed)
    }

    /// `object`'s number (`Value.CurrentValue`). An object without `Value`, or whose app has
    /// gone, is [`DbusError::NotFound`].
    pub async fn number(&self, object: &ObjectRef) -> Result<f64, DbusError> {
        let failed = |error: zbus::Error| object_failure(VALUE, &error);
        let value: ValueProxy<'_> = proxies::of(&self.conn, object).await.map_err(failed)?;
        value.current_value().await.map_err(failed)
    }

    /// Sets `object`'s number to `number` (`CurrentValue`, through `Properties.Set`). The app
    /// may keep it within its range, or refuse it, so the number is read back to know what it
    /// kept. An object without `Value`, or whose app has gone, is [`DbusError::NotFound`].
    pub async fn set_number(&self, object: &ObjectRef, number: f64) -> Result<(), DbusError> {
        let failed = |error: zbus::Error| object_failure(VALUE, &error);
        let value: ValueProxy<'_> = proxies::of(&self.conn, object).await.map_err(failed)?;
        value.set_current_value(number).await.map_err(failed)
    }
}
