//! Fixture tests of checks 10, 14 and 16: each seeds one violation into the clean tree.

use super::super::mem::MemTree;
use super::super::{Check, run};

fn found(tree: &MemTree, check: Check) -> Vec<String> {
    run(tree)
        .unwrap()
        .into_iter()
        .filter(|f| f.check == check)
        .map(|f| format!("{}:{} {}", f.path, f.line.unwrap_or(0), f.problem))
        .collect()
}

#[test]
fn unsafe_code_outside_an_ffi_module_fails() {
    let mut tree = MemTree::clean();
    tree.set(
        "crates/demo/src/io.rs",
        "// unsafe in a comment is fine\nconst S: &str = \"unsafe\";\nfn f() {\n    unsafe { g() }\n}\n",
    );
    tree.set(
        "crates/demo/src/ffi.rs",
        "fn h() {\n    // SAFETY: none needed.\n    unsafe { g() }\n}\n",
    );
    tree.set("crates/demo/src/ffi/uia.rs", "unsafe fn k() {}\n");
    let found = found(&tree, Check::Unsafe);
    assert_eq!(found, ["crates/demo/src/io.rs:4 unsafe code outside an `ffi` module"], "{found:?}");
}

#[test]
fn the_workspace_must_deny_unsafe_code_and_undocumented_blocks() {
    let mut tree = MemTree::clean();
    tree.set(
        "Cargo.toml",
        "[workspace]\nmembers = [\"crates/*\"]\n\n[workspace.lints.rust]\nunsafe_code = \"warn\"\n\n\
         [workspace.lints.clippy]\nundocumented_unsafe_blocks = { level = \"deny\", priority = 1 }\n",
    );
    let found = found(&tree, Check::Unsafe);
    assert_eq!(found.len(), 2, "unsafe_code warns and one lint is missing: {found:?}");
    assert!(found.iter().any(|f| f.contains("`unsafe_code`")), "{found:?}");
    assert!(found.iter().any(|f| f.contains("`multiple_unsafe_ops_per_block`")), "{found:?}");
}

#[test]
fn the_real_workspace_lints_deny_unsafe_code_and_hold_the_safety_comments() {
    // Check 10 is delegated to clippy for the safety comments and the one operation per block;
    // its configuration assertion runs over the repository's own root manifest.
    let manifest = std::fs::read_to_string(crate::repo_root().join("Cargo.toml")).unwrap();
    let mut tree = MemTree::clean();
    tree.set("Cargo.toml", &manifest);
    assert_eq!(found(&tree, Check::Unsafe), Vec::<String>::new(), "the root Cargo.toml");
    for lint in [
        "unsafe_code = \"deny\"",
        "undocumented_unsafe_blocks = \"deny\"",
        "multiple_unsafe_ops_per_block = \"deny\"",
    ] {
        assert!(manifest.contains(lint), "the root Cargo.toml has `{lint}`");
    }
}

#[test]
fn a_credential_path_or_the_registration_command_in_source_fails() {
    let mut tree = MemTree::clean();
    tree.set(
        "crates/demo/src/io.rs",
        "// see C:\\\\Users\\\\x\\\\.VENDOR-CLI\\\\secret.json\nfn f() {}\n",
    );
    tree.set("crates/demo/config.toml", "run = \"vendor mcp add eileen\"\n");
    tree.set("docs/a.md", "Markdown may name .vendor-cli/secret.json to say it is never read.\n");
    let found = found(&tree, Check::CredentialBoundary);
    assert_eq!(found.len(), 2, "the source and the configuration, not the docs: {found:?}");
    assert!(found[0].starts_with("crates/demo/config.toml:1 names `vendor mcp add`"), "{found:?}");
    assert!(
        found[1].starts_with("crates/demo/src/io.rs:1 names `.vendor-cli/secret.json`"),
        "{found:?}"
    );
}

#[test]
fn a_spawn_outside_the_sanctioned_sites_fails() {
    let mut tree = MemTree::clean();
    tree.set(
        "crates/demo/src/spawn.rs",
        "#[expect(clippy::disallowed_methods, reason = \"the demo's one spawn\")]\n\
         pub(crate) fn spawn_child() {\n    std::process::Command::new(\"x\");\n}\n\
         fn other() {\n    let c = Command::new(\"y\");\n}\n\
         /// Never `Command::new` here.\nfn quiet() { let s = \"Command::new\"; }\n",
    );
    tree.set("xtask/src/run.rs", "fn git() { std::process::Command::new(\"git\"); }\n");
    let found = found(&tree, Check::CredentialBoundary);
    assert_eq!(
        found,
        ["crates/demo/src/spawn.rs:6 a process spawn outside the supervisor"],
        "{found:?}"
    );
}

#[test]
fn an_extra_disallowed_methods_expectation_fails() {
    let mut tree = MemTree::clean();
    tree.set(
        "crates/demo/src/clock.rs",
        "#[expect(\n    clippy::disallowed_methods,\n    reason = \"another clock\"\n)]\nfn now() {}\n\
         fn start() {\n    #[expect(clippy::disallowed_methods, reason = \"a test daemon\")]\n    let x = 1;\n}\n\
         #[expect(clippy::too_many_lines, reason = \"long\")]\nfn long() {}\n",
    );
    let found = found(&tree, Check::CredentialBoundary);
    assert_eq!(found.len(), 2, "the function and the statement, not the other lint: {found:?}");
    assert!(found[0].contains(":1 ") && found[0].contains("on `now`"), "{found:?}");
    assert!(found[1].contains(":7 ") && found[1].contains("on `start`"), "{found:?}");
}

#[test]
fn ignored_and_panicking_tests_fail() {
    let mut tree = MemTree::clean();
    tree.set(
        "crates/demo/tests/it/more.rs",
        "#[test]\n#[ignore = \"slow\"]\nfn a() {}\n#[test]\n#[should_panic]\nfn b() {}\n\
         const S: &str = \"#[ignore]\";\n",
    );
    let found = found(&tree, Check::Tests);
    assert_eq!(
        found,
        [
            "crates/demo/tests/it/more.rs:2 a `#[ignore]` test",
            "crates/demo/tests/it/more.rs:5 a `#[should_panic]` test"
        ],
        "{found:?}"
    );
}

#[test]
fn a_real_sleep_in_test_code_fails() {
    let mut tree = MemTree::clean();
    tree.set(
        "crates/demo/src/lib.rs",
        &format!(
            "{}\nfn wait() {{ std::thread::sleep(D); }}\n\n#[cfg(test)]\nmod tests {{\n    #[test]\n    fn t() {{\n        \
             std::thread::sleep(D);\n        clock.sleep(D);\n    }}\n}}\n",
            super::super::mem::GOOD_CRATE_DOC
        ),
    );
    tree.set(
        "crates/demo/tests/it/wait.rs",
        "use tokio::time::sleep;\n\n#[tokio::test]\nasync fn t() {\n    sleep(D).await;\n}\n",
    );
    tree.set(
        "crates/demo/tests/it/paused.rs",
        "#[tokio::test(start_paused = true)]\nasync fn t() {\n    tokio::time::sleep(D).await;\n}\n",
    );
    let found = found(&tree, Check::Tests);
    let lines: Vec<&str> = found.iter().map(|f| f.split(' ').next().unwrap_or_default()).collect();
    let doc_lines = super::super::mem::GOOD_CRATE_DOC.lines().count();
    assert_eq!(
        lines,
        [
            format!("crates/demo/src/lib.rs:{}", doc_lines + 8).as_str(),
            "crates/demo/tests/it/wait.rs:5"
        ],
        "the test module's thread sleep and the unpaused tokio sleep, not the product code, the \
         manual clock or paused time: {found:?}"
    );
}

#[test]
fn a_long_module_needs_a_reason() {
    let mut tree = MemTree::clean();
    tree.set("crates/demo/src/big.rs", &"// a line\n".repeat(801));
    let found_long = found(&tree, Check::Tests);
    assert_eq!(
        found_long,
        ["crates/demo/src/big.rs:801 the module has 801 lines; over 800 needs a reason"]
    );
    tree.set(
        "xtask/tidy/allow.toml",
        "[[module]]\npath = \"crates/demo/src/big.rs\"\nreason = \"one table\"\n",
    );
    assert!(found(&tree, Check::Tests).is_empty(), "listed with a reason");
}
