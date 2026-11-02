//! The job lifecycle (BUILD.md section 2.5): the states a job passes through and the one graph
//! of steps between them, the name, slug and fan-out ordinal a job gets at accept, the approval
//! deadline, the jobs a cancel can name and which job a request names (section 2.5's pronoun
//! rules), the focus job, what a core restart does to unfinished jobs, and where a coding job
//! works. [`Registry`] is pure and cloneable: `Core::step` keeps one, and
//! `tests/it/job_model.rs` checks it against a reference model on random command sequences.

use std::collections::{BTreeMap, VecDeque};
use std::fmt;

use eileen_contracts::harness::WorktreeSpec;
use eileen_contracts::ids::{JobId, Personal, WallTime};
use eileen_contracts::jobs::{Job, JobKind, JobStatus, Route};
use eileen_contracts::tools::brain::JobRef;

/// The most jobs the registry keeps. The oldest finished job goes to make room, and while every
/// kept job is unfinished a new one is refused.
pub const KEPT: usize = 32;

/// The longest spoken name, in words (section 2.5: two to four).
pub const NAME_WORDS: usize = 4;

/// How far back a new slug is checked for a collision, in microseconds of wall time: seven days
/// (section 2.5).
pub const SLUG_WINDOW_US: i64 = 7 * 86_400_000_000;

/// The longest slug before a collision suffix, in bytes: it names a folder and a branch.
pub const SLUG_MAX: usize = 40;

/// The most slugs the registry remembers beside its kept jobs for the seven-day check. Past
/// this many in seven days, the oldest is forgotten first.
pub const SLUGS_KEPT: usize = 1024;

/// A fan-out child's spoken ordinal, in accept order (section 3.3.2: `fanout` is 1 to 8).
const ORDINALS: [&str; 8] = ["one", "two", "three", "four", "five", "six", "seven", "eight"];

/// Words a slug does not start with.
const ARTICLES: [&str; 3] = ["the", "a", "an"];

/// A change to a job's state. Each lands in one state and leaves only the states
/// [`Step::leaves`] names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// A worker or a replay took the queued job: `running`.
    Start,
    /// The running job waits on something outside its worker: `blocked`.
    Block,
    /// The running job asks the user, who has until the deadline: `awaiting_approval`.
    AwaitApproval {
        /// When the question is declined (`approvals.deadline_min` from the asking).
        deadline: WallTime,
    },
    /// Back to work after a block, an answer, or the decline at the deadline: `running`.
    Resume,
    /// Its worker finished: `completed`.
    Complete,
    /// It cannot go on: `failed`.
    Fail,
    /// The user stopped it: `cancelled`.
    Cancel,
    /// A core restart found it unfinished: `interrupted`.
    Interrupt,
}

impl Step {
    /// The state the step lands in.
    #[must_use]
    pub const fn status(self) -> JobStatus {
        match self {
            Self::Start | Self::Resume => JobStatus::Running,
            Self::Block => JobStatus::Blocked,
            Self::AwaitApproval { .. } => JobStatus::AwaitingApproval,
            Self::Complete => JobStatus::Completed,
            Self::Fail => JobStatus::Failed,
            Self::Cancel => JobStatus::Cancelled,
            Self::Interrupt => JobStatus::Interrupted,
        }
    }

    /// Whether the step may leave `from`. A finished job takes no step, so every job ends in
    /// exactly one terminal state.
    #[must_use]
    pub const fn leaves(self, from: JobStatus) -> bool {
        match self {
            Self::Start => matches!(from, JobStatus::Queued),
            Self::Block | Self::AwaitApproval { .. } | Self::Complete => {
                matches!(from, JobStatus::Running)
            }
            Self::Resume => matches!(from, JobStatus::Blocked | JobStatus::AwaitingApproval),
            Self::Fail | Self::Cancel | Self::Interrupt => !from.is_terminal(),
        }
    }
}

/// Whether a job may go from `from` to `to` in one step: the edges [`Step`] takes, written as
/// pairs of states for checking a job's successive rows (invariant 5).
#[must_use]
pub const fn legal(from: JobStatus, to: JobStatus) -> bool {
    use JobStatus::{
        AwaitingApproval, Blocked, Cancelled, Completed, Failed, Interrupted, Queued, Running,
    };
    match from {
        Queued => matches!(to, Running | Failed | Cancelled | Interrupted),
        Running => {
            matches!(to, Blocked | AwaitingApproval | Completed | Failed | Cancelled | Interrupted)
        }
        Blocked | AwaitingApproval => matches!(to, Running | Failed | Cancelled | Interrupted),
        Completed | Failed | Cancelled | Interrupted => false,
    }
}

/// Why the registry refused a change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    /// No kept job has this id.
    Unknown(JobId),
    /// The step does not leave the job's state.
    Illegal {
        /// The job.
        job: JobId,
        /// Its state.
        from: JobStatus,
        /// The step refused.
        step: Step,
    },
    /// A kept job has this id already: job IDs never repeat.
    Duplicate(JobId),
    /// A fan-out child's parent is not a kept, unfinished job: a finished fan-out takes no
    /// new children.
    NoParent(JobId),
    /// Every kept job is unfinished.
    Full,
}

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unknown(job) => write!(f, "no kept job is {job}"),
            Self::Illegal { job, from, step } => {
                write!(f, "{job} cannot take the step {step:?} from {from:?}")
            }
            Self::Duplicate(job) => write!(f, "a kept job is {job} already"),
            Self::NoParent(job) => write!(f, "{job} is not a kept, unfinished fan-out"),
            Self::Full => write!(f, "all {KEPT} kept jobs are unfinished"),
        }
    }
}

impl std::error::Error for Refused {}

/// What a job is called: given once at accept and never regenerated after the user has heard
/// it (section 2.5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Naming {
    /// Its spoken name, two to four words.
    pub name: String,
    /// Its slug, which no job of the last seven days holds: none since the core started,
    /// and none of the jobs a snapshot kept before it.
    pub slug: String,
    /// Its ordinal, for a fan-out's child.
    pub ordinal: Option<u8>,
}

/// Where a job's session works (section 2.5, "One coding job edits in place"). The core does
/// not send it to the harness yet: until the harness records the snapshot and creates the
/// worktree, every job works in its session's own folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Checkout {
    /// In its session's own folder under the data directory: every job but a coding job whose
    /// repository is an absolute path.
    Scratch,
    /// In the user's checkout, once the harness has recorded a snapshot (`git stash create`
    /// and `HEAD`) under `snapshot_ref`, so "undo that" restores exactly what was there.
    InPlace {
        /// The repository, an absolute path as the brain gave it.
        repo: Personal<String>,
        /// The ref the snapshot is kept under: `refs/eileen/<job id>`.
        snapshot_ref: String,
    },
    /// In a worktree of its own, on a branch of its own.
    Worktree(WorktreeSpec),
}

/// The jobs the core keeps, in accept order.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Registry {
    jobs: Vec<Job>,
    /// How many children each kept fan-out parent has had. It never goes down while the parent
    /// is kept, so a child dropped to make room never frees its ordinal.
    children: BTreeMap<JobId, u8>,
    /// The slugs given since the registry started, oldest first, with when each was given, at
    /// most [`SLUGS_KEPT`] of the last seven days: a job dropped to make room still holds its
    /// slug, and with it its branch and its worktree folder.
    slugs: VecDeque<(WallTime, String)>,
}

impl Registry {
    /// A registry holding `jobs` as a snapshot kept them, unchanged until
    /// [`Registry::restart`]. The snapshot keeps no slugs beside its jobs, so a slug given to
    /// a job it no longer holds is free again after a restart.
    #[must_use]
    pub fn resumed(jobs: Vec<Job>) -> Self {
        let mut children = BTreeMap::new();
        for job in &jobs {
            let Some(parent) = job.spec.parent else { continue };
            if jobs.iter().any(|kept| kept.id == parent) {
                let seen: &mut u8 = children.entry(parent).or_default();
                *seen = seen.saturating_add(1).max(job.ordinal.unwrap_or(0));
            }
        }
        Self { jobs, children, slugs: VecDeque::new() }
    }

    /// Every kept job, in accept order.
    #[must_use]
    pub fn jobs(&self) -> &[Job] {
        &self.jobs
    }

    /// The kept job `id`.
    #[must_use]
    pub fn get(&self, id: JobId) -> Option<&Job> {
        self.jobs.iter().find(|job| job.id == id)
    }

    /// A core restart: every unfinished job is interrupted, since no worker or admission queue
    /// outlives the core that started it (invariant 12). Returns them in accept order.
    pub fn restart(&mut self, at: WallTime) -> Vec<JobId> {
        let mut interrupted = Vec::new();
        for job in self.jobs.iter_mut().filter(|job| !job.status.is_terminal()) {
            job.status = JobStatus::Interrupted;
            job.approval_deadline = None;
            job.updated = at;
            interrupted.push(job.id);
        }
        interrupted
    }

    /// The name, slug and ordinal of a new job of `kind`. A child of a kept, unfinished
    /// fan-out parent is "agent one" to "agent eight" in accept order, counting every child the
    /// parent has had, with the parent's slug and its ordinal; any other job, and a ninth
    /// child, is the brain's title hint cut to four words, or its kind's words. A slug that a
    /// job of the last seven days holds takes the first free suffix from `-2`.
    pub fn naming(
        &self,
        hint: Option<&str>,
        kind: JobKind,
        parent: Option<JobId>,
        at: WallTime,
    ) -> Result<Naming, Refused> {
        let child = match parent {
            None => None,
            Some(parent) => {
                let of = self.get(parent).filter(|job| !job.status.is_terminal());
                let of = of.ok_or(Refused::NoParent(parent))?;
                let ordinal = self.children.get(&parent).copied().unwrap_or(0).saturating_add(1);
                ORDINALS.get(usize::from(ordinal.saturating_sub(1))).map(|word| Naming {
                    name: format!("agent {word}"),
                    slug: format!("{}-{ordinal}", of.slug),
                    ordinal: Some(ordinal),
                })
            }
        };
        let Naming { name, slug, ordinal } = child.unwrap_or_else(|| {
            let name = name_of(hint, kind);
            let slug = slug_of(&name);
            let slug = if slug.is_empty() { slug_of(kind_words(kind)) } else { slug };
            Naming { name, slug, ordinal: None }
        });
        Ok(Naming { name, slug: self.free_slug(slug, at), ordinal })
    }

    /// `base`, or `base` with the first suffix from `-2` that no job of the last seven days
    /// holds, kept or remembered.
    fn free_slug(&self, base: String, at: WallTime) -> String {
        let since = at.0.saturating_sub(SLUG_WINDOW_US);
        let taken = |slug: &str| {
            self.jobs.iter().any(|job| job.created.0 >= since && job.slug == slug)
                || self.slugs.iter().any(|(given, theirs)| given.0 >= since && theirs == slug)
        };
        if !taken(&base) {
            return base;
        }
        // At most `KEPT` and `SLUGS_KEPT` slugs are taken, so one of these is free.
        let last = KEPT.saturating_add(SLUGS_KEPT).saturating_add(2);
        let free = (2..=last).map(|n| format!("{base}-{n}")).find(|s| !taken(s));
        free.unwrap_or(base)
    }

    /// Keeps a new job, `queued` and with no approval deadline, and remembers its slug. The
    /// oldest finished job goes when the registry is full. A fan-out child needs its parent
    /// kept and unfinished.
    pub fn accept(&mut self, mut job: Job) -> Result<(), Refused> {
        if self.get(job.id).is_some() {
            return Err(Refused::Duplicate(job.id));
        }
        if let Some(parent) = job.spec.parent
            && self.get(parent).is_none_or(|of| of.status.is_terminal())
        {
            return Err(Refused::NoParent(parent));
        }
        while self.jobs.len() >= KEPT {
            let oldest = self.jobs.iter().position(|kept| kept.status.is_terminal());
            let gone = self.jobs.remove(oldest.ok_or(Refused::Full)?);
            self.children.remove(&gone.id);
        }
        // The parent is unfinished, so the room made above never dropped it.
        if let Some(parent) = job.spec.parent {
            let seen: &mut u8 = self.children.entry(parent).or_default();
            *seen = seen.saturating_add(1);
        }
        let since = job.created.0.saturating_sub(SLUG_WINDOW_US);
        while self.slugs.len() >= SLUGS_KEPT
            || self.slugs.front().is_some_and(|(given, _)| given.0 < since)
        {
            self.slugs.pop_front();
        }
        self.slugs.push_back((job.created, job.slug.clone()));
        job.status = JobStatus::Queued;
        job.approval_deadline = None;
        self.jobs.push(job);
        Ok(())
    }

    /// Takes `step` for the job `id` at `at`, and returns the job as it now is. Only
    /// [`Step::AwaitApproval`] leaves an approval deadline.
    pub fn step(&mut self, id: JobId, step: Step, at: WallTime) -> Result<&Job, Refused> {
        let job = self.jobs.iter_mut().find(|job| job.id == id).ok_or(Refused::Unknown(id))?;
        if !step.leaves(job.status) {
            return Err(Refused::Illegal { job: id, from: job.status, step });
        }
        job.status = step.status();
        job.approval_deadline =
            if let Step::AwaitApproval { deadline } = step { Some(deadline) } else { None };
        job.updated = at;
        Ok(&*job)
    }

    /// The jobs awaiting an approval whose deadline is at or before `at`, in accept order: each
    /// is declined and resumes (section 2.5).
    #[must_use]
    pub fn expired(&self, at: WallTime) -> Vec<JobId> {
        let due = |job: &&Job| {
            job.status == JobStatus::AwaitingApproval
                && job.approval_deadline.is_some_and(|deadline| deadline <= at)
        };
        self.jobs.iter().filter(due).map(|job| job.id).collect()
    }

    /// The unfinished job `target` names, the newest first: by id, by slug, by spoken name in
    /// any case, or by a fan-out child's ordinal. A job with no ordinal is never "the first
    /// one".
    #[must_use]
    pub fn resolve(&self, target: &JobRef) -> Option<JobId> {
        let mut open = self.jobs.iter().rev().filter(|job| !job.status.is_terminal());
        let found = match target {
            JobRef::Id(id) => open.find(|job| job.id == *id),
            JobRef::Slug(slug) => open.find(|job| job.slug == *slug),
            JobRef::Name(name) => open.find(|job| job.name.eq_ignore_ascii_case(name.trim())),
            JobRef::Ordinal(ordinal) => open.find(|job| job.ordinal == Some(*ordinal)),
        };
        found.map(|job| job.id)
    }

    /// Which unfinished job a request names (section 2.5), first match wins: an explicit id,
    /// slug, ordinal or name (its words with the articles dropped, in any case); a kind word
    /// ("the render", "the coding job"); a route word ("Claude's job"); then, for a pronoun or
    /// no words at all, the job announced just before (`announced`, which the caller gives only
    /// right after an announcement), the focus job and the most recent job outside a fan-out.
    /// A name, kind word or route word that fits several jobs picks the focus job among them,
    /// or asks which. Kind and route words name only jobs outside a fan-out, whose children are
    /// named by ordinal.
    #[must_use]
    pub fn pick(&self, target: Option<&JobRef>, announced: Option<JobId>) -> Pick {
        let words = match target {
            None => Vec::new(),
            Some(JobRef::Name(name)) => plain_words(name),
            Some(other) => return self.resolve(other).map_or(Pick::Nothing, Pick::One),
        };
        let open = |job: &&Job| !job.status.is_terminal();
        let top = |job: &&Job| open(job) && job.spec.parent.is_none();
        if !words.is_empty() && !PRONOUNS.contains(&words.join(" ").as_str()) {
            let named: Vec<&Job> = self
                .jobs
                .iter()
                .rev()
                .filter(open)
                .filter(|j| plain_words(&j.name) == words)
                .collect();
            if !named.is_empty() {
                return choose(&named);
            }
            if let Some(kind) = kind_named(&words) {
                let found: Vec<&Job> =
                    self.jobs.iter().rev().filter(top).filter(|j| j.spec.kind == kind).collect();
                return choose(&found);
            }
            if let Some(route) = route_named(&words) {
                let found: Vec<&Job> =
                    self.jobs.iter().rev().filter(top).filter(|j| j.route == route).collect();
                return choose(&found);
            }
            return Pick::Nothing;
        }
        let announced = announced.filter(|id| self.get(*id).is_some_and(|job| open(&job)));
        let newest = || self.jobs.iter().rev().find(top).map(|job| job.id);
        announced.or_else(|| self.focused()).or_else(newest).map_or(Pick::Nothing, Pick::One)
    }

    /// The unfinished job the user is focused on, if any.
    #[must_use]
    pub fn focused(&self) -> Option<JobId> {
        let focus = self.jobs.iter().find(|job| job.focus && !job.status.is_terminal());
        focus.map(|job| job.id)
    }

    /// Moves the focus to `id`, an unfinished job, at `at`. Returns the jobs whose focus
    /// changed; nothing when `id` is not a kept, unfinished job.
    pub fn focus_on(&mut self, id: JobId, at: WallTime) -> Vec<JobId> {
        if self.get(id).is_none_or(|job| job.status.is_terminal()) {
            return Vec::new();
        }
        let mut changed = Vec::new();
        for job in &mut self.jobs {
            let focus = job.id == id;
            if job.focus != focus {
                job.focus = focus;
                job.updated = at;
                changed.push(job.id);
            }
        }
        changed
    }

    /// Once the focus job has finished, or no job has the focus: a finished job gives it up,
    /// and it moves to the newest unfinished job outside a fan-out, if any. Returns the jobs
    /// whose focus changed.
    pub fn refocus(&mut self, at: WallTime) -> Vec<JobId> {
        if self.focused().is_some() {
            return Vec::new();
        }
        let mut changed = Vec::new();
        for job in self.jobs.iter_mut().filter(|job| job.focus) {
            job.focus = false;
            job.updated = at;
            changed.push(job.id);
        }
        let next = self
            .jobs
            .iter()
            .rev()
            .find(|job| !job.status.is_terminal() && job.spec.parent.is_none());
        if let Some(next) = next.map(|job| job.id) {
            changed.extend(self.focus_on(next, at));
        }
        changed.sort_unstable();
        changed.dedup();
        changed
    }

    /// The name, slug and ordinal of a job that replaces `old` (a reroute): the name and
    /// ordinal the user knows, which are never regenerated, and a slug of its own.
    pub fn successor(&self, old: JobId, at: WallTime) -> Result<Naming, Refused> {
        let job = self.get(old).ok_or(Refused::Unknown(old))?;
        let slug = self.free_slug(job.slug.clone(), at);
        Ok(Naming { name: job.name.clone(), slug, ordinal: job.ordinal })
    }

    /// Records the unfinished job `id`'s progress, at most 100 percent; returns the job when
    /// the percent changed.
    pub fn progress(&mut self, id: JobId, pct: u8, at: WallTime) -> Option<&Job> {
        let job = self.jobs.iter_mut().find(|job| job.id == id && !job.status.is_terminal())?;
        let pct = pct.min(100);
        if job.progress_pct == Some(pct) {
            return None;
        }
        job.progress_pct = Some(pct);
        job.updated = at;
        Some(&*job)
    }

    /// Where `job` works. A coding job whose repository is an absolute path works in place,
    /// behind a snapshot, when it is not a fan-out's child, no other unfinished coding job
    /// works in that repository, and the user's uncommitted files do not overlap the files its
    /// task names (`overlap`, which the runtime reports); otherwise in a worktree under
    /// `worktrees/<repo hash>/<slug>` on the branch `eileen/<slug>`, from the user's current
    /// `HEAD`. Every other job works in its session's own folder, including one whose
    /// repository is a bare name or a relative path: the brain's text never picks a folder
    /// under Eileen's data, and a name is resolved from the utterance, the focused editor or
    /// a `project` fact first (section 2.5).
    #[must_use]
    pub fn checkout(&self, job: &Job, overlap: bool) -> Checkout {
        let coding = job.spec.kind == JobKind::Coding;
        let repo = job.spec.repo.as_ref().filter(|repo| coding && absolute(repo.get()));
        let Some(repo) = repo else {
            return Checkout::Scratch;
        };
        let key = repo_key(repo.get());
        let alongside = self.jobs.iter().any(|other| {
            other.id != job.id
                && other.spec.kind == JobKind::Coding
                && !other.status.is_terminal()
                && other.spec.repo.as_ref().is_some_and(|theirs| repo_key(theirs.get()) == key)
        });
        if alongside || overlap || job.spec.parent.is_some() {
            Checkout::Worktree(WorktreeSpec {
                repo: repo.clone(),
                path: Personal::new(format!("worktrees/{:016x}/{}", fnv1a(&key), job.slug)),
                branch: format!("eileen/{}", job.slug),
                base: "HEAD".to_owned(),
            })
        } else {
            Checkout::InPlace {
                repo: repo.clone(),
                snapshot_ref: format!("refs/eileen/{}", job.id),
            }
        }
    }
}

/// Which job a request names, from [`Registry::pick`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pick {
    /// This job.
    One(JobId),
    /// Several jobs fit and none is the focus job: Eileen asks which, once. Newest first.
    Ask(Vec<JobId>),
    /// No unfinished job fits.
    Nothing,
}

/// Words that name no job by themselves, once the articles and `FILLER` are dropped: the
/// pronoun rules decide.
const PRONOUNS: [&str; 5] = ["it", "that", "this", "that one", "this one"];

/// Words dropped before a name, kind word or route word is matched.
const FILLER: [&str; 5] = ["job", "jobs", "task", "please", "now"];

/// One job of `found` (newest first): the only one, or the focus job among several; otherwise
/// a question.
fn choose(found: &[&Job]) -> Pick {
    match found {
        [] => Pick::Nothing,
        [one] => Pick::One(one.id),
        many => many.iter().find(|job| job.focus).map_or_else(
            || Pick::Ask(many.iter().map(|job| job.id).collect()),
            |focus| Pick::One(focus.id),
        ),
    }
}

/// `text` in lower-case ASCII words, with a possessive "'s", the articles and `FILLER`
/// dropped: "Claude's job" is `["claude"]`, "The Render" is `["render"]`.
fn plain_words(text: &str) -> Vec<String> {
    let text = text.replace('\u{2019}', "'").to_lowercase();
    text.split(|c: char| !(c.is_alphanumeric() || c == '\''))
        .map(|word| word.strip_suffix("'s").unwrap_or(word).trim_matches('\'').to_owned())
        .filter(|word| !word.is_empty())
        .filter(|word| !ARTICLES.contains(&word.as_str()) && !FILLER.contains(&word.as_str()))
        .collect()
}

/// The job kind `words` name, when they are one of its kind words.
fn kind_named(words: &[String]) -> Option<JobKind> {
    let words: Vec<&str> = words.iter().map(String::as_str).collect();
    match words.as_slice() {
        ["render" | "renders" | "rendering" | "export"] => Some(JobKind::AppTask),
        ["coding" | "code" | "coder"] => Some(JobKind::Coding),
        ["research" | "researcher"] => Some(JobKind::Research),
        ["computer"] => Some(JobKind::ComputerUse),
        ["answer"] | ["deep", "answer"] => Some(JobKind::DeepAnswer),
        _ => None,
    }
}

/// The route `words` name, when they are its spoken name.
fn route_named(words: &[String]) -> Option<Route> {
    let said = words.join(" ");
    Route::ALL.into_iter().find(|route| route.display_name().to_lowercase() == said)
}

/// How a job of `kind` is spoken of when the brain gave no title.
const fn kind_words(kind: JobKind) -> &'static str {
    match kind {
        JobKind::Coding => "the coding job",
        JobKind::ComputerUse => "the computer job",
        JobKind::General => "the job",
        JobKind::Research => "the research job",
        JobKind::DeepAnswer => "the deep answer",
        JobKind::Librarian => "the librarian",
        JobKind::Replay => "the replay",
        JobKind::ContinueSession => "the session job",
        JobKind::AppTask => "the app task",
        JobKind::Act => "the action",
        JobKind::MlProject => "the ML project",
    }
}

/// The spoken name from a title hint: at most four words, "the" before a single word, and the
/// kind's words when the hint is blank or only an article.
fn name_of(hint: Option<&str>, kind: JobKind) -> String {
    let words: Vec<&str> = hint.unwrap_or_default().split_whitespace().take(NAME_WORDS).collect();
    match words.as_slice() {
        [] => kind_words(kind).to_owned(),
        [one] if ARTICLES.iter().any(|article| one.eq_ignore_ascii_case(article)) => {
            kind_words(kind).to_owned()
        }
        [one] => format!("the {one}"),
        more => more.join(" "),
    }
}

/// A slug from a name: its ASCII letters and digits in lower case, words joined by hyphens,
/// leading articles dropped, and cut at a word to at most [`SLUG_MAX`] bytes.
fn slug_of(name: &str) -> String {
    let words = name
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_ascii_lowercase)
        .skip_while(|word| ARTICLES.contains(&word.as_str()));
    let mut slug = String::new();
    for word in words {
        if slug.is_empty() {
            // The words are ASCII, so any byte is a character boundary.
            slug.push_str(word.get(..SLUG_MAX).unwrap_or(&word));
        } else if slug.len().saturating_add(word.len()) < SLUG_MAX {
            slug.push('-');
            slug.push_str(&word);
        } else {
            break;
        }
    }
    slug
}

/// Whether `repo` is an absolute path with no `.` or `..` part: a drive letter and a separator,
/// a UNC path, or a leading `/`. A drive-relative path such as `D:demo` is not one. A leading
/// `/` is a Linux path; on Windows it names the root of the current drive, and whether a path
/// lies inside Eileen's own data folder is the runtime's to check, since the core does not know
/// where that folder is.
fn absolute(repo: &str) -> bool {
    let repo = repo.trim();
    let bytes = repo.as_bytes();
    let drive = bytes.first().is_some_and(u8::is_ascii_alphabetic)
        && bytes.get(1) == Some(&b':')
        && matches!(bytes.get(2), Some(b'\\' | b'/'));
    let rooted = drive || repo.starts_with("\\\\") || repo.starts_with('/');
    rooted && !repo.split(['\\', '/']).any(|part| part == "." || part == "..")
}

/// A repository named two ways is one repository: a trailing separator does not count, and in
/// a Windows path (a drive letter or a backslash) neither do case and the kind of separator.
fn repo_key(repo: &str) -> String {
    let repo = repo.trim().trim_end_matches(['\\', '/']);
    let windows = repo.contains('\\') || repo.as_bytes().get(1) == Some(&b':');
    if windows { repo.replace('/', "\\").to_ascii_lowercase() } else { repo.to_owned() }
}

/// 64-bit FNV-1a, which names a repository's folder of worktrees the same way in every
/// version.
fn fnv1a(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use eileen_contracts::ids::{JobId, TurnId, WallTime};
    use eileen_contracts::jobs::{
        Cost, Depth, FanoutMode, Job, JobKind, JobSpec, JobStatus, Route, Taint,
    };
    use eileen_contracts::tools::brain::JobRef;

    use super::{
        Pick, Registry, Step, absolute, fnv1a, kind_named, legal, name_of, plain_words, repo_key,
        route_named, slug_of,
    };

    const ALL: [JobStatus; 8] = [
        JobStatus::Queued,
        JobStatus::Running,
        JobStatus::Blocked,
        JobStatus::AwaitingApproval,
        JobStatus::Completed,
        JobStatus::Failed,
        JobStatus::Cancelled,
        JobStatus::Interrupted,
    ];

    #[test]
    fn the_steps_take_exactly_the_edges_of_the_graph() {
        let steps = [
            Step::Start,
            Step::Block,
            Step::AwaitApproval { deadline: eileen_contracts::ids::WallTime(0) },
            Step::Resume,
            Step::Complete,
            Step::Fail,
            Step::Cancel,
            Step::Interrupt,
        ];
        for from in ALL {
            for to in ALL {
                let by_step = steps.iter().any(|step| step.leaves(from) && step.status() == to);
                assert_eq!(by_step, legal(from, to), "{from:?} to {to:?}");
            }
            if from.is_terminal() {
                assert!(steps.iter().all(|step| !step.leaves(from)), "{from:?} is final");
            }
        }
    }

    #[test]
    fn names_have_two_to_four_words() {
        assert_eq!(
            name_of(Some("the hello file please now"), JobKind::General),
            "the hello file please"
        );
        assert_eq!(name_of(Some("render"), JobKind::AppTask), "the render", "one word");
        assert_eq!(name_of(Some(" The "), JobKind::Research), "the research job", "an article");
        assert_eq!(name_of(None, JobKind::Coding), "the coding job", "no hint");
        assert_eq!(name_of(Some("   "), JobKind::Coding), "the coding job", "a blank hint");
    }

    #[test]
    fn slugs_are_short_lower_case_ascii_with_no_leading_article() {
        assert_eq!(slug_of("The hello file!"), "hello-file", "the article goes");
        assert_eq!(slug_of("Fix Ünïcode parsing"), "fix-n-code-parsing", "ASCII only");
        assert_eq!(slug_of("日本語"), "", "nothing to keep");
        let long = slug_of("synchronise every calendar entry between devices tonight");
        assert_eq!(long, "synchronise-every-calendar-entry-between", "cut at a word");
        assert_eq!(slug_of(&"x".repeat(90)).len(), super::SLUG_MAX, "one long word is cut");
    }

    #[test]
    fn one_repository_named_two_ways_has_one_key() {
        assert_eq!(repo_key("D:\\Code\\Demo\\"), repo_key(" d:/code/demo"), "the same repository");
        assert_ne!(repo_key("D:\\code\\demo"), repo_key("D:\\code\\other"), "another one");
        assert_eq!(repo_key("/home/u/demo/"), repo_key("/home/u/demo"), "a trailing slash");
        assert_ne!(repo_key("/home/u/Demo"), repo_key("/home/u/demo"), "Linux keeps case");
        assert_eq!(fnv1a(""), 0xcbf2_9ce4_8422_2325, "the offset basis");
        assert_eq!(fnv1a("a"), 0xaf63_dc4c_8601_ec8c, "the published value for \"a\"");
    }

    #[test]
    fn only_an_absolute_path_with_no_dot_parts_is_a_checkout() {
        let checkouts =
            ["D:\\code\\demo", " d:/code/demo/ ", "\\\\server\\share\\demo", "/home/u/demo"];
        for repo in checkouts {
            assert!(absolute(repo), "{repo:?} is a checkout");
        }
        let names = ["demo", "learning", "..\\..", "..\\x", "run/x", "D:demo", "D:\\code\\..\\x"];
        for repo in names.into_iter().chain(["/home/./u", "\\\\.\\pipe\\x", "", "   "]) {
            assert!(!absolute(repo), "{repo:?} is not");
        }
    }

    /// A job the registry names for itself, number `n`, accepted at `n` ms.
    fn job(registry: &Registry, n: u64, hint: &str, kind: JobKind, parent: Option<JobId>) -> Job {
        let at = WallTime(i64::try_from(n).unwrap() * 1_000);
        let naming = registry.naming(Some(hint), kind, parent, at).unwrap();
        let spec = JobSpec {
            kind,
            task: hint.to_owned(),
            title_hint: Some(hint.to_owned()),
            depth: Depth::Considered,
            route: None,
            model: None,
            fanout: if hint == "flaky login tests" { 2 } else { 1 },
            mode: FanoutMode::Compete,
            subtasks: Vec::new(),
            subquestions: Vec::new(),
            repo: None,
            resume_session: None,
            skill: None,
            slots: BTreeMap::new(),
            topic: None,
            anticipated: false,
            authorized_by: TurnId::from_parts(1, 1),
            parent,
            chrome: false,
        };
        Job {
            id: JobId::from_parts(n, u128::from(n)),
            name: naming.name,
            slug: naming.slug,
            ordinal: naming.ordinal,
            spec,
            status: JobStatus::Queued,
            route: Route::Claude,
            model: "mid".to_owned(),
            session: None,
            created: at,
            updated: at,
            last_progress: None,
            progress_pct: None,
            salience_pct: 50,
            worktree: None,
            branch: None,
            snapshot_ref: None,
            cost: Cost::default(),
            grants: Vec::new(),
            taint: Taint::default(),
            focus: false,
            override_words: None,
            approval_deadline: None,
        }
    }

    /// A report, a price check (research), a render (an app task) and a fan-out of two coding
    /// agents, accepted in that order.
    fn four_jobs() -> (Registry, [JobId; 6]) {
        let mut registry = Registry::default();
        let plain = [
            (1, "the report", JobKind::General),
            (2, "the price check", JobKind::Research),
            (3, "render", JobKind::AppTask),
            (4, "flaky login tests", JobKind::Coding),
        ];
        for (n, hint, kind) in plain {
            let job = job(&registry, n, hint, kind, None);
            registry.accept(job).unwrap();
        }
        let parent = JobId::from_parts(4, 4);
        for n in [5, 6] {
            let child = job(&registry, n, "", JobKind::Coding, Some(parent));
            registry.accept(child).unwrap();
        }
        let ids = [1, 2, 3, 4, 5, 6].map(|n| JobId::from_parts(n, u128::from(n)));
        (registry, ids)
    }

    fn name(words: &str) -> JobRef {
        JobRef::Name(words.to_owned())
    }

    #[test]
    fn words_name_a_job_by_name_slug_ordinal_kind_and_route_in_that_order() {
        let (registry, [report, price, render, fan, one, two]) = four_jobs();
        let pick = |target: JobRef| registry.pick(Some(&target), None);
        assert_eq!(pick(name("The Report")), Pick::One(report), "a name in any case");
        assert_eq!(pick(name("report")), Pick::One(report), "a name without its article");
        assert_eq!(pick(JobRef::Slug("price-check".to_owned())), Pick::One(price), "a slug");
        assert_eq!(pick(JobRef::Id(render)), Pick::One(render), "an id");
        assert_eq!(pick(JobRef::Ordinal(2)), Pick::One(two), "an ordinal");
        assert_eq!(pick(name("agent one")), Pick::One(one), "a child's name");
        assert_eq!(pick(name("the render")), Pick::One(render), "the render by name");
        assert_eq!(pick(name("the research job")), Pick::One(price), "a kind word");
        assert_eq!(pick(name("the coding job")), Pick::One(fan), "a kind word names no child");
        assert_eq!(
            pick(name("Claude's job")),
            Pick::Ask(vec![fan, render, price, report]),
            "a route word that fits several jobs, none in focus, asks, newest first"
        );
        assert_eq!(pick(name("the other file")), Pick::Nothing, "words that fit no job");
        assert_eq!(pick(JobRef::Ordinal(3)), Pick::Nothing, "no third agent");
    }

    #[test]
    fn a_pronoun_names_the_announced_job_then_the_focus_job_then_the_newest() {
        let (mut registry, [report, price, render, fan, _, _]) = four_jobs();
        assert_eq!(registry.pick(None, None), Pick::One(fan), "the newest outside a fan-out");
        assert_eq!(registry.focus_on(price, WallTime(10)), [price], "the price check in focus");
        for words in [None, Some(name("it")), Some(name("that one")), Some(name("the job"))] {
            let picked = registry.pick(words.as_ref(), None);
            assert_eq!(picked, Pick::One(price), "{words:?}: the focus job");
        }
        let said = registry.pick(Some(&name("it")), Some(render));
        assert_eq!(said, Pick::One(render), "it, right after an announcement");
        assert_eq!(
            registry.pick(Some(&name("Claude's job")), None),
            Pick::One(price),
            "the focus job among several that fit"
        );
        registry.step(report, Step::Start, WallTime(11)).unwrap();
        registry.step(report, Step::Complete, WallTime(12)).unwrap();
        let finished = registry.pick(Some(&name("it")), Some(report));
        assert_eq!(finished, Pick::One(price), "a finished announced job is passed over");
        assert_eq!(registry.pick(Some(&name("the report")), None), Pick::Nothing, "finished");
    }

    #[test]
    fn the_focus_is_held_by_one_job_and_moves_off_a_finished_one() {
        let (mut registry, [report, price, _, fan, one, _]) = four_jobs();
        assert_eq!(registry.focused(), None, "nothing in focus yet");
        assert!(registry.refocus(WallTime(9)).contains(&fan), "the newest outside a fan-out");
        let moved = registry.focus_on(price, WallTime(10));
        assert_eq!(moved, [price, fan], "the old focus goes as the new one comes");
        assert!(registry.focus_on(price, WallTime(11)).is_empty(), "already in focus");
        assert!(registry.refocus(WallTime(11)).is_empty(), "the focus job runs on");
        registry.step(price, Step::Cancel, WallTime(12)).unwrap();
        let moved = registry.refocus(WallTime(13));
        assert_eq!(moved, [price, fan], "a finished job gives it up, to the newest");
        assert_eq!(registry.focused(), Some(fan), "one focus job");
        assert_eq!(registry.focus_on(one, WallTime(14)), [fan, one], "a child can be followed");
        registry.step(report, Step::Cancel, WallTime(15)).unwrap();
        assert!(registry.focus_on(report, WallTime(16)).is_empty(), "never a finished job");
        let count = registry.jobs().iter().filter(|job| job.focus).count();
        assert_eq!(count, 1, "exactly one job holds the focus");
    }

    #[test]
    fn a_successor_keeps_the_name_and_ordinal_with_a_slug_of_its_own() {
        let (registry, [report, _, _, _, _, two]) = four_jobs();
        let next = registry.successor(report, WallTime(20)).unwrap();
        assert_eq!((next.name.as_str(), next.ordinal), ("the report", None), "the same name");
        assert_eq!(next.slug, "report-2", "a slug of its own");
        let next = registry.successor(two, WallTime(20)).unwrap();
        assert_eq!((next.name.as_str(), next.ordinal), ("agent two", Some(2)), "the same agent");
    }

    #[test]
    fn progress_is_kept_once_per_change_and_never_past_a_hundred() {
        let (mut registry, [_, _, render, _, _, _]) = four_jobs();
        assert!(registry.progress(render, 60, WallTime(1)).is_some(), "a new percent");
        assert!(registry.progress(render, 60, WallTime(2)).is_none(), "the same percent");
        let pct = registry.progress(render, 250, WallTime(3)).and_then(|job| job.progress_pct);
        assert_eq!(pct, Some(100), "at most a hundred");
    }

    #[test]
    fn plain_words_drop_articles_filler_and_possessives() {
        assert_eq!(plain_words("Claude's job"), ["claude"], "a possessive");
        assert_eq!(plain_words("the  Render, please"), ["render"], "an article and filler");
        assert_eq!(plain_words("Claude\u{2019}s task"), ["claude"], "a curly apostrophe");
        assert_eq!(kind_named(&plain_words("the deep answer")), Some(JobKind::DeepAnswer), "two");
        assert_eq!(kind_named(&plain_words("the hello file")), None, "not a kind word");
        assert_eq!(route_named(&plain_words("the local model")), Some(Route::Local), "a route");
    }
}
