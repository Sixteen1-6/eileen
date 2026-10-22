//! A bookmark's path: its `href`, which must be a `file:` URI with no host or `localhost`
//! (RFC 8089, <https://www.rfc-editor.org/rfc/rfc8089>). It is read as `GLib`'s
//! `g_filename_from_uri` reads it
//! (<https://gitlab.gnome.org/GNOME/glib/-/blob/main/glib/gconvert.c>): a query or fragment
//! (`?`, `#`) is dropped, the rest is percent-decoded, and an escaped `/` or NUL or a bad escape
//! makes it unusable. A path that is not UTF-8 is skipped rather than altered, because the trait
//! holds a `String`. Other schemes (`sftp:`, `smb:`, `trash:`) are remote, and so is a host
//! naming this machine: knowing it is this one would mean reading the machine's name.

/// The local path a `file:` URI names, read as `GLib`'s `g_filename_from_uri` reads it: without
/// its query or fragment, and percent-decoded. `None` for another scheme, a host other than
/// `localhost`, an escaped `/` or NUL, a bad escape, or a path that is not UTF-8.
pub(super) fn local_path(uri: &str) -> Option<String> {
    let (scheme, rest) = uri.split_once(':')?;
    if !scheme.eq_ignore_ascii_case("file") {
        return None;
    }
    // A query or a fragment is not part of the path; `GLib` writes `?` and `#` escaped.
    let rest = rest.split(['?', '#']).next().unwrap_or_default();
    // What follows the path's first slash.
    let path = match rest.strip_prefix("//") {
        Some(after) => {
            let (host, path) = after.split_once('/')?;
            if !(host.is_empty() || host.eq_ignore_ascii_case("localhost")) {
                return None;
            }
            path
        }
        None => rest.strip_prefix('/')?,
    };
    let mut bytes = Vec::with_capacity(path.len() + 1);
    bytes.push(b'/');
    let mut rest = path.as_bytes();
    while let Some((&byte, after)) = rest.split_first() {
        rest = after;
        if byte != b'%' {
            bytes.push(byte);
            continue;
        }
        let [high, low, tail @ ..] = rest else {
            return None;
        };
        let value = (hex(*high)? << 4) | hex(*low)?;
        // No file name holds a slash or a NUL.
        if value == b'/' || value == 0 {
            return None;
        }
        bytes.push(value);
        rest = tail;
    }
    String::from_utf8(bytes).ok()
}

/// The value of a hexadecimal digit.
const fn hex(digit: u8) -> Option<u8> {
    match digit {
        b'0'..=b'9' => Some(digit - b'0'),
        b'a'..=b'f' => Some(digit - b'a' + 10),
        b'A'..=b'F' => Some(digit - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::local_path;

    #[test]
    fn file_uris_decode_to_local_paths() {
        let path = |uri: &str| local_path(uri);
        assert_eq!(
            path("file:///home/sam/a%20b.odt").as_deref(),
            Some("/home/sam/a b.odt"),
            "a space"
        );
        assert_eq!(
            path("file://localhost/home/sam/x").as_deref(),
            Some("/home/sam/x"),
            "localhost"
        );
        assert_eq!(path("file://LocalHost/x").as_deref(), Some("/x"), "the host without case");
        assert_eq!(path("FILE:///x").as_deref(), Some("/x"), "the scheme without case");
        assert_eq!(path("file:/home/sam/x").as_deref(), Some("/home/sam/x"), "one slash, as GLib");
        assert_eq!(
            path("file:///home/sam/R%C3%A9sum%C3%A9%2b.pdf").as_deref(),
            Some("/home/sam/Résumé+.pdf"),
            "escaped UTF-8 and a lowercase escape"
        );
        assert_eq!(
            path("file:///home/sam/Résumé.pdf").as_deref(),
            Some("/home/sam/Résumé.pdf"),
            "raw UTF-8"
        );
        assert_eq!(path("file:///a?b").as_deref(), Some("/a"), "a query is dropped, as GLib does");
        assert_eq!(path("file:///a#b").as_deref(), Some("/a"), "and so is a fragment");
        assert_eq!(
            path("file:///a%3Fb%23c").as_deref(),
            Some("/a?b#c"),
            "an escaped question mark and hash are part of the name"
        );
        for uri in [
            "sftp://nas.local/srv/x",
            "smb://nas.local/public/x",
            "trash:///x",
            "https://example.org/x",
            "file://nas.local/home/sam/x",
            "file:relative/x",
            "file:",
            "file://",
            "file:///a%2Fb",
            "file:///a%2fb",
            "file:///a%00b",
            "file:///a%zz",
            "file:///a%2",
            "file:///a%",
            "file://localhost?query",
            "file:///%FF",
            "no scheme",
        ] {
            assert_eq!(path(uri), None, "{uri:?} names no local path");
        }
    }
}
