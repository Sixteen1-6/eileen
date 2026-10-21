//! The distribution from `os-release` and the kernel release.

use crate::SysRoot;

/// The `os-release` fields the build string uses.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct OsRelease {
    /// `ID`, such as `ubuntu`.
    pub(crate) id: Option<String>,
    /// `VERSION_ID`, such as `24.04`.
    pub(crate) version_id: Option<String>,
}

/// Reads `os-release` (freedesktop.org's format: `KEY=value`, values optionally in double or
/// single quotes, with backslash escapes inside double quotes).
pub(crate) fn parse_os_release(text: &str) -> OsRelease {
    let mut release = OsRelease::default();
    for line in text.lines() {
        let Some((key, raw)) = line.trim().split_once('=') else {
            continue;
        };
        let value = unquote(raw.trim());
        match key {
            "ID" => release.id = Some(value),
            "VERSION_ID" => release.version_id = Some(value),
            _ => {}
        }
    }
    release
}

/// A value without its quotes, with the escapes of double-quoted values resolved.
fn unquote(raw: &str) -> String {
    if let Some(inner) = raw.strip_prefix('"').and_then(|rest| rest.strip_suffix('"')) {
        let mut out = String::with_capacity(inner.len());
        let mut chars = inner.chars();
        while let Some(c) = chars.next() {
            if c == '\\' {
                if let Some(escaped) = chars.next() {
                    out.push(escaped);
                }
            } else {
                out.push(c);
            }
        }
        return out;
    }
    if let Some(inner) = raw.strip_prefix('\'').and_then(|rest| rest.strip_suffix('\'')) {
        return inner.to_owned();
    }
    raw.to_owned()
}

/// Whether the kernel release is WSL's (`...-microsoft-standard-WSL2`).
pub(crate) fn is_wsl(kernel_release: &str) -> bool {
    kernel_release.to_ascii_lowercase().contains("microsoft")
}

/// The OS build string, `<id> <version>, kernel <release>`, and the kernel release on its own.
pub(crate) fn read_os(root: &SysRoot, degraded: &mut Vec<String>) -> (String, String) {
    let release =
        root.read("etc/os-release").or_else(|_| root.read("usr/lib/os-release")).map_or_else(
            |_| {
                degraded.push("os release: no etc/os-release or usr/lib/os-release".to_owned());
                OsRelease::default()
            },
            |text| parse_os_release(&text),
        );
    let kernel = root.read_trimmed("proc/sys/kernel/osrelease").unwrap_or_else(|| {
        degraded.push("kernel release: /proc/sys/kernel/osrelease is unreadable".to_owned());
        "unknown".to_owned()
    });
    let distro = match (release.id, release.version_id) {
        (Some(id), Some(version)) => format!("{id} {version}"),
        (Some(id), None) => id,
        (None, _) => "linux".to_owned(),
    };
    (format!("{distro}, kernel {kernel}"), kernel)
}

#[cfg(test)]
mod tests {
    use super::{is_wsl, parse_os_release, unquote};

    #[test]
    fn ubuntu_os_release_gives_id_and_version() {
        let text = "PRETTY_NAME=\"Ubuntu 24.04.4 LTS\"\nNAME=\"Ubuntu\"\nVERSION_ID=\"24.04\"\n\
                    VERSION_CODENAME=noble\nID=ubuntu\nID_LIKE=debian\n";
        let release = parse_os_release(text);
        assert_eq!(release.id.as_deref(), Some("ubuntu"), "unquoted ID");
        assert_eq!(release.version_id.as_deref(), Some("24.04"), "quoted VERSION_ID");
    }

    #[test]
    fn quotes_and_escapes_follow_the_os_release_format() {
        assert_eq!(unquote("\"a \\\"b\\\" c\""), "a \"b\" c", "escaped quotes");
        assert_eq!(unquote("'single'"), "single", "single quotes");
        assert_eq!(unquote("bare"), "bare", "no quotes");
        assert_eq!(unquote("\"unterminated"), "\"unterminated", "left alone");
    }

    #[test]
    fn wsl_kernels_are_recognised() {
        assert!(is_wsl("6.18.33.2-microsoft-standard-WSL2"), "WSL2");
        assert!(!is_wsl("6.8.0-45-generic"), "Ubuntu's own kernel");
    }
}
