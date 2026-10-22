//! The monitor count from the kernel's DRM connectors (`/sys/class/drm/card<N>-<connector>`).
//!
//! A connector counts when its `status` is `connected` and it is not `disabled` (a monitor the
//! user turned off in the display settings is connected but drives nothing). The compositor's
//! own outputs, with their positions and scales, are the desktop's (phase L2).

use crate::SysRoot;

/// Whether a `drm` entry is a connector: `card<N>-<name>`, such as `card1-eDP-1`.
fn is_connector(name: &str) -> bool {
    name.strip_prefix("card").and_then(|rest| rest.split_once('-')).is_some_and(|(card, port)| {
        !card.is_empty() && card.bytes().all(|b| b.is_ascii_digit()) && !port.is_empty()
    })
}

/// Counts the connected monitors; when the kernel lists no connectors at all, the count is
/// unknown and named in `degraded`.
pub(crate) fn read_monitors(root: &SysRoot, wsl: bool, degraded: &mut Vec<String>) -> u8 {
    let connectors: Vec<String> =
        root.entries("sys/class/drm").into_iter().filter(|name| is_connector(name)).collect();
    if connectors.is_empty() {
        degraded.push(if wsl {
            "monitors: under WSL the displays are the host's, which the kernel does not list"
                .to_owned()
        } else {
            "monitors: the kernel lists no display connectors (a headless PC, or a driver \
             without kernel mode setting)"
                .to_owned()
        });
        return 0;
    }
    let read = |name: &str, file: &str| root.read_trimmed(&format!("sys/class/drm/{name}/{file}"));
    let count = connectors
        .iter()
        .filter(|name| {
            read(name, "status").as_deref() == Some("connected")
                && read(name, "enabled").as_deref() != Some("disabled")
        })
        .count();
    u8::try_from(count).unwrap_or(u8::MAX)
}

#[cfg(test)]
mod tests {
    use super::is_connector;

    #[test]
    fn connectors_are_card_number_dash_port() {
        assert!(is_connector("card1-eDP-1"), "eDP");
        assert!(is_connector("card0-HDMI-A-1"), "a dash inside the port name");
        assert!(!is_connector("card1"), "a card");
        assert!(!is_connector("renderD128"), "a render node");
        assert!(!is_connector("card-x"), "no card number");
        assert!(!is_connector("card1-"), "no port");
        assert!(!is_connector("version"), "the version file");
    }
}
