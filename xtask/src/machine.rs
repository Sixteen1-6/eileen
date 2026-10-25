//! The commands about this machine rather than a build: `setup`, `slots`, `desktop-lock`,
//! `fetch-native` and `lock` (BUILD.md sections 4.1, 4b.5 and 6.1).

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, BufRead as _, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::Duration;

use eileen_buildguard::baseline::{compute_baseline, hours_covered, utc_date};
use eileen_buildguard::clock::{Clock, RealClock};
use eileen_buildguard::samples::{self, Sample, Source};
use eileen_buildguard::slots::{self, Holder};
use eileen_buildguard::{
    MemorySource as _, Reason, Store, SystemMemory, acquire_desktop, pressure,
};

use crate::cli::{DesktopLockOptions, FetchNativeOptions, SlotsCommand};
use crate::sha256;

/// The prebuilt tools `setup` installs with cargo-binstall, at section 4.1's versions. The
/// others of that table are CI's (zizmor, cargo-fuzz, mutants, coverage, module maps) or the
/// owner's (sccache, cargo-binstall itself).
const TOOLS: [&str; 5] = [
    "cargo-nextest@0.9.148",
    "cargo-deny@0.20.2",
    "cargo-machete@0.9.2",
    "cargo-insta@1.49.0",
    "typos-cli@1.51.1",
];

/// A native library archive pinned by its size and hash.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NativeArchive {
    /// The file name, which `sherpa-onnx-sys` looks for in `SHERPA_ONNX_ARCHIVE_DIR`.
    pub(crate) file: &'static str,
    /// Where it is downloaded from.
    pub(crate) url: &'static str,
    /// Its size in bytes.
    pub(crate) size: u64,
    /// Its SHA-256, lowercase hex.
    pub(crate) sha256: &'static str,
    /// The folder the archive unpacks to, which holds `lib/`.
    pub(crate) folder: &'static str,
}

/// The native table: upstream's sherpa-onnx 1.13.8 shared MT archive until WP4's espeak-free
/// build replaces this one entry (BUILD.md section 4.1).
pub(crate) const SHERPA_ONNX: NativeArchive = NativeArchive {
    file: "sherpa-onnx-v1.13.8-win-x64-shared-MT-Release-lib.tar.bz2",
    url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/v1.13.8/sherpa-onnx-v1.13.8-win-x64-shared-MT-Release-lib.tar.bz2",
    size: 8_032_957,
    sha256: "b8eedf41bd6d3779218887b48367bb7a3ece5aaa7667f01f69ee823a12b0a9e7",
    folder: "sherpa-onnx-v1.13.8-win-x64-shared-MT-Release-lib",
};

/// The marker `fetch-native` writes once an archive's hash checked out, holding that hash.
const VERIFIED_SUFFIX: &str = ".verified";

/// The markers between which `slots --baseline` writes its percentiles into the WP0 file.
const BASELINE_START: &str = "<!-- baseline:start -->";
const BASELINE_END: &str = "<!-- baseline:end -->";

/// The work package file that records the baseline.
const WP0_FILE: &str = "docs/wp/WP0-foundation.md";

/// `cargo xtask setup`: the pinned toolchain, the prebuilt tools, the git hooks and, on
/// Windows, the native libraries.
pub(crate) fn setup(out: &mut dyn Write) -> io::Result<ExitCode> {
    let root = crate::repo_root();
    let mut failed = Vec::new();
    writeln!(out, "setup: the toolchain rust-toolchain.toml pins")?;
    if !run_tool(Command::new("rustup").args(["toolchain", "install"]).current_dir(&root)) {
        failed.push("the toolchain (rustup toolchain install)");
    }
    writeln!(out, "setup: the prebuilt tools, with cargo-binstall")?;
    if run_quiet(Command::new("cargo").args(["binstall", "-V"])) {
        let mut binstall = Command::new("cargo");
        binstall.args(["binstall", "--no-confirm", "--disable-strategies", "compile"]).args(TOOLS);
        if !run_tool(&mut binstall) {
            failed.push("the prebuilt tools (cargo binstall)");
        }
    } else {
        writeln!(
            out,
            "setup: cargo-binstall is not installed. Install its prebuilt 1.26.0 release (see \
             docs/DEV-SETUP.md) and run `cargo xtask setup` again; never `cargo install` from \
             source (BUILD.md section 6.1 rule 10)."
        )?;
        failed.push("the prebuilt tools (cargo-binstall is missing)");
    }
    writeln!(out, "setup: git hooks from .githooks")?;
    let mut hooks = Command::new("git");
    hooks.args(["config", "core.hooksPath", ".githooks"]).current_dir(&root);
    if !run_tool(&mut hooks) {
        failed.push("the git hooks (git config core.hooksPath .githooks)");
    }
    if cfg!(windows) && fetch_native(FetchNativeOptions::default(), out)? != ExitCode::SUCCESS {
        failed.push("the native libraries (cargo xtask fetch-native)");
    }
    if failed.is_empty() {
        writeln!(out, "setup: done")?;
        Ok(ExitCode::SUCCESS)
    } else {
        writeln!(out, "setup: these steps failed: {}", failed.join("; "))?;
        Ok(ExitCode::from(crate::EXIT_FINDINGS))
    }
}

/// Runs a tool with inherited output and says whether it succeeded.
fn run_tool(command: &mut Command) -> bool {
    command.status().is_ok_and(|status| status.success())
}

/// Runs a tool with its output discarded and says whether it succeeded.
fn run_quiet(command: &mut Command) -> bool {
    command.output().is_ok_and(|output| output.status.success())
}

/// `cargo xtask slots`: the build slots.
pub(crate) fn slots(command: SlotsCommand, out: &mut dyn Write) -> io::Result<ExitCode> {
    let store = Store::machine()?;
    fs::create_dir_all(store.root())?;
    match command {
        SlotsCommand::List => list_slots(&store, out),
        SlotsCommand::KillStale => {
            let freed = slots::kill_stale(store.root())?;
            if freed.is_empty() {
                writeln!(out, "slots: no stale holder records")?;
            }
            for f in freed {
                writeln!(
                    out,
                    "slots: freed slot {}, whose holder pid {} (`{}`) is gone",
                    f.slot.index(),
                    f.holder.pid,
                    f.holder.command
                )?;
            }
            Ok(ExitCode::SUCCESS)
        }
        SlotsCommand::Baseline { hours } => baseline(&store, hours, out),
        SlotsCommand::Pressure { gb } => {
            writeln!(
                out,
                "slots: committing and touching {gb} GB for 60 s, as section 6.1 rule 1 allows \
                 once with the owner's agreement"
            )?;
            pressure::pressure(gb, Duration::from_secs(60), &RealClock)?;
            writeln!(out, "slots: released")?;
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn list_slots(store: &Store, out: &mut dyn Write) -> io::Result<ExitCode> {
    let now = RealClock.now_ms();
    for status in slots::list(store.root())? {
        let state = match (&status.holder, status.held) {
            (Some(holder), true) => format!("held by {}", holder_text(holder, now)),
            (Some(holder), false) => format!(
                "free, with a stale record of {} (`cargo xtask slots --kill-stale` removes it)",
                holder_text(holder, now)
            ),
            (None, true) => "held, with no holder record".to_owned(),
            (None, false) => "free".to_owned(),
        };
        writeln!(out, "slot {}: {state}", status.slot.index())?;
    }
    let desktop = match slots::holder(store.root(), slots::DESKTOP) {
        Some(holder) if slots::is_held(store.root(), slots::DESKTOP)? => {
            format!("held by {}", holder_text(&holder, now))
        }
        _ => "free".to_owned(),
    };
    writeln!(out, "desktop.lock: {desktop}")?;
    match store.baseline()? {
        Some(b) => writeln!(
            out,
            "baseline: {} to {}, {} samples; slot 0 under {} KB/s, re-check under {} KB/s, \
             headroom floor {} MB",
            b.measured_from,
            b.measured_to,
            b.samples,
            b.slot0_paging_kb_s,
            b.recheck_paging_kb_s,
            b.headroom_floor_mb
        )?,
        None => writeln!(
            out,
            "baseline: none yet, so the code defaults apply (`cargo xtask slots --baseline`)"
        )?,
    }
    Ok(ExitCode::SUCCESS)
}

fn holder_text(holder: &Holder, now_ms: u64) -> String {
    let minutes = now_ms.saturating_sub(holder.started_ms) / 60_000;
    format!(
        "pid {} running `{}` in {} for {minutes} min",
        holder.pid, holder.command, holder.worktree
    )
}

/// `slots --baseline`: samples every 5 s into the machine-wide store, and every 5 minutes (and
/// at the end) writes `buildguard.toml` and the percentiles, for `hours` or until stopped.
fn baseline(store: &Store, hours: Option<u32>, out: &mut dyn Write) -> io::Result<ExitCode> {
    let clock = RealClock;
    let mut memory = SystemMemory::new(&clock)?;
    let started = clock.mono_ms();
    let end = hours.map(|h| started.saturating_add(u64::from(h).saturating_mul(3_600_000)));
    let every_write = 5 * 60_000;
    let mut next_write = started.saturating_add(every_write);
    writeln!(
        out,
        "slots --baseline: sampling every 5 s; stop it with Ctrl+C at any time, the last write \
         stands (a typical working day needs at least four separate hours)"
    )?;
    loop {
        let reading = memory.sample(&clock)?;
        let sample = Sample {
            at_ms: clock.now_ms(),
            avail_mb: reading.avail_mb,
            headroom_mb: reading.headroom_mb,
            paging_kb_s: reading.paging_kb_s,
            slot_held: store.slot_held(),
            src: Source::Guard,
        };
        store.append(&sample, &clock)?;
        let now = clock.mono_ms();
        let done = end.is_some_and(|end| now >= end);
        if now >= next_write || done {
            write_baseline(store, out)?;
            next_write = now.saturating_add(every_write);
        }
        if done {
            return Ok(ExitCode::SUCCESS);
        }
        clock.sleep(Duration::from_secs(5));
    }
}

fn write_baseline(store: &Store, out: &mut dyn Write) -> io::Result<()> {
    let samples = store.samples()?;
    let Some(computed) = compute_baseline(&samples) else {
        writeln!(out, "slots --baseline: no samples without a held build slot yet")?;
        return Ok(());
    };
    store.write_baseline(&computed)?;
    let hours = hours_covered(&samples);
    let report = baseline_report(&samples, hours);
    let wp0 = crate::repo_root().join(WP0_FILE);
    let written = fs::read_to_string(&wp0)
        .ok()
        .and_then(|text| replace_between(&text, BASELINE_START, BASELINE_END, &report))
        .map(|text| fs::write(&wp0, text));
    match written {
        Some(result) => {
            result?;
            writeln!(out, "slots --baseline: wrote buildguard.toml and {WP0_FILE}")?;
        }
        None => writeln!(
            out,
            "slots --baseline: wrote buildguard.toml; {WP0_FILE} has no baseline markers, so \
             here are the percentiles:\n{report}"
        )?,
    }
    if hours < 4 {
        writeln!(
            out,
            "slots --baseline: only {hours} separate hours so far; a typical working day needs \
             at least four"
        )?;
    }
    Ok(())
}

/// The percentiles the WP0 file records: the 30 s paging averages and the headroom and
/// available memory, over the samples no build slot held.
pub(crate) fn baseline_report(samples: &[Sample], hours: usize) -> String {
    let kept: Vec<&Sample> = samples.iter().filter(|s| !s.slot_held).collect();
    let mut windows: BTreeMap<u64, (u64, u64)> = BTreeMap::new();
    for s in &kept {
        let entry = windows.entry(s.at_ms / 30_000).or_insert((0, 0));
        entry.0 = entry.0.saturating_add(u64::from(s.paging_kb_s));
        entry.1 = entry.1.saturating_add(1);
    }
    let paging: Vec<u32> = windows
        .values()
        .map(|(sum, n)| u32::try_from(sum / (*n).max(1)).unwrap_or(u32::MAX))
        .collect();
    let headroom: Vec<u32> = kept.iter().map(|s| s.headroom_mb).collect();
    let avail: Vec<u32> = kept.iter().map(|s| s.avail_mb).collect();
    let cell = |values: &[u32], pct: u32| {
        samples::percentile(values, pct).map_or_else(|| "-".to_owned(), |v| v.to_string())
    };
    let row = |name: &str, values: &[u32]| {
        format!(
            "| {name} | {} | {} | {} | {} | {} |",
            cell(values, 10),
            cell(values, 50),
            cell(values, 75),
            cell(values, 90),
            cell(values, 95)
        )
    };
    let first = kept.iter().map(|s| s.at_ms).min().map_or_else(String::new, utc_date);
    let last = kept.iter().map(|s| s.at_ms).max().map_or_else(String::new, utc_date);
    [
        format!(
            "Measured {first} to {last}: {} samples in {hours} separate hours, {} left out because \
             a build slot was held.",
            kept.len(),
            samples.len() - kept.len()
        ),
        String::new(),
        "| Measure | p10 | p50 | p75 | p90 | p95 |".to_owned(),
        "|---|---|---|---|---|---|".to_owned(),
        row("Paging, 30 s average (KB/s)", &paging),
        row("Commit headroom (MB)", &headroom),
        row("Available memory (MB)", &avail),
    ]
    .join("\n")
}

/// `text` with everything between the `start` and `end` markers replaced by `block`, or `None`
/// when the markers are missing or out of order.
pub(crate) fn replace_between(text: &str, start: &str, end: &str, block: &str) -> Option<String> {
    let open = text.find(start)? + start.len();
    let close = open + text.get(open..)?.find(end)?;
    Some(format!("{}\n{block}\n{}", text.get(..open)?, text.get(close..)?))
}

/// `cargo xtask desktop-lock --hold`: holds `desktop.lock` until Enter, standard input closing,
/// or the process being stopped (the OS releases the lock then).
pub(crate) fn desktop_lock(
    options: DesktopLockOptions,
    out: &mut dyn Write,
) -> io::Result<ExitCode> {
    let store = Store::machine()?;
    fs::create_dir_all(store.root())?;
    let clock = RealClock;
    let holder = Holder {
        pid: std::process::id(),
        worktree: crate::repo_root().display().to_string(),
        command: "cargo xtask desktop-lock --hold".to_owned(),
        started_ms: clock.now_ms(),
    };
    let mut report = |reason: &Reason, holders: &[Holder]| {
        let who: Vec<String> = holders.iter().map(|h| holder_text(h, clock.now_ms())).collect();
        // Nobody is left to tell if standard error is closed; the wait goes on either way.
        let _reported: io::Result<()> =
            writeln!(io::stderr().lock(), "desktop-lock: waiting: {reason}; {}", who.join("; "));
    };
    match acquire_desktop(&store, &holder, options.wait_max, &clock, &mut report) {
        Ok(held) => {
            writeln!(out, "desktop-lock: holding desktop.lock; press Enter to release it")?;
            out.flush()?;
            let mut line = String::new();
            io::stdin().lock().read_line(&mut line)?;
            drop(held);
            writeln!(out, "desktop-lock: released")?;
            Ok(ExitCode::SUCCESS)
        }
        Err(refusal) => {
            writeln!(out, "desktop-lock: {refusal}; exit code {}", refusal.exit_code())?;
            Ok(exit_code(refusal.exit_code()))
        }
    }
}

/// An exit code from an `i32`, 2 when it does not fit.
fn exit_code(code: i32) -> ExitCode {
    ExitCode::from(u8::try_from(code).unwrap_or(crate::EXIT_ERROR))
}

/// Where the native archives live: beside the machine-wide files, whatever `EILEEN_DATA_DIR`
/// says.
fn native_root() -> io::Result<PathBuf> {
    Ok(samples::machine_root()?.join("native"))
}

/// The variables every cargo invocation `cargo xtask` makes gets: on Windows, once
/// `fetch-native` has verified the archive, where sherpa-onnx and ONNX Runtime are; otherwise
/// none.
pub(crate) fn native_env() -> Vec<(String, String)> {
    if !cfg!(windows) {
        return Vec::new();
    }
    native_root().ok().and_then(|root| env_for(&root, &SHERPA_ONNX)).unwrap_or_default()
}

/// The variables for `archive` under `root`, when its verified marker and its unpacked `lib`
/// folder are there.
pub(crate) fn env_for(root: &Path, archive: &NativeArchive) -> Option<Vec<(String, String)>> {
    let marker =
        fs::read_to_string(root.join(format!("{}{VERIFIED_SUFFIX}", archive.file))).ok()?;
    let lib = root.join(archive.folder).join("lib");
    (marker.trim() == archive.sha256 && lib.is_dir()).then(|| {
        vec![
            ("SHERPA_ONNX_ARCHIVE_DIR".to_owned(), root.display().to_string()),
            ("ORT_LIB_LOCATION".to_owned(), lib.display().to_string()),
            ("ORT_PREFER_DYNAMIC_LINK".to_owned(), "1".to_owned()),
        ]
    })
}

/// Whether the file at `path` has `archive`'s size and hash.
pub(crate) fn verify(path: &Path, archive: &NativeArchive) -> io::Result<bool> {
    if fs::metadata(path)?.len() != archive.size {
        return Ok(false);
    }
    let digest = sha256::digest_reader(fs::File::open(path)?)?;
    Ok(sha256::hex(&digest) == archive.sha256)
}

/// The entries of `root` an older pin left: anything named like a sherpa-onnx archive, folder
/// or marker that is not `archive`'s.
pub(crate) fn stale_entries(root: &Path, archive: &NativeArchive) -> io::Result<Vec<PathBuf>> {
    let keep = [
        archive.file.to_owned(),
        archive.folder.to_owned(),
        format!("{}{VERIFIED_SUFFIX}", archive.file),
    ];
    let mut stale = Vec::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("sherpa-onnx-") && !keep.contains(&name) {
            stale.push(entry.path());
        }
    }
    stale.sort();
    Ok(stale)
}

/// `cargo xtask fetch-native`: on Windows, downloads the pinned archive with curl when it is
/// missing or wrong, checks its size and hash, unpacks it with tar, removes what older pins
/// left, and says (or with `--print-env` prints) the variables cargo gets. Off Windows there is
/// nothing to fetch: the native libraries are Windows only.
pub(crate) fn fetch_native(
    options: FetchNativeOptions,
    out: &mut dyn Write,
) -> io::Result<ExitCode> {
    if !cfg!(windows) {
        if !options.print_env {
            writeln!(
                out,
                "fetch-native: nothing to fetch on this OS; the native libraries are Windows only \
                 (BUILD.md section 4.1)"
            )?;
        }
        return Ok(ExitCode::SUCCESS);
    }
    let root = native_root()?;
    fs::create_dir_all(&root)?;
    let archive = SHERPA_ONNX;
    let path = root.join(archive.file);
    if !path.is_file() || !verify(&path, &archive)? {
        note(options.print_env, out, &format!("fetch-native: downloading {}", archive.url))?;
        let partial = root.join(format!("{}.partial", archive.file));
        let mut curl = Command::new("curl");
        curl.args(["--fail", "--location", "--retry", "5", "--silent", "--show-error", "--output"])
            .arg(&partial)
            .arg(archive.url);
        if !run_tool(&mut curl) {
            note(options.print_env, out, "fetch-native: the download failed")?;
            return Ok(ExitCode::from(crate::EXIT_ERROR));
        }
        if !verify(&partial, &archive)? {
            fs::remove_file(&partial)?;
            note(
                options.print_env,
                out,
                "fetch-native: the download does not match the pinned size and hash",
            )?;
            return Ok(ExitCode::from(crate::EXIT_FINDINGS));
        }
        fs::rename(&partial, &path)?;
    }
    fs::write(root.join(format!("{}{VERIFIED_SUFFIX}", archive.file)), archive.sha256)?;
    if !root.join(archive.folder).join("lib").is_dir() {
        let mut tar = Command::new("tar");
        tar.arg("-xjf").arg(&path).arg("-C").arg(&root);
        if !run_tool(&mut tar) {
            note(
                options.print_env,
                out,
                &format!("fetch-native: unpacking {} failed", archive.file),
            )?;
            return Ok(ExitCode::from(crate::EXIT_ERROR));
        }
    }
    for stale in stale_entries(&root, &archive)? {
        note(
            options.print_env,
            out,
            &format!("fetch-native: removing {}, left by an older pin", stale.display()),
        )?;
        if stale.is_dir() { fs::remove_dir_all(&stale)? } else { fs::remove_file(&stale)? }
    }
    let Some(env) = env_for(&root, &archive) else {
        note(
            options.print_env,
            out,
            &format!("fetch-native: the archive did not unpack to {}/lib", archive.folder),
        )?;
        return Ok(ExitCode::from(crate::EXIT_ERROR));
    };
    for (name, value) in &env {
        if options.print_env {
            writeln!(out, "{name}={value}")?;
        } else {
            writeln!(out, "fetch-native: {name}={value}")?;
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// Progress of `fetch-native`: on standard error under `--print-env`, whose standard output is
/// for the variables alone, else on `out`.
fn note(print_env: bool, out: &mut dyn Write, text: &str) -> io::Result<()> {
    if print_env { writeln!(io::stderr().lock(), "{text}") } else { writeln!(out, "{text}") }
}

/// The third-party packages of a `Cargo.lock`: name and version of every package with a
/// source.
pub(crate) fn third_party(lockfile: &str) -> Result<Vec<(String, String)>, String> {
    let value: toml::Table = toml::from_str(lockfile).map_err(|e| format!("Cargo.lock: {e}"))?;
    let packages =
        value.get("package").and_then(toml::Value::as_array).cloned().unwrap_or_default();
    let mut found: Vec<(String, String)> = packages
        .iter()
        .filter(|p| p.get("source").is_some())
        .filter_map(|p| {
            let name = p.get("name")?.as_str()?.to_owned();
            let version = p.get("version")?.as_str()?.to_owned();
            Some((name, version))
        })
        .collect();
    found.sort();
    Ok(found)
}

/// `cargo xtask lock`: `cargo update --workspace`, which refreshes the workspace's own entries
/// after a lockfile merge, failing (and putting the old lockfile back) if any third-party
/// version changed (BUILD.md section 3.1).
pub(crate) fn lock(out: &mut dyn Write) -> io::Result<ExitCode> {
    let root = crate::repo_root();
    let lockfile = root.join("Cargo.lock");
    let before = fs::read_to_string(&lockfile)?;
    if !run_tool(Command::new("cargo").args(["update", "--workspace"]).current_dir(&root)) {
        writeln!(out, "lock: cargo update --workspace failed")?;
        return Ok(ExitCode::from(crate::EXIT_ERROR));
    }
    let after = fs::read_to_string(&lockfile)?;
    let parsed = third_party(&before).and_then(|b| Ok((b, third_party(&after)?)));
    let (old, new) = match parsed {
        Ok(pair) => pair,
        Err(problem) => {
            fs::write(&lockfile, &before)?;
            writeln!(out, "lock: {problem}; the old Cargo.lock is back")?;
            return Ok(ExitCode::from(crate::EXIT_ERROR));
        }
    };
    if old == new {
        writeln!(out, "lock: {} third-party packages, none changed", new.len())?;
        return Ok(ExitCode::SUCCESS);
    }
    fs::write(&lockfile, &before)?;
    for gone in old.iter().filter(|p| !new.contains(p)) {
        writeln!(out, "lock: would remove {} {}", gone.0, gone.1)?;
    }
    for added in new.iter().filter(|p| !old.contains(p)) {
        writeln!(out, "lock: would add {} {}", added.0, added.1)?;
    }
    writeln!(
        out,
        "lock: a third-party version would change, so the old Cargo.lock is back; take main's \
         lockfile and run `cargo xtask lock` again, or change the pins in an amendment"
    )?;
    Ok(ExitCode::from(crate::EXIT_FINDINGS))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::mem::ScratchGit;

    fn sample(at_ms: u64, paging_kb_s: u32, slot_held: bool) -> Sample {
        Sample {
            at_ms,
            avail_mb: 1_500,
            headroom_mb: 4_000,
            paging_kb_s,
            slot_held,
            src: Source::Guard,
        }
    }

    #[test]
    fn the_baseline_report_leaves_out_slot_held_samples() {
        let samples: Vec<Sample> = (0..120)
            .map(|i| sample(1_793_000_000_000 + i * 5_000, 1_000, false))
            .chain((0..10).map(|i| sample(1_793_000_700_000 + i * 5_000, 90_000, true)))
            .collect();
        let report = baseline_report(&samples, 1);
        assert!(report.contains("120 samples in 1 separate hours"), "{report}");
        assert!(report.contains("10 left out"), "{report}");
        assert!(
            report.contains("| Paging, 30 s average (KB/s) | 1000 | 1000 | 1000 | 1000 | 1000 |"),
            "the held samples' 90,000 KB/s is not in the percentiles: {report}"
        );
    }

    #[test]
    fn the_report_replaces_only_what_is_between_the_markers() {
        let text = "intro\n<!-- baseline:start -->\nold\n<!-- baseline:end -->\nafter\n";
        let replaced = replace_between(text, BASELINE_START, BASELINE_END, "new").unwrap();
        assert_eq!(
            replaced, "intro\n<!-- baseline:start -->\nnew\n<!-- baseline:end -->\nafter\n",
            "the block and nothing else"
        );
        assert_eq!(replace_between("no markers", BASELINE_START, BASELINE_END, "x"), None, "none");
    }

    #[test]
    fn the_wp0_file_has_the_baseline_markers() {
        let text = fs::read_to_string(crate::repo_root().join(WP0_FILE)).unwrap();
        assert!(
            replace_between(&text, BASELINE_START, BASELINE_END, "x").is_some(),
            "slots --baseline writes between the markers in {WP0_FILE}"
        );
    }

    #[test]
    fn a_verified_archive_gives_the_three_variables() {
        let scratch = ScratchGit::new("native-env");
        let root = scratch.path();
        let archive = NativeArchive {
            file: "sherpa-onnx-v9-test.tar.bz2",
            url: "https://example.invalid/archive",
            size: 3,
            sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            folder: "sherpa-onnx-v9-test",
        };
        assert_eq!(env_for(root, &archive), None, "nothing until it is verified and unpacked");
        scratch.write(archive.file, "abc");
        assert!(verify(&root.join(archive.file), &archive).unwrap(), "the size and hash match");
        scratch.write(&format!("{}{VERIFIED_SUFFIX}", archive.file), archive.sha256);
        scratch.write(&format!("{}/lib/onnxruntime.dll", archive.folder), "dll");
        let env = env_for(root, &archive).unwrap();
        let names: Vec<&str> = env.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(
            names,
            ["SHERPA_ONNX_ARCHIVE_DIR", "ORT_LIB_LOCATION", "ORT_PREFER_DYNAMIC_LINK"],
            "the variables section 4.1 names"
        );
        scratch.write(archive.file, "abd");
        assert!(!verify(&root.join(archive.file), &archive).unwrap(), "a changed byte fails");
    }

    #[test]
    fn what_an_older_pin_left_is_stale_and_the_current_pin_is_kept() {
        let scratch = ScratchGit::new("native-stale");
        let current = SHERPA_ONNX;
        scratch.write(current.file, "x");
        scratch.write(&format!("{}/lib/a.dll", current.folder), "x");
        scratch.write("sherpa-onnx-v1.13.7-win-x64-shared-MT-Release-lib/lib/a.dll", "old");
        scratch.write("sherpa-onnx-v1.13.7-win-x64-shared-MT-Release-lib.tar.bz2", "old");
        scratch.write("buildguard.toml", "not native");
        let stale = stale_entries(scratch.path(), &current).unwrap();
        let names: Vec<String> =
            stale.iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(
            names,
            [
                "sherpa-onnx-v1.13.7-win-x64-shared-MT-Release-lib",
                "sherpa-onnx-v1.13.7-win-x64-shared-MT-Release-lib.tar.bz2"
            ],
            "only the older pin's folder and archive"
        );
    }

    #[test]
    fn the_pinned_archive_is_section_4_1s() {
        assert_eq!(SHERPA_ONNX.size, 8_032_957, "the size section 4.1 gives");
        assert_eq!(
            SHERPA_ONNX.sha256, "b8eedf41bd6d3779218887b48367bb7a3ece5aaa7667f01f69ee823a12b0a9e7",
            "the hash section 4.1 gives"
        );
        assert!(SHERPA_ONNX.url.ends_with(SHERPA_ONNX.file), "the URL names the file");
        assert!(SHERPA_ONNX.file.starts_with(SHERPA_ONNX.folder), "it unpacks to its folder");
    }

    #[test]
    fn lock_reads_the_third_party_packages() {
        let lockfile = r#"
version = 4

[[package]]
name = "eileen-core"
version = "0.0.0"

[[package]]
name = "serde"
version = "1.0.229"
source = "registry+https://github.com/rust-lang/crates.io-index"
"#;
        assert_eq!(
            third_party(lockfile).unwrap(),
            [("serde".to_owned(), "1.0.229".to_owned())],
            "workspace members have no source and are not third-party"
        );
        assert!(third_party("not = [toml").is_err(), "a broken lockfile is an error");
    }
}
