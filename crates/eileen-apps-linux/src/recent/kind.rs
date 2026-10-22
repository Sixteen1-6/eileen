//! What kind of file a bookmark names, from its MIME type, which is compared without case or
//! parameters (RFC 2045 section 5.1) and named as the shared MIME database names it
//! (shared-mime-info 2.4, `freedesktop.org.xml`):
//!
//! - ODF and Microsoft Office documents, spreadsheets and presentations, by family, so
//!   templates and macro-enabled variants count too;
//! - PDF, RTF, plain text and Markdown as documents, and CSV as a spreadsheet;
//! - `image/*`, `video/*` and `audio/*`;
//! - code, by the database's types for programming and scripting languages (shader,
//!   hardware-description, interface-definition and user-interface languages among them),
//!   stylesheets, build files and patches;
//! - a folder, `inode/directory`.
//!
//! Anything else has no kind. [`FileKind::Media`] is never set: it belongs to the media history.

use eileen_contracts::tools::apps::FileKind;

/// MIME type families by prefix, so templates and macro-enabled variants count too.
const OFFICE: [(&str, FileKind); 10] = [
    ("application/vnd.oasis.opendocument.text", FileKind::Document),
    ("application/vnd.oasis.opendocument.spreadsheet", FileKind::Spreadsheet),
    ("application/vnd.oasis.opendocument.presentation", FileKind::Presentation),
    ("application/vnd.openxmlformats-officedocument.wordprocessingml.", FileKind::Document),
    ("application/vnd.openxmlformats-officedocument.spreadsheetml.", FileKind::Spreadsheet),
    ("application/vnd.openxmlformats-officedocument.presentationml.", FileKind::Presentation),
    ("application/msword", FileKind::Document),
    ("application/vnd.ms-word.", FileKind::Document),
    ("application/vnd.ms-excel", FileKind::Spreadsheet),
    ("application/vnd.ms-powerpoint", FileKind::Presentation),
];

/// Other MIME types with a kind, with the aliases the shared MIME database lists for them.
const NAMED: [(&str, FileKind); 11] = [
    ("application/pdf", FileKind::Document),
    ("application/rtf", FileKind::Document),
    ("text/rtf", FileKind::Document),
    ("text/plain", FileKind::Document),
    ("text/markdown", FileKind::Document),
    ("text/x-markdown", FileKind::Document),
    ("text/csv", FileKind::Spreadsheet),
    ("text/x-csv", FileKind::Spreadsheet),
    ("text/x-comma-separated-values", FileKind::Spreadsheet),
    ("inode/directory", FileKind::Folder),
    ("x-directory/normal", FileKind::Folder),
];

/// Code: the shared MIME database's types (shared-mime-info 2.4) for the languages,
/// stylesheets, build files and patches the module doc names, each with every alias the
/// database lists for it.
const CODE: [&str; 115] = [
    "text/x-csrc",
    "text/x-c",
    "text/x-chdr",
    "text/x-c++src",
    "text/x-c++hdr",
    "text/x-moc",
    "text/x-objcsrc",
    "text/x-objc++src",
    "text/x-ooc",
    "text/x-csharp",
    "text/x-dsrc",
    "text/x-java",
    "text/x-kotlin",
    "text/x-scala",
    "text/x-groovy",
    "text/x-gradle",
    "text/rust",
    "text/x-go",
    "text/x-adasrc",
    "text/x-cobol",
    "text/x-eiffel",
    "text/x-basic",
    "text/x-vb",
    "text/x-python",
    "text/x-python3",
    "text/x-sagemath",
    "text/x-lua",
    "text/x-haskell",
    "text/x-literate-haskell",
    "text/x-ocaml",
    "text/x-elixir",
    "text/x-erlang",
    "text/x-fortran",
    "text/x-pascal",
    "text/x-vala",
    "text/x-genie",
    "text/x-nim",
    "text/x-nimscript",
    "text/x-crystal",
    "text/crystal",
    "text/julia",
    "text/x-common-lisp",
    "text/x-emacs-lisp",
    "text/x-scheme",
    "text/x-matlab",
    "text/x-octave",
    "text/x-modelica",
    "text/tcl",
    "text/x-tcl",
    "text/vnd.senx.warpscript",
    "text/x-verilog",
    "text/x-svsrc",
    "text/x-svhdr",
    "text/x-vhdl",
    "text/x-devicetree-source",
    "text/x-opencl-src",
    "application/x-godot-shader",
    "application/x-gdscript",
    "text/x-qml",
    "text/x-blueprint",
    "text/x-uil",
    "text/x-idl",
    "text/x-mof",
    "text/x-ocl",
    "text/javascript",
    "application/javascript",
    "application/x-javascript",
    "text/jscript",
    "text/jscript.encode",
    "application/ecmascript",
    "text/ecmascript",
    "application/vnd.coffeescript",
    "text/vbscript",
    "text/vbs",
    "text/vbscript.encode",
    "text/vnd.wap.wmlscript",
    "application/vnd.dart",
    "text/x-dart",
    "application/x-perl",
    "text/x-perl",
    "application/x-ruby",
    "application/x-markaby",
    "application/x-php",
    "application/x-asp",
    "application/x-shellscript",
    "text/x-sh",
    "application/x-csh",
    "application/x-fishscript",
    "text/x-fish",
    "application/x-nuscript",
    "text/x-nu",
    "application/x-powershell",
    "application/x-bat",
    "application/bat",
    "text/x-dcl",
    "application/x-awk",
    "application/x-m4",
    "application/x-gnuplot",
    "application/sieve",
    "text/css",
    "text/x-scss",
    "text/x-sass",
    "application/xslt+xml",
    "text/x-makefile",
    "text/x-cmake",
    "text/x-meson",
    "text/x-scons",
    "text/x-maven+xml",
    "text/x-rpm-spec",
    "application/sql",
    "text/x-sql",
    "text/x-patch",
    "text/x-diff",
    "text/x-reject",
    "application/x-reject",
];

/// What kind of file a MIME type names, compared without case or parameters; the module doc
/// lists the kinds. `None` for any other type.
pub(super) fn kind_of(mime: &str) -> Option<FileKind> {
    let mime =
        mime.split_once(';').map_or(mime, |(essence, _)| essence).trim().to_ascii_lowercase();
    let (family, _) = mime.split_once('/')?;
    match family {
        "image" => return Some(FileKind::Image),
        "video" => return Some(FileKind::Video),
        "audio" => return Some(FileKind::Audio),
        _ => {}
    }
    OFFICE
        .iter()
        .find(|(prefix, _)| mime.starts_with(prefix))
        .or_else(|| NAMED.iter().find(|(name, _)| *name == mime))
        .map(|(_, kind)| *kind)
        .or_else(|| CODE.contains(&mime.as_str()).then_some(FileKind::Code))
}

#[cfg(test)]
mod tests {
    use eileen_contracts::tools::apps::FileKind;

    use super::kind_of;

    #[test]
    fn mime_types_name_their_kinds() {
        let cases = [
            ("application/vnd.oasis.opendocument.text", Some(FileKind::Document)),
            ("application/vnd.oasis.opendocument.text-template", Some(FileKind::Document)),
            (
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
                Some(FileKind::Document),
            ),
            ("application/msword", Some(FileKind::Document)),
            ("application/vnd.ms-word.document.macroEnabled.12", Some(FileKind::Document)),
            ("application/pdf", Some(FileKind::Document)),
            ("text/rtf", Some(FileKind::Document)),
            ("text/plain; charset=UTF-8", Some(FileKind::Document)),
            ("text/markdown", Some(FileKind::Document)),
            ("application/vnd.oasis.opendocument.spreadsheet", Some(FileKind::Spreadsheet)),
            (
                "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
                Some(FileKind::Spreadsheet),
            ),
            ("application/vnd.ms-excel", Some(FileKind::Spreadsheet)),
            ("application/vnd.ms-excel.sheet.macroEnabled.12", Some(FileKind::Spreadsheet)),
            ("text/csv", Some(FileKind::Spreadsheet)),
            ("application/vnd.oasis.opendocument.presentation", Some(FileKind::Presentation)),
            (
                "application/vnd.openxmlformats-officedocument.presentationml.presentation",
                Some(FileKind::Presentation),
            ),
            ("application/vnd.ms-powerpoint", Some(FileKind::Presentation)),
            ("image/jpeg", Some(FileKind::Image)),
            ("IMAGE/SVG+XML", Some(FileKind::Image)),
            ("video/mp4", Some(FileKind::Video)),
            ("audio/flac", Some(FileKind::Audio)),
            ("text/rust", Some(FileKind::Code)),
            ("text/x-c++src", Some(FileKind::Code)),
            ("application/x-shellscript", Some(FileKind::Code)),
            ("application/javascript", Some(FileKind::Code)),
            ("inode/directory", Some(FileKind::Folder)),
            ("application/zip", None),
            ("application/vnd.oasis.opendocument.graphics", None),
            ("text/html", None),
            ("application/octet-stream", None),
            ("nonsense", None),
            ("", None),
        ];
        for (mime, kind) in cases {
            assert_eq!(kind_of(mime), kind, "{mime:?}");
        }
    }

    #[test]
    fn the_mime_databases_code_types_and_their_aliases_are_code() {
        for mime in [
            "text/x-adasrc",
            "text/x-cobol",
            "application/x-awk",
            "application/x-csh",
            "text/x-vb",
            "text/x-basic",
            "application/ecmascript",
            "text/ecmascript",
            "application/vnd.coffeescript",
            "text/x-literate-haskell",
            "text/x-eiffel",
            "text/x-genie",
            "text/x-nimscript",
            "application/x-gdscript",
            "text/x-devicetree-source",
            "text/x-svhdr",
            "text/vbscript",
            "text/vbs",
            "text/x-c",
            "text/x-dart",
            "text/x-fish",
            "text/jscript",
            "text/crystal",
            "text/x-octave",
            "text/x-ooc",
            "application/x-nuscript",
            "application/x-godot-shader",
            "text/x-blueprint",
            "text/x-idl",
            "application/xslt+xml",
            "text/x-scons",
            "text/x-reject",
        ] {
            assert_eq!(kind_of(mime), Some(FileKind::Code), "{mime}");
        }
        for (mime, why) in [
            ("application/x-wais-source", "describes a search server, though called source code"),
            ("application/json5", "is data, though a kind of JavaScript"),
            ("application/x-shared-library-la", "describes a library"),
            ("text/x.gcode", "drives a machine"),
        ] {
            assert_eq!(kind_of(mime), None, "{mime} {why}");
        }
    }
}
