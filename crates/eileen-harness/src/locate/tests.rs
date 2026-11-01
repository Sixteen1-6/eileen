use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use super::{
    ClaudeInstall, ClaudeVersion, InstallKind, LocateEnv, LocateError, MIN_CLI, Refusal,
    RefusalReason, is_desktop_app_copy, lexical, locate_claude,
};

/// A folder under the system's temporary folder, deleted when dropped.
pub(crate) struct Scratch(PathBuf);

impl Scratch {
    pub(crate) fn new() -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("eileen-locate-{}-{n}", std::process::id()));
        if dir.exists() {
            fs::remove_dir_all(&dir).unwrap();
        }
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    pub(crate) fn path(&self) -> &Path {
        &self.0
    }

    /// Writes `text` at `relative`, making its folders.
    pub(crate) fn file(&self, relative: &str, text: &str) -> PathBuf {
        let path: PathBuf = relative.split('/').fold(self.0.clone(), |path, part| path.join(part));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, text).unwrap();
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        // A file another process still holds open stays; the next run's name differs.
        let _cleanup: Result<(), _> = fs::remove_dir_all(&self.0);
    }
}

/// The `claude.cmd` npm 10 writes for a package whose bin is a native executable.
pub(crate) const CMD_SHIM: &str = "@ECHO off\r\nGOTO start\r\n:find_dp0\r\nSET dp0=%~dp0\r\n\
EXIT /b\r\n:start\r\nSETLOCAL\r\nCALL :find_dp0\r\n\
\"%dp0%\\node_modules\\@anthropic-ai\\claude-code\\bin\\claude.exe\"   %*\r\n";

/// The `claude.ps1` npm 10 writes beside it.
const PS1_SHIM: &str = "#!/usr/bin/env pwsh\n\
$basedir=Split-Path $MyInvocation.MyCommand.Definition -Parent\n\
$exe=\"\"\n\
if ($MyInvocation.ExpectingInput) {\n\
  $input | & \"$basedir/node_modules/@anthropic-ai/claude-code/bin/claude.exe\"   $args\n\
} else {\n\
  & \"$basedir/node_modules/@anthropic-ai/claude-code/bin/claude.exe\"   $args\n\
}\nexit $LASTEXITCODE\n";

/// The `claude.cmd` npm writes for a package whose bin is a script that node runs.
const NODE_SCRIPT_SHIM: &str = "@ECHO off\r\nSETLOCAL\r\nCALL :find_dp0\r\n\
IF EXIST \"%dp0%\\node.exe\" (\r\n  SET \"_prog=%dp0%\\node.exe\"\r\n) ELSE (\r\n  SET \"_prog=node\"\r\n)\r\n\
endLocal & \"%_prog%\"  \"%dp0%\\node_modules\\@anthropic-ai\\claude-code\\cli.js\" %*\r\n";

/// A fake npm global folder at `npm/` holding the shims and the package at `version`.
pub(crate) fn npm_layout(scratch: &Scratch, version: &str) -> (PathBuf, PathBuf) {
    let shim = scratch.file("npm/claude.cmd", CMD_SHIM);
    scratch.file("npm/claude.ps1", PS1_SHIM);
    scratch.file("npm/claude", "#!/bin/sh\n");
    let exe = scratch.file("npm/node_modules/@anthropic-ai/claude-code/bin/claude.exe", "MZ");
    scratch.file(
        "npm/node_modules/@anthropic-ai/claude-code/package.json",
        &format!("{{\"name\":\"@anthropic-ai/claude-code\",\"version\":\"{version}\"}}"),
    );
    (shim, exe)
}

/// A Windows host whose `PATH` is `dirs`, with no npm or native folder of its own.
pub(crate) fn windows_env(dirs: &[&Path]) -> LocateEnv {
    LocateEnv {
        path_dirs: dirs.iter().map(|dir| dir.to_path_buf()).collect(),
        appdata: None,
        home: None,
        windows: true,
    }
}

fn version(text: &str) -> ClaudeVersion {
    ClaudeVersion::parse(text).unwrap()
}

#[test]
fn the_npm_shim_resolves_to_the_real_binary() {
    let scratch = Scratch::new();
    let (shim, exe) = npm_layout(&scratch, "2.1.304");

    let install = locate_claude(&windows_env(&[&scratch.path().join("npm")])).unwrap();

    assert_eq!(
        install,
        ClaudeInstall {
            kind: InstallKind::NpmShim { shim },
            exe,
            version: Some(version("2.1.304")),
            skipped: Vec::new(),
        },
        "the cmd shim leads to the package's binary, whose version its package.json gives"
    );
}

#[test]
fn the_powershell_shim_resolves_when_it_is_the_only_one() {
    let scratch = Scratch::new();
    let shim = scratch.file("npm/claude.ps1", PS1_SHIM);
    let exe = scratch.file("npm/node_modules/@anthropic-ai/claude-code/bin/claude.exe", "MZ");

    let install = locate_claude(&windows_env(&[&scratch.path().join("npm")])).unwrap();

    assert_eq!(install.kind, InstallKind::NpmShim { shim }, "the ps1 shim is the npm shim");
    assert_eq!(install.exe, exe, "and its target the binary");
    assert_eq!(install.version, None, "no package.json, so no version from the layout");
}

#[test]
fn the_npm_folder_under_appdata_is_searched_when_path_lacks_it() {
    let scratch = Scratch::new();
    let (shim, exe) = npm_layout(&scratch, "2.1.300");
    let env = LocateEnv { appdata: Some(scratch.path().to_path_buf()), ..windows_env(&[]) };

    let install = locate_claude(&env).unwrap();

    assert_eq!(install.kind, InstallKind::NpmShim { shim }, "%APPDATA%\\npm holds the shim");
    assert_eq!(install.exe, exe, "and the package's binary is behind it");
}

#[test]
fn a_shim_that_runs_a_node_script_is_refused() {
    let scratch = Scratch::new();
    let shim = scratch.file("npm/claude.cmd", NODE_SCRIPT_SHIM);
    scratch.file("npm/node_modules/@anthropic-ai/claude-code/cli.js", "");

    let found = locate_claude(&windows_env(&[&scratch.path().join("npm")]));

    assert_eq!(
        found,
        Err(LocateError::NoUsableCli {
            refused: vec![Refusal { path: shim, reason: RefusalReason::ShimWithoutBinary }],
        }),
        "a package without a native binary has no real executable to run"
    );
}

#[test]
fn the_native_layout_resolves_to_its_binary_and_leaves_the_version_to_the_cli() {
    let scratch = Scratch::new();
    let exe = scratch.file("home/.local/bin/claude.exe", "MZ");
    scratch.file("home/.local/share/claude/versions/2.1.290", "MZ");
    scratch.file("home/.local/share/claude/versions/2.1.304", "MZ");
    scratch.file("home/.local/share/claude/versions/not-a-version", "");
    let env = LocateEnv { home: Some(scratch.path().join("home")), ..windows_env(&[]) };

    let install = locate_claude(&env).unwrap();

    assert_eq!(
        install,
        ClaudeInstall { kind: InstallKind::Native, exe, version: None, skipped: Vec::new() },
        "the native installer's binary is a copy of some release, not always the newest one"
    );
}

#[test]
fn the_desktop_app_copy_is_refused() {
    let scratch = Scratch::new();
    let exe =
        scratch.file("Local/Claude-Data/app-2.31226.1.0/claude-code/2.1.295/claude.exe", "MZ");

    let found = locate_claude(&windows_env(&[exe.parent().unwrap()]));

    assert_eq!(
        found,
        Err(LocateError::NoUsableCli {
            refused: vec![Refusal { path: exe, reason: RefusalReason::DesktopApp }],
        }),
        "the copy bundled with the Claude desktop app is never run"
    );
}

#[test]
fn the_desktop_app_copy_is_skipped_for_the_npm_install_after_it() {
    let scratch = Scratch::new();
    let desktop = scratch.file("Roaming/Claude/claude-code/2.1.295/claude.exe", "MZ");
    let (shim, exe) = npm_layout(&scratch, "2.1.304");

    let install =
        locate_claude(&windows_env(&[desktop.parent().unwrap(), &scratch.path().join("npm")]))
            .unwrap();

    assert_eq!(install.kind, InstallKind::NpmShim { shim }, "the npm install is chosen");
    assert_eq!(install.exe, exe, "with its own binary");
    assert_eq!(
        install.skipped,
        vec![Refusal { path: desktop, reason: RefusalReason::DesktopApp }],
        "and the desktop copy before it on PATH is named as skipped"
    );
}

#[test]
fn a_shim_that_leads_into_the_desktop_app_is_refused() {
    let scratch = Scratch::new();
    let shim = scratch.file(
        "npm/claude.cmd",
        "@ECHO off\r\n\"%dp0%\\..\\WindowsApps\\Claude_2.31226.1.0_x64__k\\claude.exe\" %*\r\n",
    );
    scratch.file("WindowsApps/Claude_2.31226.1.0_x64__k/claude.exe", "MZ");

    let found = locate_claude(&windows_env(&[&scratch.path().join("npm")]));

    assert_eq!(
        found,
        Err(LocateError::NoUsableCli {
            refused: vec![Refusal { path: shim, reason: RefusalReason::DesktopApp }],
        }),
        "the shim's target is checked as well as the shim"
    );
}

#[test]
fn a_missing_cli_is_a_typed_error() {
    let scratch = Scratch::new();
    let empty = scratch.path().join("bin");
    fs::create_dir_all(&empty).unwrap();
    let env = LocateEnv { appdata: Some(scratch.path().join("Roaming")), ..windows_env(&[&empty]) };

    let found = locate_claude(&env);

    assert_eq!(
        found,
        Err(LocateError::NotFound {
            searched: vec![empty, scratch.path().join("Roaming").join("npm")],
        }),
        "every folder searched is named, PATH first and then the npm folder"
    );
}

#[test]
fn a_folder_under_the_users_claude_folder_is_never_searched() {
    let scratch = Scratch::new();
    let local = scratch.path().join("home").join(".claude").join("local");
    scratch.file("home/.claude/local/claude.cmd", CMD_SHIM);

    let found = locate_claude(&windows_env(&[&local]));

    assert_eq!(
        found,
        Err(LocateError::NoUsableCli {
            refused: vec![Refusal { path: local, reason: RefusalReason::UnderClaudeFolder }],
        }),
        "a PATH folder under .claude is refused before anything in it is looked at"
    );
}

#[cfg(unix)]
#[test]
fn an_npm_link_on_unix_resolves_to_the_package_binary() {
    let scratch = Scratch::new();
    let exe = scratch.file("lib/node_modules/@anthropic-ai/claude-code/bin/claude.exe", "MZ");
    scratch.file(
        "lib/node_modules/@anthropic-ai/claude-code/package.json",
        "{\"version\":\"2.1.304\"}",
    );
    fs::create_dir_all(scratch.path().join("bin")).unwrap();
    let link = scratch.path().join("bin").join("claude");
    std::os::unix::fs::symlink(&exe, &link).unwrap();
    let env = LocateEnv { windows: false, ..windows_env(&[&scratch.path().join("bin")]) };

    let install = locate_claude(&env).unwrap();

    assert_eq!(install.kind, InstallKind::NpmShim { shim: link }, "npm's link is its shim");
    assert_eq!(install.exe, exe, "the link's target is the binary");
    assert_eq!(install.version, Some(version("2.1.304")), "read from the package");
}

#[test]
fn desktop_app_paths_are_recognised_in_windows_form_on_any_os() {
    for path in [
        r"C:\Users\u\AppData\Local\Claude-Data\x\claude-code\2.1.304\claude.exe",
        r"C:\Users\u\AppData\Roaming\Claude\claude-code\2.1.295\claude.exe",
        r"C:\Users\u\AppData\Local\AnthropicClaude\app-1.0\claude.exe",
        r"C:\Program Files\WindowsApps\Claude_2.31226.1.0_x64__abc\app\claude.exe",
        r"C:\Users\u\AppData\Local\Packages\Claude_abc\LocalCache\claude.exe",
    ] {
        assert!(is_desktop_app_copy(Path::new(path)), "{path} is the desktop app's");
    }
    for path in [
        r"C:\Users\u\AppData\Roaming\npm\node_modules\@anthropic-ai\claude-code\bin\claude.exe",
        r"C:\Users\u\.local\bin\claude.exe",
        r"C:\Users\u\.local\share\claude\versions\2.1.304",
        r"C:\Users\u\Packages\notes\claude.exe",
    ] {
        assert!(!is_desktop_app_copy(Path::new(path)), "{path} is not the desktop app's");
    }
}

#[test]
fn versions_compare_by_number_and_the_floor_is_2_1_284() {
    assert!(version("2.1.99") < version("2.1.100"), "by number, not by text");
    assert!(version("2.1.283") < MIN_CLI, "under the floor");
    assert_eq!(MIN_CLI, version("2.1.284"), "the D77 floor");
    assert_eq!(
        ClaudeVersion::parse_version_output("2.1.304 (Claude Code)\n"),
        Some(version("2.1.304")),
        "`claude --version` prints the number first"
    );
    assert_eq!(ClaudeVersion::parse("2.1"), None, "three parts");
    assert_eq!(ClaudeVersion::parse("2.1.x"), None, "numbers only");
    assert_eq!(version("2.1.304").to_string(), "2.1.304", "displays as written");
}

#[test]
fn a_claude_exe_comes_before_the_shims_in_one_folder() {
    let scratch = Scratch::new();
    let exe = scratch.file("home/.local/bin/claude.exe", "MZ");
    scratch.file("home/.local/bin/claude.cmd", CMD_SHIM);

    let install = locate_claude(&windows_env(&[&scratch.path().join("home/.local/bin")])).unwrap();

    assert_eq!(install.exe, exe, "cmd.exe runs claude.exe first, as the default PATHEXT orders");
    assert_eq!(install.kind, InstallKind::Native, "the native installer's");
}

#[test]
fn a_bare_claude_exe_in_another_folder_is_passed_over() {
    let scratch = Scratch::new();
    let stray = scratch.file("tools/claude.exe", "MZ");
    let (shim, exe) = npm_layout(&scratch, "2.1.304");
    let env = windows_env(&[&scratch.path().join("tools"), &scratch.path().join("npm")]);

    let install = locate_claude(&env).unwrap();

    assert_eq!(install.exe, exe, "the npm install after it is chosen");
    assert_eq!(install.kind, InstallKind::NpmShim { shim }, "through its shim");
    assert_eq!(
        install.skipped,
        vec![Refusal { path: stray, reason: RefusalReason::UnknownInstall }],
        "an executable of neither layout is named and passed over"
    );
}

#[test]
fn a_shim_that_names_a_binary_outside_the_npm_package_is_passed_over() {
    let scratch = Scratch::new();
    scratch.file("elsewhere/claude.exe", "MZ");
    let shim =
        scratch.file("npm/claude.cmd", "@ECHO off\r\n\"%dp0%\\..\\elsewhere\\claude.exe\" %*\r\n");

    let found = locate_claude(&windows_env(&[&scratch.path().join("npm")]));

    assert_eq!(
        found,
        Err(LocateError::NoUsableCli {
            refused: vec![Refusal { path: shim, reason: RefusalReason::UnknownInstall }],
        }),
        "a shim is accepted only when it leads into npm's package"
    );
}

#[test]
fn a_shim_that_leads_under_a_claude_folder_is_refused_before_anything_there_is_looked_at() {
    let scratch = Scratch::new();
    let shim = scratch.file(
        "npm/claude.cmd",
        "@ECHO off\r\n\"%dp0%\\..\\home\\.claude\\local\\claude.exe\" %*\r\n",
    );

    let found = locate_claude(&windows_env(&[&scratch.path().join("npm")]));

    assert_eq!(
        found,
        Err(LocateError::NoUsableCli {
            refused: vec![Refusal { path: shim, reason: RefusalReason::UnderClaudeFolder }],
        }),
        "refused by name: the target does not even exist, so no check of it ran first"
    );
}

#[test]
fn lexical_paths_drop_dots_and_apply_parents() {
    let path: PathBuf = ["a", "b", "..", "c", ".", "d"].iter().collect();
    let expected: PathBuf = ["a", "c", "d"].iter().collect();
    assert_eq!(lexical(&path), expected, "each `..` takes off the folder before it");
    let up: PathBuf = ["..", "a"].iter().collect();
    assert_eq!(lexical(&up), up, "a leading `..` stays");
}

#[cfg(unix)]
#[test]
fn a_link_into_the_desktop_app_is_refused() {
    let scratch = Scratch::new();
    let target = scratch.file("Roaming/Claude/claude-code/2.1.295/claude", "MZ");
    fs::create_dir_all(scratch.path().join("bin")).unwrap();
    let link = scratch.path().join("bin").join("claude");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let env = LocateEnv { windows: false, ..windows_env(&[&scratch.path().join("bin")]) };

    let found = locate_claude(&env);

    assert_eq!(
        found,
        Err(LocateError::NoUsableCli {
            refused: vec![Refusal { path: link, reason: RefusalReason::DesktopApp }],
        }),
        "the link's target is checked, not only the link"
    );
}

#[cfg(unix)]
#[test]
fn a_link_into_a_claude_folder_is_refused_without_following_it() {
    let scratch = Scratch::new();
    fs::create_dir_all(scratch.path().join("bin")).unwrap();
    let link = scratch.path().join("bin").join("claude");
    let target = scratch.path().join("home").join(".claude").join("local").join("claude");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let env = LocateEnv { windows: false, ..windows_env(&[&scratch.path().join("bin")]) };

    let found = locate_claude(&env);

    assert_eq!(
        found,
        Err(LocateError::NoUsableCli {
            refused: vec![Refusal { path: link, reason: RefusalReason::UnderClaudeFolder }],
        }),
        "refused by the target's name: the target does not exist, so nothing followed the link"
    );
}

#[cfg(unix)]
#[test]
fn a_folder_link_on_the_way_is_followed_too() {
    let scratch = Scratch::new();
    let target = scratch.file("Local/Claude-Data/x/claude", "MZ");
    let link = scratch.path().join("links");
    std::os::unix::fs::symlink(target.parent().unwrap(), &link).unwrap();
    let env = LocateEnv { windows: false, ..windows_env(&[&link]) };

    let found = locate_claude(&env);

    assert_eq!(
        found,
        Err(LocateError::NoUsableCli {
            refused: vec![Refusal { path: link.join("claude"), reason: RefusalReason::DesktopApp }],
        }),
        "a linked folder, like a Windows junction, is resolved before the desktop check"
    );
}

#[cfg(unix)]
#[test]
fn a_path_folder_linked_into_a_claude_folder_is_refused() {
    let scratch = Scratch::new();
    let link = scratch.path().join("bin");
    let target = scratch.path().join("home").join(".claude").join("local");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let env = LocateEnv { windows: false, ..windows_env(&[&link]) };

    let found = locate_claude(&env);

    assert_eq!(
        found,
        Err(LocateError::NoUsableCli {
            refused: vec![Refusal { path: link, reason: RefusalReason::UnderClaudeFolder }],
        }),
        "the folder's link is read, and its target refused by name before anything in it"
    );
}

#[cfg(unix)]
#[test]
fn the_native_link_on_unix_resolves_to_its_release() {
    let scratch = Scratch::new();
    let release = scratch.file("home/.local/share/claude/versions/2.1.316", "MZ");
    fs::create_dir_all(scratch.path().join("home/.local/bin")).unwrap();
    let link = scratch.path().join("home/.local/bin/claude");
    std::os::unix::fs::symlink(&release, &link).unwrap();
    let env =
        LocateEnv { windows: false, ..windows_env(&[&scratch.path().join("home/.local/bin")]) };

    let install = locate_claude(&env).unwrap();

    assert_eq!(install.kind, InstallKind::Native, "the native installer's link");
    assert_eq!(install.exe, release, "the release it leads to is the binary");
    assert_eq!(install.version, Some(version("2.1.316")), "named by the release");
}

#[cfg(unix)]
#[test]
fn a_unix_claude_outside_both_layouts_is_passed_over() {
    let scratch = Scratch::new();
    let stray = scratch.file(
        "bin/claude",
        "#!/bin/sh
",
    );
    let env = LocateEnv { windows: false, ..windows_env(&[&scratch.path().join("bin")]) };

    let found = locate_claude(&env);

    assert_eq!(
        found,
        Err(LocateError::NoUsableCli {
            refused: vec![Refusal { path: stray, reason: RefusalReason::UnknownInstall }],
        }),
        "neither npm's link nor the native installer's"
    );
}
