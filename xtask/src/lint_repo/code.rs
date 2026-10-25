//! Checks 10, 14 and 16 over the Rust sources, read through the [`lexer`](super::lexer) so
//! comments and string literals never count as code.
//!
//! - Check 10: the `unsafe` keyword only in a module named `ffi` (`ffi.rs`, or a file under
//!   `ffi/`), and the workspace lints that hold the rest (`unsafe_code`,
//!   `undocumented_unsafe_blocks` and `multiple_unsafe_ops_per_block` denied); clippy enforces
//!   the safety comments and one operation per block.
//! - Check 14: no source names a vendor credential path or the persistent MCP registration
//!   command; `Command::new` and `#[expect(clippy::disallowed_methods)]` only at the sites
//!   `xtask/tidy/disallowed-methods.toml` lists (section 4b.2's exemptions).
//! - Check 16: no `#[ignore]` or `#[should_panic]`; no real-time sleep in test code (a tokio
//!   sleep counts unless the file runs on paused time); a module over 800 lines only with a
//!   reason in `xtask/tidy/allow.toml`. Whether a test needs the owner's PC or real Claude is a
//!   reviewer's call: no scan can tell, so the `owner_pc::` and `real_claude::` rule is not here.

use super::lexer::{find_word, is_ident, lex};
use super::tidy::{Tidy, is_listed};
use super::{Check, Finding, Tree, TreeError, line_of, under};

/// The longest module allowed without a reason.
const MAX_MODULE_LINES: usize = 800;

/// Runs checks 10, 14 and 16.
pub(crate) fn check(
    tree: &dyn Tree,
    tidy: &Tidy,
    findings: &mut Vec<Finding>,
) -> Result<(), TreeError> {
    check_unsafe_lints(tree, findings)?;
    for path in tree.files() {
        let extension = path.rsplit_once('.').map_or("", |(_, ext)| ext);
        let scan_credentials = tidy.source_extensions.iter().any(|e| e == extension)
            && !is_listed(&tidy.credential_exempt, path);
        if !scan_credentials && extension != "rs" {
            continue;
        }
        let Some(text) = tree.read(path)? else { continue };
        if scan_credentials {
            check_credentials(path, &text, tidy, findings);
        }
        if extension == "rs" {
            let code = lex(&text).code;
            check_unsafe(path, &code, findings);
            check_spawns(path, &code, tidy, findings);
            check_tests(path, &code, findings);
            let lines = text.lines().count();
            if lines > MAX_MODULE_LINES && !is_listed(&tidy.long_modules, path) {
                findings.push(Finding::new(
                    Check::Tests,
                    path,
                    Some(MAX_MODULE_LINES + 1),
                    format!("the module has {lines} lines; over {MAX_MODULE_LINES} needs a reason"),
                    "split the module, or list it in xtask/tidy/allow.toml with the reason",
                ));
            }
        }
    }
    Ok(())
}

/// Check 10's configuration half: the workspace lints that deny unsafe code and enforce the
/// safety comments.
fn check_unsafe_lints(tree: &dyn Tree, findings: &mut Vec<Finding>) -> Result<(), TreeError> {
    let Some(text) = tree.read("Cargo.toml")? else { return Ok(()) };
    let manifest =
        text.parse::<toml::Table>().map_err(|error| TreeError(format!("Cargo.toml: {error}")))?;
    let lints = manifest.get("workspace").and_then(|w| w.get("lints"));
    for (group, lint) in [
        ("rust", "unsafe_code"),
        ("clippy", "undocumented_unsafe_blocks"),
        ("clippy", "multiple_unsafe_ops_per_block"),
    ] {
        let level = lints.and_then(|l| l.get(group)).and_then(|g| g.get(lint));
        let level = level.and_then(|v| v.as_str().or_else(|| v.get("level")?.as_str()));
        if level != Some("deny") {
            findings.push(Finding::new(
                Check::Unsafe,
                "Cargo.toml",
                None,
                format!("[workspace.lints.{group}] does not deny `{lint}`"),
                format!("set `{lint} = \"deny\"` (BUILD.md section 4b.2)"),
            ));
        }
    }
    Ok(())
}

/// Whether `path` is an `ffi` module: a file named `ffi.rs` or one under a folder named `ffi`.
fn is_ffi(path: &str) -> bool {
    path.split('/').any(|part| part == "ffi") || path.ends_with("/ffi.rs") || path == "ffi.rs"
}

fn check_unsafe(path: &str, code: &str, findings: &mut Vec<Finding>) {
    if is_ffi(path) {
        return;
    }
    for at in find_word(code, "unsafe") {
        findings.push(Finding::new(
            Check::Unsafe,
            path,
            Some(line_of(code, at)),
            "unsafe code outside an `ffi` module",
            "move it into the crate's private `ffi` module, declared with \
             `#[expect(unsafe_code, reason = \"...\")] mod ffi;`, behind a safe function",
        ));
    }
}

fn check_credentials(path: &str, text: &str, tidy: &Tidy, findings: &mut Vec<Finding>) {
    let patterns: Vec<(&String, String)> = tidy
        .credential_paths
        .iter()
        .chain(&tidy.credential_commands)
        .map(|pattern| (pattern, pattern.replace('\\', "/").to_lowercase()))
        .collect();
    for (index, line) in text.lines().enumerate() {
        // Paths are matched with either separator, an escaped backslash included, and any case.
        let normalised = line.replace("\\\\", "/").replace('\\', "/").to_lowercase();
        for (pattern, needle) in &patterns {
            if normalised.contains(needle.as_str()) {
                findings.push(Finding::new(
                    Check::CredentialBoundary,
                    path,
                    Some(index + 1),
                    format!("names `{pattern}`, which the credential boundary forbids"),
                    "Eileen never reads vendor credentials or registers MCP servers persistently \
                     (BUILD.md section 2.12); remove the reference",
                ));
            }
        }
    }
}

/// A function's name and the byte range of its body.
#[derive(Debug, Clone, PartialEq, Eq)]
struct FnSpan {
    name: String,
    start: usize,
    end: usize,
}

/// Every function with a body in `code`, from `fn <name>` to its closing brace.
fn fn_spans(code: &str) -> Vec<FnSpan> {
    let mut spans = Vec::new();
    for at in find_word(code, "fn") {
        let rest = code.get(at + 2..).unwrap_or_default();
        let name: String = rest.trim_start().chars().take_while(|c| is_ident(*c)).collect();
        if name.is_empty() {
            continue;
        }
        let Some(open) = rest.find(['{', ';']) else { continue };
        if rest.get(open..open + 1) != Some("{") {
            continue;
        }
        let start = at + 2 + open;
        spans.push(FnSpan { name, start, end: matching_brace(code, start) });
    }
    spans
}

/// The offset just past the brace that closes the one at `open`, or the end of the code.
fn matching_brace(code: &str, open: usize) -> usize {
    let mut depth = 0_usize;
    for (at, c) in code.char_indices().skip_while(|(at, _)| *at < open) {
        match c {
            '{' => depth += 1,
            '}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return at + 1;
                }
            }
            _ => {}
        }
    }
    code.len()
}

/// The innermost function whose body holds `offset`.
fn enclosing(spans: &[FnSpan], offset: usize) -> Option<&str> {
    spans
        .iter()
        .filter(|span| span.start <= offset && offset < span.end)
        .max_by_key(|span| span.start)
        .map(|span| span.name.as_str())
}

/// The function an attribute ending at `end` applies to: the next item when it is a function,
/// otherwise the function around the attribute (it sits on a statement there).
fn attribute_target<'a>(code: &str, end: usize, spans: &'a [FnSpan]) -> Option<&'a str> {
    let mut rest = code.get(end..).unwrap_or_default().trim_start();
    while rest.starts_with("#[") {
        let close = rest.find(']').map_or(rest.len(), |at| at + 1);
        rest = rest.get(close..).unwrap_or_default().trim_start();
    }
    loop {
        let word: String = rest.chars().take_while(|c| is_ident(*c)).collect();
        rest = rest.get(word.len()..).unwrap_or_default().trim_start();
        match word.as_str() {
            "pub" if rest.starts_with('(') => {
                let close = rest.find(')').map_or(rest.len(), |at| at + 1);
                rest = rest.get(close..).unwrap_or_default().trim_start();
            }
            "pub" | "const" | "async" | "unsafe" | "default" => {}
            "extern" => {
                if rest.starts_with('"') {
                    let close =
                        rest.get(1..).and_then(|r| r.find('"')).map_or(rest.len(), |at| at + 2);
                    rest = rest.get(close..).unwrap_or_default().trim_start();
                }
            }
            "fn" => {
                let name: String = rest.chars().take_while(|c| is_ident(*c)).collect();
                return spans
                    .iter()
                    .find(|s| s.name == name && s.start >= end)
                    .map(|s| s.name.as_str());
            }
            _ => return enclosing(spans, end),
        }
    }
}

fn check_spawns(path: &str, code: &str, tidy: &Tidy, findings: &mut Vec<Finding>) {
    if is_listed(&tidy.spawn_exempt, path) {
        return;
    }
    let spans = fn_spans(code);
    let sanctioned = |function: Option<&str>, spawn: bool| {
        tidy.sites.iter().any(|site| {
            under(path, &site.path)
                && Some(site.function.as_str()) == function
                && (site.spawns || !spawn)
        })
    };
    for at in find_word(code, "Command::new") {
        if !sanctioned(enclosing(&spans, at), true) {
            findings.push(Finding::new(
                Check::CredentialBoundary,
                path,
                Some(line_of(code, at)),
                "a process spawn outside the supervisor",
                "start children through `eileen_supervisor`; only the sites in \
                 xtask/tidy/disallowed-methods.toml with `spawns = true` may call `Command::new`",
            ));
        }
    }
    let mut from = 0;
    while let Some(at) =
        code.get(from..).and_then(|rest| rest.find("#[expect(")).map(|at| from + at)
    {
        let close = matching_paren(code, at + "#[expect".len());
        from = close;
        let attribute = code.get(at..close).unwrap_or_default();
        if !attribute.contains("clippy::disallowed_methods") {
            continue;
        }
        let target = attribute_target(code, close, &spans);
        if !sanctioned(target, false) {
            findings.push(Finding::new(
                Check::CredentialBoundary,
                path,
                Some(line_of(code, at)),
                format!(
                    "an `#[expect(clippy::disallowed_methods)]` on `{}` that section 4b.2 does not sanction",
                    target.unwrap_or("an item outside any function")
                ),
                "use the sanctioned helper instead (eileen_stdx, eileen_rt, eileen_supervisor); a \
                 new exception needs a decision record and a line in xtask/tidy/disallowed-methods.toml",
            ));
        }
    }
}

/// The offset just past the `)` that closes the `(` at `open`, and the `]` after it.
fn matching_paren(code: &str, open: usize) -> usize {
    let mut depth = 0_usize;
    for (at, c) in code.char_indices().skip_while(|(at, _)| *at < open) {
        match c {
            '(' => depth += 1,
            ')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    let after = code.get(at + 1..).unwrap_or_default();
                    return at + 1 + usize::from(after.starts_with(']'));
                }
            }
            _ => {}
        }
    }
    code.len()
}

/// Whether the whole file is test code: an integration test or a test module's own file.
fn is_test_file(path: &str) -> bool {
    path.starts_with("tests/") || path.contains("/tests/") || path.ends_with("/tests.rs")
}

/// The byte ranges of `#[cfg(test)] mod name { ... }` blocks.
fn test_blocks(code: &str) -> Vec<(usize, usize)> {
    let mut blocks = Vec::new();
    for at in find_word(code, "cfg(test)") {
        let rest = code.get(at..).unwrap_or_default();
        let Some(close) = rest.find(']') else { continue };
        let mut item = rest.get(close + 1..).unwrap_or_default().trim_start();
        while item.starts_with("#[") {
            let end = item.find(']').map_or(item.len(), |end| end + 1);
            item = item.get(end..).unwrap_or_default().trim_start();
        }
        for visibility in ["pub(crate)", "pub(super)", "pub"] {
            item = item.strip_prefix(visibility).unwrap_or(item).trim_start();
        }
        let Some(module) = item.strip_prefix("mod").filter(|m| m.starts_with(char::is_whitespace))
        else {
            continue;
        };
        let Some(open) = module.find(['{', ';']) else { continue };
        if module.get(open..open + 1) == Some("{") {
            let start = code.len() - module.len() + open;
            blocks.push((start, matching_brace(code, start)));
        }
    }
    blocks
}

fn check_tests(path: &str, code: &str, findings: &mut Vec<Finding>) {
    let mut from = 0;
    while let Some(at) = code.get(from..).and_then(|rest| rest.find("#[")).map(|at| from + at) {
        from = at + 2;
        let word: String = code
            .get(from..)
            .unwrap_or_default()
            .trim_start()
            .chars()
            .take_while(|c| is_ident(*c))
            .collect();
        if word == "ignore" || word == "should_panic" {
            findings.push(Finding::new(
                Check::Tests,
                path,
                Some(line_of(code, at)),
                format!("a `#[{word}]` test"),
                "make the test pass, or move it into an `owner_pc::` or `real_claude::` module \
                 when it needs the owner's PC or real Claude; assert on an error instead of a panic",
            ));
        }
    }
    let whole = is_test_file(path);
    let blocks = if whole { vec![(0, code.len())] } else { test_blocks(code) };
    if blocks.is_empty() {
        return;
    }
    let paused = code.contains("start_paused") || !find_word(code, "pause").is_empty();
    let imported = |module: &str| {
        code.lines().any(|line| {
            let line = line.trim_start();
            line.starts_with("use ")
                && line.contains(module)
                && !find_word(line, "sleep").is_empty()
        })
    };
    let mut sleeps: Vec<usize> = Vec::new();
    for at in find_word(code, "sleep").into_iter().chain(find_word(code, "sleep_until")) {
        let line_text = code.get(..at).and_then(|b| b.rsplit('\n').next()).unwrap_or_default();
        if line_text.trim_start().starts_with("use ") {
            continue;
        }
        let before = code.get(..at).unwrap_or_default().trim_end();
        let after = code.get(at..).unwrap_or_default();
        let call = after.trim_start_matches(|c: char| is_ident(c)).trim_start().starts_with('(');
        let real = if before.ends_with("thread::") {
            true
        } else if before.ends_with("time::") {
            !paused
        } else if before.ends_with('.') || before.ends_with("::") || !call {
            continue;
        } else if imported("thread::") {
            true
        } else if imported("time::") {
            !paused
        } else {
            continue;
        };
        if real && blocks.iter().any(|(start, end)| *start <= at && at < *end) {
            sleeps.push(at);
        }
    }
    for at in sleeps {
        findings.push(Finding::new(
            Check::Tests,
            path,
            Some(line_of(code, at)),
            "a real-time sleep in test code",
            "drive time with a manual clock (`ManualClock`) or tokio's paused time instead",
        ));
    }
}

#[cfg(test)]
mod tests;
