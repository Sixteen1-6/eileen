//! An element's [`ElementHandle`] on Linux: its window's id and, as its `runtime_id`, its
//! object packed into words, so a handle names the same object in every process and every read,
//! for as long as its app keeps its connection, and Eileen keeps no table of handles.
//!
//! The words are a version, the lengths in bytes of the object's bus name and path, then the
//! bytes of the two, one after the other, four to a word (little-endian, the last word padded
//! with zero bytes). A handle comes back from Eileen's callers, so [`object_of`] checks every
//! part of it before it names an object.

use eileen_platform::{ElementHandle, PlatformError};
use zbus::zvariant::ObjectPath;

use crate::atspi::ObjectRef;

/// The version word of today's packing.
const VERSION: i32 = 1;
/// What a handle's errors name.
const HANDLE: &str = "element handle";

/// The handle of `object`, an element of the window `hwnd`.
pub(crate) fn handle_of(hwnd: i64, object: &ObjectRef) -> ElementHandle {
    let (bus, path) = (object.bus.as_bytes(), object.path.as_bytes());
    let length = |bytes: &[u8]| i32::try_from(bytes.len()).unwrap_or(i32::MAX);
    let mut runtime_id = vec![VERSION, length(bus), length(path)];
    let bytes: Vec<u8> = bus.iter().chain(path).copied().collect();
    runtime_id.extend(bytes.chunks(4).map(|chunk| {
        let mut word = [0_u8; 4];
        word.iter_mut().zip(chunk).for_each(|(to, from)| *to = *from);
        i32::from_le_bytes(word)
    }));
    ElementHandle { hwnd, runtime_id }
}

/// The object `handle` names. A handle that [`handle_of`] could not have made is
/// [`PlatformError::Invalid`]: another version, lengths that do not fit its words, padding that
/// is not zero, a name or path that is not UTF-8, a bus name that is not a unique name, or a
/// path that is not an object path or is the null reference's.
pub(crate) fn object_of(handle: &ElementHandle) -> Result<ObjectRef, PlatformError> {
    let invalid = |detail: &str| PlatformError::Invalid { what: HANDLE, detail: detail.to_owned() };
    let [version, bus_len, path_len, words @ ..] = handle.runtime_id.as_slice() else {
        return Err(invalid("too short"));
    };
    if *version != VERSION {
        return Err(invalid("an unknown version"));
    }
    let length = |word: i32| usize::try_from(word).map_err(|_| invalid("a negative length"));
    let (bus_len, path_len) = (length(*bus_len)?, length(*path_len)?);
    let total = bus_len.checked_add(path_len).ok_or_else(|| invalid("lengths too long"))?;
    if words.len() != total.div_ceil(4) {
        return Err(invalid("lengths that do not fit its words"));
    }
    let bytes: Vec<u8> = words.iter().flat_map(|word| word.to_le_bytes()).collect();
    let (named, padding) = bytes.split_at(total);
    if padding.iter().any(|&byte| byte != 0) {
        return Err(invalid("padding that is not zero"));
    }
    let (bus, path) = named.split_at(bus_len);
    let text = |bytes| std::str::from_utf8(bytes).map_err(|_| invalid("bytes that are not UTF-8"));
    let path = ObjectPath::try_from(text(path)?).map_err(|_| invalid("no object path"))?;
    ObjectRef::new(text(bus)?, &path).ok_or_else(|| invalid("no app's object"))
}

#[cfg(test)]
mod tests {
    use eileen_platform::{ElementHandle, PlatformError};
    use proptest::prelude::*;
    use zbus::names::OwnedUniqueName;
    use zbus::zvariant::OwnedObjectPath;

    use super::{ObjectRef, handle_of, object_of};

    /// The object at `path` of the app at `bus`.
    fn object(bus: &str, path: &str) -> ObjectRef {
        let bus = OwnedUniqueName::try_from(bus).unwrap();
        ObjectRef { bus, path: OwnedObjectPath::try_from(path).unwrap() }
    }

    #[test]
    fn a_handle_round_trips_its_object() {
        let button = object(":1.42", "/org/a11y/atspi/accessible/17");
        let handle = handle_of(7, &button);
        assert_eq!(handle.hwnd, 7, "its window");
        assert_eq!(&handle.runtime_id[..3], [1, 5, 29], "the version and the two lengths");
        assert_eq!(handle.runtime_id[3], i32::from_le_bytes(*b":1.4"), "four bytes to a word");
        assert_eq!(handle.runtime_id.len(), 3 + 34_usize.div_ceil(4), "the last word padded");
        assert_eq!(object_of(&handle), Ok(button.clone()), "the same object back");
        assert_eq!(handle_of(7, &button), handle, "the same handle on every read");
        let root = object(":1.1", "/");
        assert_eq!(object_of(&handle_of(1, &root)), Ok(root), "a path of one byte");
        let invalid = |runtime_id: Vec<i32>| object_of(&ElementHandle { hwnd: 7, runtime_id });
        let refused = |result: Result<ObjectRef, PlatformError>| {
            matches!(result, Err(PlatformError::Invalid { what: "element handle", .. }))
        };
        let mut longer = handle.runtime_id.clone();
        longer[1] += 1;
        assert!(refused(invalid(longer)), "a length its words do not hold");
        let mut padded = handle.runtime_id.clone();
        *padded.last_mut().unwrap() |= 0x7f00_0000;
        assert!(refused(invalid(padded)), "padding that is not zero");
        let mut version = handle.runtime_id;
        version[0] = 2;
        assert!(refused(invalid(version)), "another version");
        assert!(refused(invalid(vec![1, -1, 1, 0])), "a negative length");
        let named = handle_of(7, &object(":1.42", "/x"));
        let well_known =
            [&[1, 4, 2][..], &[i32::from_le_bytes(*b"org."), i32::from_le_bytes(*b"/x\0\0")]];
        assert!(refused(invalid(well_known.concat())), "a well-known name, not an app's");
        assert!(object_of(&named).is_ok(), "the same path at a unique name");
    }

    proptest! {
        #[test]
        fn any_runtime_id_decodes_or_is_invalid(
            runtime_id in prop::collection::vec(any::<i32>(), 0..24),
            prefix in prop::option::of((0_i32..12, 0_i32..40)),
        ) {
            let mut runtime_id = runtime_id;
            if let Some((bus, path)) = prefix {
                runtime_id.splice(0..0, [1, bus, path]);
            }
            let handle = ElementHandle { hwnd: 3, runtime_id };
            match object_of(&handle) {
                Ok(object) => prop_assert_eq!(handle_of(3, &object), handle),
                Err(error) => prop_assert!(
                    matches!(error, PlatformError::Invalid { what: "element handle", .. }),
                    "{:?}",
                    error
                ),
            }
        }
    }
}
