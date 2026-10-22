# App fixtures

Every fixture here is synthetic: written by hand in the format its real counterpart has, with
only the keys and elements the code reads. None was captured from a machine, so nothing had to
be scrubbed. They hold no real user name, host name, machine id, serial number or MAC address:
the user is the placeholder `sam` (`/home/sam`), remote hosts are `nas.local` and
`example.org`, and app names and versions (`LibreOffice 24.2`) are illustrative. A fixture
captured later must be scrubbed the same way, and this table must say so and what was replaced.

## `session/`: desktop entries for the app index

The folders are a home folder's `.local/share`, the system's `/usr/share` and the export folders
of Flatpak and Snap, as `tests/it/index.rs` points `XDG_DATA_HOME` and `XDG_DATA_DIRS` at them.

| Fixture | Made | What it is |
|---|---|---|
| `home/sam/.local/share/applications/firefox.desktop` | Synthetic | The user's own copy, which replaces the system's |
| `home/sam/.local/share/applications/org.gnome.Calculator.desktop` | Synthetic | The user's `Hidden=true` copy, which removes the app |
| `home/sam/.local/share/applications/backup-photos.desktop` | Synthetic | A user's own terminal app, with an escaped space in its name |
| `usr/share/applications/firefox.desktop` | Synthetic | Firefox's entry as Mozilla ships it: localized names and keywords, an action |
| `usr/share/applications/org.gnome.Calculator.desktop` | Synthetic | A D-Bus-activatable app with a German name |
| `usr/share/applications/org.gnome.TextEditor.desktop` | Synthetic | A D-Bus-activatable app with localized keywords and an escaped `;` |
| `usr/share/applications/org.kde.konsole.desktop` | Synthetic | `OnlyShowIn=KDE` |
| `usr/share/applications/gnome-only-elsewhere.desktop` | Synthetic | `NotShowIn=GNOME` |
| `usr/share/applications/nm-applet.desktop` | Synthetic | `NoDisplay=true` |
| `usr/share/applications/needs-tool.desktop` | Synthetic | `TryExec` on a tool the test puts on `PATH` |
| `usr/share/applications/htop.desktop` | Synthetic | A terminal app |
| `usr/share/applications/kde4/kate.desktop` | Synthetic | A subfolder's desktop file ID, `kde4-kate.desktop` |
| `usr/share/applications/ubuntu-help.desktop` | Synthetic | A `Link` entry, which is not an app |
| `usr/share/applications/no-exec.desktop` | Synthetic | An application with no `Exec` |
| `usr/share/applications/broken.desktop` | Synthetic | A file that is not a key file |
| `usr/share/applications/mimeinfo.cache` | Synthetic | A file that is not a desktop entry, which the index passes over |
| `var/lib/flatpak/exports/share/applications/org.videolan.VLC.desktop` | Synthetic | Flatpak's exported entry, with its `flatpak run` line |
| `var/lib/snapd/desktop/applications/spotify_spotify.desktop` | Synthetic | Snap's exported entry |

## `recent/`: bookmark files for the recent files

`tests/it/recent.rs` reads each file through `RecentlyUsed::at`.

| Fixture | Made | What it is |
|---|---|---|
| `home/sam/.local/share/recently-used.xbel` | Synthetic | GLib's layout, as GTK apps write it: microsecond times, `&amp;` and `&apos;`, escaped UTF-8, a folder, two apps on one image, an entry KIO registered, `sftp:` and `smb:` entries and a private one |
| `xdg-data/recently-used.xbel` | Synthetic | KIO's layout, as KDE apps write it: one line of namespaces, millisecond times |
| `odd.xbel` | Synthetic | What GTK and KIO never write: times in other zones, ties, a path listed three times, other owners' metadata, folders, nameless apps and unusable entries |
| `namespaces.xbel` | Synthetic | The metadata under other prefixes and a default namespace, and the usual prefixes bound to other namespaces |
| `cleared.xbel` | Synthetic | GLib's file once the list is cleared: a root and no bookmarks |
| `empty.xbel` | Synthetic | An empty file, as people switch the list off |
| `truncated.xbel` | Synthetic | A file cut off inside an attribute |
| `mismatched.xbel` | Synthetic | An end tag that does not match |
| `not-xbel.xbel` | Synthetic | Well-formed XML whose root is not `xbel` |
