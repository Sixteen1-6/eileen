//! The XBEL walk over `recently-used.xbel`, as the Desktop Bookmark Specification 0.8.5 lays it
//! out (<https://www.freedesktop.org/wiki/Specifications/desktop-bookmark-spec/>), read as a
//! stream with quick-xml's `NsReader`, so memory follows the number of bookmarks and the
//! longest event. quick-xml holds an event whole, a run of text included, so the caller bounds
//! how much of the file is read.
//!
//! - Every `bookmark` child of the `xbel` root is one entry. Anything else is skipped with all
//!   it holds, including the `folder`, `alias` and `separator` elements the specification says
//!   to ignore.
//! - Metadata counts only inside a bookmark's `info`, in the `metadata` whose owner is
//!   `http://freedesktop.org`: the shared-mime-info namespace's `mime-type`, and the
//!   desktop-bookmarks namespace's `applications` and `private`. They are matched by namespace,
//!   not by prefix, as the specification says other owners' metadata is ignored.
//! - **When** is the newest of the bookmark's `modified` and `visited` and its applications'
//!   `modified`; its `added` when none of those is there.
//! - **The app** is the `name` of the application with the newest `modified`; among equals,
//!   the first listed. An application without a name is ignored, as `GLib` requires one.
//! - **Skipped:** a bookmark marked `private`, which the specification shows only to the
//!   applications that registered it; one that names no local file; one whose time does not
//!   parse, since one entry should not hide the rest (`GLib` refuses the whole file over such a
//!   time, unless it is an ordinal or week date); and one with no time at all, which `GLib`
//!   would stamp as now.
//! - A file with no root element, such as an empty one, holds no bookmarks. One that is not
//!   well-formed UTF-8 XML, ends inside an element, has a second root, or whose root is not
//!   `xbel` is [`FileError::Invalid`]. Only XML's predefined entities and character references
//!   are expanded, never a DTD's, so an entity XML does not define makes the file invalid.

use std::io::BufRead;

use eileen_contracts::ids::{Personal, Untrusted, WallTime};
use quick_xml::events::{BytesStart, Event};
use quick_xml::name::{Namespace, ResolveResult};
use quick_xml::{NsReader, XmlVersion};

use super::kind::kind_of;
use super::time::parse_time;
use super::uri::local_path;
use super::{FILE_NAME, RecentItem};
use crate::FileError;

/// The desktop-bookmarks namespace, of `applications`, `application` and `private`.
const BOOKMARK_NS: &str = "http://www.freedesktop.org/standards/desktop-bookmarks";

/// The shared-mime-info namespace, of `mime-type`.
const MIME_NS: &str = "http://www.freedesktop.org/standards/shared-mime-info";

/// The owner of the metadata the specification defines.
const OWNER: &str = "http://freedesktop.org";

/// The problem for a file that is not well-formed XML.
const NOT_XML: &str = "not well-formed XML";

/// The problem for a file whose root is not `xbel`.
const NOT_XBEL: &str = "not an XBEL bookmark file";

/// Where an element sits, as far as the reader cares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Place {
    /// The `xbel` root.
    Root,
    /// A `bookmark` under the root.
    Bookmark,
    /// A bookmark's `info`.
    Info,
    /// The `metadata` that `http://freedesktop.org` owns.
    Metadata,
    /// Its `applications` list.
    Applications,
    /// Anything else, with everything inside it.
    Other,
}

/// The namespace of an element in the metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ns {
    /// The desktop-bookmarks namespace.
    Bookmark,
    /// The shared-mime-info namespace.
    Mime,
    /// Another namespace, or none.
    Other,
}

impl Ns {
    /// The namespace an element's name resolved to.
    fn of(resolved: &ResolveResult<'_>) -> Self {
        match resolved {
            ResolveResult::Bound(Namespace(uri)) if *uri == BOOKMARK_NS => Self::Bookmark,
            ResolveResult::Bound(Namespace(uri)) if *uri == MIME_NS => Self::Mime,
            ResolveResult::Bound(_) | ResolveResult::Unbound | ResolveResult::Unknown(_) => {
                Self::Other
            }
        }
    }
}

/// One bookmark as the reader has seen it so far. It has no `Debug`, since it holds a path.
#[derive(Default)]
struct Draft {
    /// The local path its `href` names; `None` for anything else.
    path: Option<String>,
    /// Its `added` time.
    added: Option<WallTime>,
    /// The newest of its `modified` and `visited` and its applications' `modified`.
    newest: Option<WallTime>,
    /// The name of the application with the newest `modified`.
    app: Option<String>,
    /// That application's `modified`.
    app_modified: Option<WallTime>,
    /// The MIME type.
    mime: Option<String>,
    /// Whether it is marked `private`.
    private: bool,
    /// Whether one of its times does not parse.
    bad_time: bool,
}

impl Draft {
    /// A bookmark from the attributes of its `bookmark` element.
    fn open(element: &BytesStart<'_>) -> Result<Self, quick_xml::Error> {
        let [href, added, modified, visited] =
            attributes(element, ["href", "added", "modified", "visited"])?;
        let mut draft = Self { path: href.as_deref().and_then(local_path), ..Self::default() };
        draft.added = draft.time(added.as_deref());
        let modified = draft.time(modified.as_deref());
        let visited = draft.time(visited.as_deref());
        draft.newest = modified.max(visited);
        Ok(draft)
    }

    /// Reads a `bookmark:application` element. One without a name is ignored.
    fn application(&mut self, element: &BytesStart<'_>) -> Result<(), quick_xml::Error> {
        let [name, modified] = attributes(element, ["name", "modified"])?;
        let Some(name) = name.filter(|name| !name.is_empty()) else {
            return Ok(());
        };
        let modified = self.time(modified.as_deref());
        self.newest = self.newest.max(modified);
        if self.app.is_none() || modified > self.app_modified {
            self.app = Some(name);
            self.app_modified = modified;
        }
        Ok(())
    }

    /// `text` as a time; one that does not parse makes the bookmark unusable.
    fn time(&mut self, text: Option<&str>) -> Option<WallTime> {
        let time = parse_time(text?);
        self.bad_time |= time.is_none();
        time
    }

    /// The recent item, or `None` for a bookmark that is not a local file, is private, or has a
    /// time that does not parse or no time at all.
    fn finish(self) -> Option<RecentItem> {
        if self.private || self.bad_time {
            return None;
        }
        Some(RecentItem {
            path: Personal::new(self.path?),
            app: self.app.map(Untrusted::new),
            kind: self.mime.as_deref().and_then(kind_of),
            opened_at: self.newest.or(self.added)?,
        })
    }
}

/// Every usable bookmark in a bookmark file, in the file's order.
pub(super) fn read_bookmarks(source: impl BufRead) -> Result<Vec<RecentItem>, FileError> {
    let mut reader = NsReader::from_reader(source);
    let mut buf = Vec::new();
    let mut open: Vec<Place> = Vec::new();
    let mut root_seen = false;
    let mut draft = Draft::default();
    let mut items = Vec::new();
    loop {
        buf.clear();
        let read = reader.read_resolved_event_into(&mut buf);
        let (ns, event) = match read.map(|(resolved, event)| (Ns::of(&resolved), event)) {
            Ok(read) => read,
            Err(error) => return Err(broken(&error, reader.error_position())),
        };
        let (element, empty) = match event {
            Event::Start(element) => (element, false),
            Event::Empty(element) => (element, true),
            Event::End(_) => {
                if open.pop() == Some(Place::Bookmark) {
                    items.extend(std::mem::take(&mut draft).finish());
                }
                continue;
            }
            Event::Eof => break,
            Event::Text(_)
            | Event::CData(_)
            | Event::Comment(_)
            | Event::Decl(_)
            | Event::PI(_)
            | Event::DocType(_)
            | Event::GeneralRef(_) => continue,
        };
        let place = match open.last() {
            Some(parent) => enter(*parent, &element, ns, &mut draft)
                .map_err(|error| broken(&error, reader.buffer_position()))?,
            None if !root_seen && element.name().as_ref() == "xbel" => Place::Root,
            None => return Err(invalid(NOT_XBEL, reader.buffer_position())),
        };
        root_seen = true;
        if !empty {
            open.push(place);
        } else if place == Place::Bookmark {
            items.extend(std::mem::take(&mut draft).finish());
        }
    }
    // quick-xml reports a file that ends inside an element as a clean end of file.
    if !open.is_empty() {
        return Err(invalid(NOT_XML, reader.buffer_position()));
    }
    Ok(items)
}

/// Where `element`, a child of `parent` in the namespace `ns`, sits, reading what it holds
/// into `draft`.
fn enter(
    parent: Place,
    element: &BytesStart<'_>,
    ns: Ns,
    draft: &mut Draft,
) -> Result<Place, quick_xml::Error> {
    let name = element.name();
    let local = element.local_name();
    let place = match (parent, ns, local.as_ref()) {
        (Place::Root, _, _) if name.as_ref() == "bookmark" => {
            *draft = Draft::open(element)?;
            Place::Bookmark
        }
        (Place::Bookmark, _, _) if name.as_ref() == "info" => Place::Info,
        (Place::Info, _, _) if name.as_ref() == "metadata" => {
            let [owner] = attributes(element, ["owner"])?;
            if owner.as_deref() == Some(OWNER) { Place::Metadata } else { Place::Other }
        }
        (Place::Metadata, Ns::Mime, "mime-type") => {
            let [mime] = attributes(element, ["type"])?;
            if mime.is_some() {
                draft.mime = mime;
            }
            Place::Other
        }
        (Place::Metadata, Ns::Bookmark, "applications") => Place::Applications,
        (Place::Metadata, Ns::Bookmark, "private") => {
            draft.private = true;
            Place::Other
        }
        (Place::Applications, Ns::Bookmark, "application") => {
            draft.application(element)?;
            Place::Other
        }
        _ => Place::Other,
    };
    Ok(place)
}

/// The values of `keys` among `element`'s unprefixed attributes, entity and character
/// references resolved; `None` for one it lacks.
fn attributes<const N: usize>(
    element: &BytesStart<'_>,
    keys: [&str; N],
) -> Result<[Option<String>; N], quick_xml::Error> {
    let mut values = [const { None }; N];
    for attribute in element.attributes() {
        let attribute = attribute?;
        let slot = keys
            .iter()
            .position(|key| attribute.key.as_ref() == *key)
            .and_then(|at| values.get_mut(at));
        if let Some(slot) = slot {
            *slot = Some(attribute.normalized_value(XmlVersion::Implicit1_0)?.into_owned());
        }
    }
    Ok(values)
}

/// What a failed read of the XML means, near byte `offset`. Only fixed descriptions are kept:
/// the parser's messages quote the file's text.
fn broken(error: &quick_xml::Error, offset: u64) -> FileError {
    let problem = match error {
        quick_xml::Error::Io(source) => return FileError::io(FILE_NAME, source),
        quick_xml::Error::Syntax(_)
        | quick_xml::Error::IllFormed(_)
        | quick_xml::Error::InvalidAttr(_) => NOT_XML,
        quick_xml::Error::Encoding(_) => "not UTF-8",
        quick_xml::Error::Escape(_) => "a bad entity or character reference",
        quick_xml::Error::Namespace(_) => "a bad namespace declaration",
    };
    invalid(problem, offset)
}

/// The file is not a bookmark file: `problem`, near byte `offset`.
fn invalid(problem: &str, offset: u64) -> FileError {
    FileError::Invalid { what: FILE_NAME, detail: format!("{problem} near byte {offset}") }
}

#[cfg(test)]
mod tests {
    use super::read_bookmarks;
    use crate::FileError;

    #[test]
    fn a_file_that_is_not_utf8_or_ends_early_is_broken() {
        let broken = |bytes: &[u8]| match read_bookmarks(bytes) {
            Err(FileError::Invalid { what: "recently-used.xbel", detail }) => {
                let (problem, offset) = detail.rsplit_once(" near byte ").unwrap();
                assert!(offset.parse::<u64>().is_ok(), "{detail:?} ends with an offset");
                problem.to_owned()
            }
            other => panic!("{bytes:?} read as {other:?}"),
        };
        let latin1 =
            b"<xbel version=\"1.0\"><bookmark href=\"file:///home/sam/r\xe9sum\xe9.pdf\"/></xbel>";
        assert_eq!(broken(latin1), "not UTF-8", "Latin-1 bytes");
        assert_eq!(
            broken(b"<xbel version=\"1.0\">\n  <bookmark href=\"file:///x\">"),
            "not well-formed XML",
            "cut off"
        );
        assert_eq!(
            broken(b"<xbel><bookmark href=\"file:///x&nbsp;\"/></xbel>"),
            "a bad entity or character reference",
            "an entity XML does not define"
        );
        assert_eq!(broken(b"<xbel/><xbel/>"), "not an XBEL bookmark file", "two roots");
    }
}
