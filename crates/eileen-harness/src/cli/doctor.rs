//! `eileen doctor claude` (BUILD.md sections 4.2 and 6.5): which CLI Eileen would run and how
//! it was installed, its version against the D77 floor and the tested release, the versions
//! running sessions use, whether it is signed in, and the toolchains a worker would see.
//!
//! The sign-in check is `claude auth status`, read by its exit code only (D4): its output names
//! the account, so it goes nowhere. When the CLI is signed out the report asks the user to run
//! `claude auth login` and nothing else starts; Eileen never runs a login. Nothing here opens a
//! credential file or anything under a `.claude` folder. Every probe starts through
//! `eileen_supervisor`, in the worker environment, with a deadline.

use std::collections::BTreeSet;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use eileen_supervisor::{ChildClass, ChildSpec};
use tokio::io::AsyncReadExt;

use crate::locate::{
    ClaudeInstall, ClaudeVersion, InstallKind, LocateEnv, LocateError, MIN_CLI, TESTED_CLI,
    locate_claude,
};
use crate::session::{is_denylisted, under_claude_folder};

/// How long `claude auth status` may take. A cold start of the CLI takes about 1.5 s on the
/// owner's PC (the prior-art report), and much longer under load.
pub const AUTH_STATUS_DEADLINE: Duration = Duration::from_secs(20);

/// How long `claude --version` may take.
pub const VERSION_DEADLINE: Duration = Duration::from_secs(20);

/// How long vswhere may take.
pub const VSWHERE_DEADLINE: Duration = Duration::from_secs(15);

/// The exit code of `claude auth status` when no account is signed in.
pub const SIGNED_OUT_EXIT: i32 = 1;

/// The most bytes of a probe's standard output kept; the rest is read and dropped.
const MAX_STDOUT: usize = 4 * 1024;

/// The vswhere component that holds the MSVC compiler and linker.
const MSVC_COMPONENT: &str = "Microsoft.VisualStudio.Component.VC.Tools.x86.x64";

/// What the doctor reads from the user's machine. The CLI fills it from the process
/// ([`DoctorInputs::from_process`]); tests build it.
#[derive(Clone, PartialEq, Eq)]
pub struct DoctorInputs {
    /// Where to look for the CLI.
    pub locate: LocateEnv,
    /// The user's environment, from which each probe's is built by the worker denylist.
    pub host_vars: Vec<(String, String)>,
    /// `vswhere.exe` in the Visual Studio Installer folder, on Windows.
    pub vswhere: Option<PathBuf>,
    /// The CLI versions the running Eileen's Claude sessions report, or `None` when no running
    /// Eileen was asked.
    pub running: Option<Vec<ClaudeVersion>>,
}

/// Names only: the values can hold the user's secrets.
impl fmt::Debug for DoctorInputs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DoctorInputs")
            .field("locate", &self.locate)
            .field("host_vars", &self.host_vars.iter().map(|(name, _)| name).collect::<Vec<_>>())
            .field("vswhere", &self.vswhere)
            .field("running", &self.running)
            .finish()
    }
}

impl DoctorInputs {
    /// Reads the process's environment: `PATH` (which the worker denylist keeps), `%APPDATA%`,
    /// the home folder and the Visual Studio Installer folder. No running Eileen is asked yet:
    /// the status endpoint that reports the running sessions' versions is WP2's.
    #[must_use]
    pub fn from_process() -> Self {
        let windows = cfg!(windows);
        let host_vars: Vec<(String, String)> = std::env::vars_os()
            .filter_map(|(name, value)| Some((name.into_string().ok()?, value.into_string().ok()?)))
            .collect();
        let get = |wanted: &str| {
            host_vars
                .iter()
                .find(
                    |(name, _)| {
                        if windows { name.eq_ignore_ascii_case(wanted) } else { name == wanted }
                    },
                )
                .map(|(_, value)| PathBuf::from(value))
        };
        let path_dirs = get("PATH").map(|path| std::env::split_paths(&path).collect());
        let vswhere = get("ProgramFiles(x86)").filter(|_| windows).map(|programs| {
            programs.join("Microsoft Visual Studio").join("Installer").join("vswhere.exe")
        });
        let locate = LocateEnv {
            path_dirs: path_dirs.unwrap_or_default(),
            appdata: get("APPDATA"),
            home: get(if windows { "USERPROFILE" } else { "HOME" }),
            windows,
        };
        Self { locate, host_vars, vswhere, running: None }
    }
}

/// One process the doctor starts, and how long it may take.
#[derive(Debug)]
pub struct Probe {
    /// What to start.
    pub spec: ChildSpec,
    /// How long it may take, its output read and its exit included.
    pub deadline: Duration,
}

/// What a probe answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeOutput {
    /// Its exit code, `None` when a signal ended it.
    pub exit_code: Option<i32>,
    /// Its standard output, at most 4 KB of it, when the spec piped it; otherwise empty.
    pub stdout: String,
}

/// Why a probe gave no answer.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProbeError {
    /// It could not be started.
    #[error("it could not start: {0}")]
    Spawn(String),
    /// It gave no answer before its deadline, and was killed.
    #[error("no answer within {} s", .0.as_secs())]
    DeadlinePassed(Duration),
    /// Reading its output or waiting for it failed.
    #[error("waiting for it failed: {0}")]
    Wait(String),
}

/// Starts the doctor's probes. [`SupervisedRunner`] starts them through `eileen_supervisor`;
/// tests answer from a script.
pub trait ProbeRunner {
    /// Runs `probe` to its exit or its deadline.
    fn run(&mut self, probe: &Probe) -> Result<ProbeOutput, ProbeError>;
}

/// Runs each probe through `eileen_supervisor` on a tokio runtime the caller owns: in a
/// kill-on-close Job Object, killed when its deadline passes.
#[derive(Debug)]
pub struct SupervisedRunner<'rt> {
    runtime: &'rt tokio::runtime::Runtime,
}

impl<'rt> SupervisedRunner<'rt> {
    /// A runner on `runtime`, which needs I/O and time enabled (`eileen_rt::runtime::build`).
    #[must_use]
    pub const fn new(runtime: &'rt tokio::runtime::Runtime) -> Self {
        Self { runtime }
    }
}

impl ProbeRunner for SupervisedRunner<'_> {
    fn run(&mut self, probe: &Probe) -> Result<ProbeOutput, ProbeError> {
        self.runtime.block_on(run_supervised(probe))
    }
}

async fn run_supervised(probe: &Probe) -> Result<ProbeOutput, ProbeError> {
    let mut child = eileen_supervisor::spawn(&probe.spec)
        .map_err(|error| ProbeError::Spawn(error.to_string()))?;
    drop(child.take_stdin());
    let stdout = child.take_stdout();
    let answered = tokio::time::timeout(probe.deadline, async {
        let mut kept = Vec::new();
        if let Some(mut stdout) = stdout {
            let mut chunk = [0_u8; 1024];
            loop {
                let read = stdout.read(&mut chunk).await?;
                if read == 0 {
                    break;
                }
                let room = MAX_STDOUT.saturating_sub(kept.len());
                kept.extend_from_slice(chunk.get(..read.min(room)).unwrap_or_default());
            }
        }
        let status = child.wait().await?;
        Ok::<_, io::Error>((status, kept))
    })
    .await;
    match answered {
        Ok(Ok((status, kept))) => Ok(ProbeOutput {
            exit_code: status.code(),
            stdout: String::from_utf8_lossy(&kept).into_owned(),
        }),
        Ok(Err(error)) => Err(ProbeError::Wait(error.to_string())),
        // Dropping the child kills its Job Object, and with it everything it started.
        Err(_) => {
            drop(child);
            Err(ProbeError::DeadlinePassed(probe.deadline))
        }
    }
}

/// The version against the floor and the tested release.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionCheck {
    /// From [`MIN_CLI`] to [`TESTED_CLI`].
    InRange(ClaudeVersion),
    /// Newer than [`TESTED_CLI`]: allowed, and checked by the nightly drift job.
    NewerThanTested(ClaudeVersion),
    /// Older than [`MIN_CLI`]: Eileen does not run it.
    BelowFloor(ClaudeVersion),
    /// Neither the layout nor the CLI said.
    Unknown,
}

impl VersionCheck {
    fn of(version: Option<ClaudeVersion>) -> Self {
        match version {
            None => Self::Unknown,
            Some(version) if version < MIN_CLI => Self::BelowFloor(version),
            Some(version) if version > TESTED_CLI => Self::NewerThanTested(version),
            Some(version) => Self::InRange(version),
        }
    }

    const fn version(self) -> Option<ClaudeVersion> {
        match self {
            Self::InRange(version) | Self::NewerThanTested(version) | Self::BelowFloor(version) => {
                Some(version)
            }
            Self::Unknown => None,
        }
    }
}

/// What `claude auth status` answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthCheck {
    /// Exit code 0.
    SignedIn,
    /// Exit code [`SIGNED_OUT_EXIT`].
    SignedOut,
    /// Another exit code, or none.
    Unclear {
        /// The exit code, `None` when a signal ended it.
        exit_code: Option<i32>,
    },
    /// No answer at all.
    NoAnswer(ProbeError),
}

/// The running sessions' versions against the installed one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunningCheck {
    /// No running Eileen was asked.
    NotKnown,
    /// The running Eileen has no Claude session.
    NoneRunning,
    /// Every running session uses the installed version.
    AllInstalled,
    /// These versions, each once, differ from the installed one.
    Differ(Vec<ClaudeVersion>),
}

/// The MSVC toolchain, through vswhere.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MsvcCheck {
    /// The Visual Studio installation that holds the C++ build tools.
    Installed(PathBuf),
    /// vswhere found no installation with the C++ build tools.
    NoCppTools,
    /// vswhere is not installed.
    NoVswhere,
    /// vswhere failed.
    Failed(String),
    /// Not a Windows host.
    NotWindows,
}

/// The toolchains a worker would find on its `PATH`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Toolchains {
    /// `cargo`, if a worker would find it.
    pub cargo: Option<PathBuf>,
    /// `node`, if a worker would find it.
    pub node: Option<PathBuf>,
    /// The MSVC toolchain.
    pub msvc: MsvcCheck,
}

/// What `eileen doctor claude` found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoctorReport {
    /// The CLI Eileen would run.
    pub install: ClaudeInstall,
    /// Its version.
    pub version: VersionCheck,
    /// Whether it is signed in.
    pub auth: AuthCheck,
    /// The running sessions' versions.
    pub running: RunningCheck,
    /// The toolchains, checked only once the CLI is signed in.
    pub toolchains: Option<Toolchains>,
}

impl DoctorReport {
    /// Whether Eileen can run this CLI: signed in, at or above the floor.
    #[must_use]
    pub fn healthy(&self) -> bool {
        self.auth == AuthCheck::SignedIn
            && matches!(self.version, VersionCheck::InRange(_) | VersionCheck::NewerThanTested(_))
    }
}

/// Runs the checks: finds the CLI, asks `claude auth status`, and only when it is signed in
/// asks its version (when the layout gave none) and looks for the toolchains.
pub fn doctor(
    inputs: &DoctorInputs,
    runner: &mut dyn ProbeRunner,
) -> Result<DoctorReport, LocateError> {
    let install = locate_claude(&inputs.locate)?;
    let env: Vec<(String, String)> =
        inputs.host_vars.iter().filter(|(name, _)| !is_denylisted(name)).cloned().collect();
    let auth = auth_status(&install.exe, &env, runner);
    let signed_in = auth == AuthCheck::SignedIn;
    let version = match install.version {
        None if signed_in => ask_version(&install.exe, &env, runner),
        known => known,
    };
    let toolchains = signed_in.then(|| find_toolchains(inputs, &env, runner));
    let version = VersionCheck::of(version);
    let running = running_check(inputs.running.as_deref(), version.version());
    Ok(DoctorReport { install, version, auth, running, toolchains })
}

fn claude_probe(exe: &Path, args: &[&str], env: &[(String, String)], deadline: Duration) -> Probe {
    let mut spec = ChildSpec::new("claude.doctor", exe)
        .args(args)
        .envs(env.iter().cloned())
        .class(ChildClass::ClaudeSession);
    spec.env_clear = true;
    spec.piped = false;
    Probe { spec, deadline }
}

fn auth_status(exe: &Path, env: &[(String, String)], runner: &mut dyn ProbeRunner) -> AuthCheck {
    match runner.run(&claude_probe(exe, &["auth", "status"], env, AUTH_STATUS_DEADLINE)) {
        Ok(ProbeOutput { exit_code: Some(0), .. }) => AuthCheck::SignedIn,
        Ok(ProbeOutput { exit_code: Some(SIGNED_OUT_EXIT), .. }) => AuthCheck::SignedOut,
        Ok(ProbeOutput { exit_code, .. }) => AuthCheck::Unclear { exit_code },
        Err(error) => AuthCheck::NoAnswer(error),
    }
}

fn ask_version(
    exe: &Path,
    env: &[(String, String)],
    runner: &mut dyn ProbeRunner,
) -> Option<ClaudeVersion> {
    let mut probe = claude_probe(exe, &["--version"], env, VERSION_DEADLINE);
    probe.spec.piped = true;
    let output = runner.run(&probe).ok().filter(|output| output.exit_code == Some(0))?;
    ClaudeVersion::parse_version_output(&output.stdout)
}

fn find_toolchains(
    inputs: &DoctorInputs,
    env: &[(String, String)],
    runner: &mut dyn ProbeRunner,
) -> Toolchains {
    let windows = inputs.locate.windows;
    let dirs = &inputs.locate.path_dirs;
    let msvc = match &inputs.vswhere {
        _ if !windows => MsvcCheck::NotWindows,
        Some(vswhere) if vswhere.is_file() => ask_vswhere(vswhere, env, runner),
        Some(_) | None => MsvcCheck::NoVswhere,
    };
    Toolchains {
        cargo: on_path(dirs, "cargo", windows),
        node: on_path(dirs, "node", windows),
        msvc,
    }
}

fn ask_vswhere(
    vswhere: &Path,
    env: &[(String, String)],
    runner: &mut dyn ProbeRunner,
) -> MsvcCheck {
    let args = [
        "-latest",
        "-products",
        "*",
        "-requires",
        MSVC_COMPONENT,
        "-property",
        "installationPath",
        "-nologo",
        "-utf8",
    ];
    let mut spec = ChildSpec::new("vswhere", vswhere)
        .args(args)
        .envs(env.iter().cloned())
        .class(ChildClass::Tool);
    spec.env_clear = true;
    match runner.run(&Probe { spec, deadline: VSWHERE_DEADLINE }) {
        Ok(ProbeOutput { exit_code: Some(0), stdout }) => {
            match stdout.lines().map(str::trim).find(|line| !line.is_empty()) {
                Some(path) => MsvcCheck::Installed(PathBuf::from(path)),
                None => MsvcCheck::NoCppTools,
            }
        }
        Ok(ProbeOutput { exit_code, .. }) => MsvcCheck::Failed(match exit_code {
            Some(code) => format!("exit code {code}"),
            None => "ended by a signal".to_owned(),
        }),
        Err(error) => MsvcCheck::Failed(error.to_string()),
    }
}

/// The first `name` a worker would run from `dirs`: `name.exe` or `name.cmd` on Windows.
fn on_path(dirs: &[PathBuf], name: &str, windows: bool) -> Option<PathBuf> {
    let suffixes: &[&str] = if windows { &[".exe", ".cmd"] } else { &[""] };
    dirs.iter()
        .filter(|dir| !under_claude_folder(dir))
        .flat_map(|dir| suffixes.iter().map(move |suffix| dir.join(format!("{name}{suffix}"))))
        .find(|path| path.is_file())
}

fn running_check(
    running: Option<&[ClaudeVersion]>,
    installed: Option<ClaudeVersion>,
) -> RunningCheck {
    match running {
        None => RunningCheck::NotKnown,
        Some([]) => RunningCheck::NoneRunning,
        Some(versions) => {
            let differ: BTreeSet<ClaudeVersion> =
                versions.iter().copied().filter(|version| Some(*version) != installed).collect();
            if differ.is_empty() {
                RunningCheck::AllInstalled
            } else {
                RunningCheck::Differ(differ.into_iter().collect())
            }
        }
    }
}

impl fmt::Display for DoctorReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Claude CLI")?;
        match &self.install.kind {
            InstallKind::NpmShim { shim } => writeln!(f, "  npm shim:    {}", shim.display())?,
            InstallKind::Native => writeln!(f, "  installed:   by the native installer")?,
            InstallKind::Other => writeln!(f, "  installed:   not by npm or the native installer")?,
        }
        writeln!(f, "  executable:  {}", self.install.exe.display())?;
        for refusal in &self.install.skipped {
            writeln!(f, "  passed over: {} ({})", refusal.path.display(), refusal.reason)?;
        }
        writeln!(f, "  version:     {}", self.version)?;
        writeln!(f, "  running:     {}", self.running)?;
        writeln!(f, "  signed in:   {}", self.auth)?;
        match &self.toolchains {
            Some(toolchains) => write!(f, "{toolchains}"),
            None => {
                writeln!(f, "The toolchains a worker sees are checked once the CLI is signed in.")
            }
        }
    }
}

impl fmt::Display for VersionCheck {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InRange(version) => {
                write!(f, "{version}, within the tested range ({MIN_CLI} to {TESTED_CLI})")
            }
            Self::NewerThanTested(version) => write!(
                f,
                "{version}, newer than the tested {TESTED_CLI}; the nightly drift job checks each \
                 release"
            ),
            Self::BelowFloor(version) => write!(
                f,
                "{version}, below the floor of {MIN_CLI} that Eileen needs. Update the CLI, then \
                 run `eileen doctor claude` again."
            ),
            Self::Unknown => f.write_str("not known"),
        }
    }
}

impl fmt::Display for RunningCheck {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotKnown => f.write_str("not checked (needs a running Eileen)"),
            Self::NoneRunning => f.write_str("no Claude sessions"),
            Self::AllInstalled => f.write_str("every session on the installed version"),
            Self::Differ(versions) => {
                let list: Vec<String> = versions.iter().map(ToString::to_string).collect();
                write!(
                    f,
                    "sessions on {} differ from the installed version; `eileen claude \
                     update-window` restarts them",
                    list.join(", ")
                )
            }
        }
    }
}

impl fmt::Display for AuthCheck {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SignedIn => f.write_str("yes"),
            Self::SignedOut => f.write_str(
                "no. Please run `claude auth login` yourself, then run `eileen doctor claude` \
                 again. Eileen never runs the login.",
            ),
            Self::Unclear { exit_code: Some(code) } => {
                write!(f, "not clear: `claude auth status` exited with code {code}")
            }
            Self::Unclear { exit_code: None } => {
                f.write_str("not clear: `claude auth status` was ended by a signal")
            }
            Self::NoAnswer(ProbeError::DeadlinePassed(deadline)) => write!(
                f,
                "not clear: `claude auth status` gave no answer within {} s",
                deadline.as_secs()
            ),
            Self::NoAnswer(error) => write!(f, "not clear: `claude auth status` failed: {error}"),
        }
    }
}

impl fmt::Display for Toolchains {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Toolchains a worker sees")?;
        for (label, found) in [("cargo:      ", &self.cargo), ("node:       ", &self.node)] {
            match found {
                Some(path) => writeln!(f, "  {label} {}", path.display())?,
                None => writeln!(f, "  {label} not on the worker's PATH")?,
            }
        }
        match &self.msvc {
            MsvcCheck::Installed(path) => {
                writeln!(f, "  MSVC:        {} (from vswhere)", path.display())
            }
            MsvcCheck::NoCppTools => {
                writeln!(
                    f,
                    "  MSVC:        vswhere found no Visual Studio with the C++ build tools"
                )
            }
            MsvcCheck::NoVswhere => writeln!(f, "  MSVC:        vswhere is not installed"),
            MsvcCheck::Failed(reason) => writeln!(f, "  MSVC:        vswhere failed: {reason}"),
            MsvcCheck::NotWindows => Ok(()),
        }
    }
}

/// What the doctor prints when no CLI could be used: every folder searched, or every
/// candidate passed over and why.
#[derive(Debug, Clone, Copy)]
pub struct LocateReport<'a>(pub &'a LocateError);

impl fmt::Display for LocateReport<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Claude CLI")?;
        match self.0 {
            LocateError::NotFound { searched } => {
                writeln!(f, "  not found. Searched:")?;
                for dir in searched {
                    writeln!(f, "    {}", dir.display())?;
                }
            }
            LocateError::NoUsableCli { refused } => {
                writeln!(f, "  none usable. Passed over:")?;
                for refusal in refused {
                    writeln!(f, "    {} ({})", refusal.path.display(), refusal.reason)?;
                }
            }
        }
        writeln!(
            f,
            "Install Claude Code with npm or its native installer, sign in by running `claude auth \
             login` yourself, then run `eileen doctor claude` again."
        )
    }
}

#[cfg(test)]
mod tests;
