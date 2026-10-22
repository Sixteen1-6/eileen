//! Builds a fixture machine's tree from its `.tree` file (format in `tests/fixtures/README.md`).

use std::fs;
use std::path::{Path, PathBuf};

/// The tree of fixture `name`, built afresh in a folder of its own for the test `test`, so
/// tests that run at once never share a tree.
pub(crate) fn build(name: &str, test: &str) -> PathBuf {
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(format!("{name}.tree"));
    let description = fs::read_to_string(&source).unwrap();
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join("machines").join(test);
    if root.exists() {
        fs::remove_dir_all(&root).unwrap();
    }
    fs::create_dir_all(&root).unwrap();
    for (path, content) in files(&description) {
        let file = root.join(&path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, content).unwrap();
    }
    root
}

/// The files a `.tree` text describes, as (path, content).
pub(crate) fn files(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(path) = line.strip_prefix("<<< ") {
            let mut content = String::new();
            for body in lines.by_ref() {
                if body == ">>>" {
                    break;
                }
                content.push_str(body);
                content.push('\n');
            }
            out.push((path.trim().to_owned(), content));
        } else if let Some(path) = line.strip_suffix(" =") {
            out.extend(expand(path, None));
        } else {
            let (path, value) =
                line.split_once(" = ").unwrap_or_else(|| panic!("not a fixture line: {line}"));
            out.extend(expand(path, Some(value)));
        }
    }
    out
}

/// One file per number of the path's `{list}`, with `$n` in the value replaced.
fn expand(path: &str, value: Option<&str>) -> Vec<(String, String)> {
    let content = |n: Option<u32>| {
        value.map_or_else(String::new, |value| {
            let value = value.replace("\\0", "\0");
            let value = n.map_or_else(|| value.clone(), |n| value.replace("$n", &n.to_string()));
            format!("{value}\n")
        })
    };
    let Some((before, rest)) = path.split_once('{') else {
        return vec![(path.to_owned(), content(None))];
    };
    let (list, after) = rest.split_once('}').unwrap();
    numbers(list).into_iter().map(|n| (format!("{before}{n}{after}"), content(Some(n)))).collect()
}

/// The numbers of a CPU-list, such as `0-3,8`.
fn numbers(list: &str) -> Vec<u32> {
    list.split(',')
        .flat_map(|part| match part.split_once('-') {
            Some((low, high)) => (low.parse().unwrap()..=high.parse().unwrap()).collect(),
            None => vec![part.parse().unwrap()],
        })
        .collect()
}

#[test]
fn the_format_writes_one_line_files_ranges_and_blocks() {
    let text = "# a comment\n\nsys/block/loop{0-1}/dev = 7:$n\ndev/dxg =\n\
                a/compatible = x\\0y\\0\n<<< proc/meminfo\nMemTotal: 1 kB\n\nCommitLimit: 2 kB\n>>>\n";
    let files = files(text);
    let expected = [
        ("sys/block/loop0/dev", "7:0\n"),
        ("sys/block/loop1/dev", "7:1\n"),
        ("dev/dxg", ""),
        ("a/compatible", "x\0y\0\n"),
        ("proc/meminfo", "MemTotal: 1 kB\n\nCommitLimit: 2 kB\n"),
    ];
    let expected: Vec<(String, String)> = expected
        .iter()
        .map(|(path, content)| ((*path).to_owned(), (*content).to_owned()))
        .collect();
    assert_eq!(files, expected, "every form");
}
