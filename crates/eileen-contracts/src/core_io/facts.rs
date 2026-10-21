//! The facts the runtime adds to a call before the core sees it (paths, git repositories and
//! URLs), and the URL rule's inputs and answers (section 2.12).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::ids::Personal;
use crate::schema::Registry;

/// Built by the runtime for each path argument of a file tool, of a shell command (from the
/// tokenizer's path positions; `access` says whether the command reads or writes it) and of an
/// Eileen tool field marked `x-eileen-path`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PathFact {
    /// The argument it came from.
    pub arg: String,
    /// Its class, most specific first.
    pub class: PathClass,
    /// How the call uses it.
    pub access: PathAccess,
    /// The canonical path.
    pub canonical: Personal<String>,
    /// An executable, script, installer or a shortcut to one.
    pub runs_code: bool,
    /// Its registered handler is a web browser, or it is a `.url` or `.website` file.
    pub opens_web: bool,
    /// A component is a git directory or pointer file by the `protectNTFS` rule of section
    /// 2.12; outside `GitInternals` it keeps its class and raises a write to tier 2, HUD only.
    pub git_component: bool,
}

/// The repository a git command will use (section 2.12, "Git runs code too"), found by the
/// runtime by walking up from the command's working folder to the first `.git` entry, never by
/// running git.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct GitFact {
    /// The index of the git command within the command line.
    pub command: u16,
    /// The git directory.
    pub git_dir: Personal<String>,
    /// Whether it is the job's own repository.
    pub job_repo: bool,
    /// Whether it has hooks.
    pub has_hooks: bool,
}

/// Built by the runtime for each URL in a call, the way it builds `PathFact`s, so the URL rule
/// needs no IO.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct UrlFact {
    /// The argument it came from.
    pub arg: String,
    /// The URL.
    pub url: Personal<String>,
    /// Its host, when it parsed.
    pub host: Option<String>,
    /// Verbatim in the media history or the recent-work index (rule 1).
    pub in_index: bool,
    /// The full URL, path and query included, is in the media history or the ledger's browser
    /// lines (rule 3).
    pub visited: bool,
}

/// What the core supplies from its own state for each URL check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct UrlCtx {
    /// The core's own code built the URL: a grammar entry or resolver, the video URL builder, a
    /// replay step's pointer into an `apps_recent` url.
    pub built_by_eileen: bool,
    /// The authorizing turn's normalised final transcript.
    pub transcript: Personal<String>,
    /// The job's dispatched task, subtasks and subquestions, once it passed.
    pub task: Option<Personal<String>>,
    /// The string arguments of the job's untainted steps.
    pub untainted_args: Vec<Personal<String>>,
    /// Hosts whose pages are in the deciding context (section 2.12).
    pub context_hosts: Vec<String>,
    /// False when a browser window there had no readable address bar.
    pub context_complete: bool,
}

/// The URL rule's answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UrlVerdict {
    /// The URL may be opened, by this rule.
    Vouched(VouchRule),
    /// The URL is not vouched for, for this reason.
    NotVouched(UrlRefusal),
}

/// Why a URL is vouched for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum VouchRule {
    /// Eileen's own code built it.
    BuiltByEileen,
    /// It is in the media history or the recent-work index.
    InIndex,
    /// The user named its host.
    NamedHost,
    /// The user visited it.
    Visited,
}

/// Why a URL is not vouched for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UrlRefusal {
    /// It did not parse.
    Unparsed,
    /// The user did not name its host.
    HostNotNamed,
    /// It does not appear verbatim where it must.
    NotVerbatim,
    /// Only its host was visited, not the URL.
    VisitedHostOnly,
    /// Its host is a page in the deciding context.
    ContextHost,
    /// The deciding context is incomplete.
    ContextIncomplete,
}

/// The class of a path, tried in this order, first match wins (section 2.12): `Credential`,
/// `BrowserProfile`, `GitInternals`, `SkillSnapshot`, `JobFolder`, `JobTemp`, `EileenData`,
/// `Toolchain`, `Elsewhere`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum PathClass {
    /// The job's own folders.
    JobFolder,
    /// The job's temporary folder.
    JobTemp,
    /// A `.git` path component inside the job's own folders, and the job's own git and common
    /// directories.
    GitInternals,
    /// The session's learned-skill snapshot.
    SkillSnapshot,
    /// A toolchain folder.
    Toolchain,
    /// Eileen's own data folder.
    EileenData,
    /// A credential file or folder.
    Credential,
    /// A browser profile.
    BrowserProfile,
    /// Anywhere else.
    Elsewhere,
    /// A glob or an unresolved variable: the command is unparseable.
    Unknown,
}

/// How a call uses a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PathAccess {
    /// It reads the path.
    Read,
    /// It writes the path.
    Write,
    /// It opens the path with its handler.
    Open,
}

pub(super) fn register(r: &mut Registry) {
    r.add::<PathFact>();
    r.add::<GitFact>();
    r.add::<UrlFact>();
    r.add::<UrlCtx>();
    r.add::<UrlVerdict>();
    r.add::<VouchRule>();
    r.add::<UrlRefusal>();
    r.add::<PathClass>();
    r.add::<PathAccess>();
}
