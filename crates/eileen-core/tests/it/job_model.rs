//! Acceptance 3 of BUILD.md section 6.3: a reference model of the job lifecycle, written apart
//! from the core's own code, agrees with `eileen_core::lifecycle::Registry` on random command
//! sequences (proptest-state-machine). The model holds section 2.5's graph as a table of edges,
//! each job's state and approval deadline, its name and fan-out ordinal, the decline at the
//! approval deadline, a restart, the job a cancel names, and where a coding job works.

use std::collections::BTreeMap;

use eileen_contracts::ids::{JobId, Personal, TurnId, WallTime};
use eileen_contracts::jobs::{
    Cost, Depth, FanoutMode, Job, JobKind, JobSpec, JobStatus, Route, Taint,
};
use eileen_contracts::tools::brain::JobRef;
use eileen_core::lifecycle::{
    self, Checkout, KEPT, Naming, Refused, Registry, SLUG_MAX, SLUG_WINDOW_US, Step,
};
use proptest::prelude::*;
use proptest::sample::select;
use proptest_state_machine::{ReferenceStateMachine, StateMachineTest, prop_state_machine};

/// 2026-01-01T00:00:00Z, in microseconds: the model's clock starts here.
const START_US: i64 = 1_767_225_600_000_000;

/// `approvals.deadline_min`'s default, 30 minutes, in microseconds.
const DEADLINE_US: i64 = 30 * 60_000_000;

/// A minute, in microseconds. A sequence waits at most 20 minutes a step, so every job in it
/// is inside the seven days a slug is checked against.
const MINUTE_US: i64 = 60_000_000;

/// The titles the brain gives, each with its kind and the spoken name the model expects.
const TITLES: [(Option<&str>, JobKind, &str); 6] = [
    (Some("the hello file"), JobKind::General, "the hello file"),
    (Some("render"), JobKind::General, "the render"),
    (Some("fix the flaky login tests now"), JobKind::Coding, "fix the flaky login"),
    (Some("parser fix"), JobKind::Coding, "parser fix"),
    (None, JobKind::Research, "the research job"),
    (Some("the hello file"), JobKind::Coding, "the hello file"),
];

/// The repositories a job names: two of them are one repository named two ways.
const REPOS: [Option<&str>; 4] =
    [None, Some("D:\\code\\demo"), Some("d:/code/demo/"), Some("D:\\code\\other")];

/// A fan-out child's spoken name, by ordinal.
const AGENTS: [&str; 8] = [
    "agent one",
    "agent two",
    "agent three",
    "agent four",
    "agent five",
    "agent six",
    "agent seven",
    "agent eight",
];

/// A change the model can ask of a job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Move {
    Start,
    Block,
    Ask,
    Resume,
    Complete,
    Fail,
    Cancel,
    Interrupt,
}

const MOVES: [Move; 8] = [
    Move::Start,
    Move::Block,
    Move::Ask,
    Move::Resume,
    Move::Complete,
    Move::Fail,
    Move::Cancel,
    Move::Interrupt,
];

/// Section 2.5's graph, written out edge by edge: every move a state allows and where it goes.
const EDGES: [(JobStatus, Move, JobStatus); 18] = [
    (JobStatus::Queued, Move::Start, JobStatus::Running),
    (JobStatus::Queued, Move::Fail, JobStatus::Failed),
    (JobStatus::Queued, Move::Cancel, JobStatus::Cancelled),
    (JobStatus::Queued, Move::Interrupt, JobStatus::Interrupted),
    (JobStatus::Running, Move::Block, JobStatus::Blocked),
    (JobStatus::Running, Move::Ask, JobStatus::AwaitingApproval),
    (JobStatus::Running, Move::Complete, JobStatus::Completed),
    (JobStatus::Running, Move::Fail, JobStatus::Failed),
    (JobStatus::Running, Move::Cancel, JobStatus::Cancelled),
    (JobStatus::Running, Move::Interrupt, JobStatus::Interrupted),
    (JobStatus::Blocked, Move::Resume, JobStatus::Running),
    (JobStatus::Blocked, Move::Fail, JobStatus::Failed),
    (JobStatus::Blocked, Move::Cancel, JobStatus::Cancelled),
    (JobStatus::Blocked, Move::Interrupt, JobStatus::Interrupted),
    (JobStatus::AwaitingApproval, Move::Resume, JobStatus::Running),
    (JobStatus::AwaitingApproval, Move::Fail, JobStatus::Failed),
    (JobStatus::AwaitingApproval, Move::Cancel, JobStatus::Cancelled),
    (JobStatus::AwaitingApproval, Move::Interrupt, JobStatus::Interrupted),
];

/// Where to the model's graph sends `from` on `mv`, if anywhere.
fn edge(from: JobStatus, mv: Move) -> Option<JobStatus> {
    EDGES.iter().find(|(at, by, _)| *at == from && *by == mv).map(|(_, _, to)| *to)
}

/// Whether a job can still change.
fn open(status: JobStatus) -> bool {
    !matches!(
        status,
        JobStatus::Completed | JobStatus::Failed | JobStatus::Cancelled | JobStatus::Interrupted
    )
}

/// What a cancel names.
#[derive(Debug, Clone)]
enum Target {
    Id(JobId),
    SlugOf(JobId),
    Name(usize),
    Agent(u8),
}

/// One command of a random sequence.
#[derive(Debug, Clone)]
enum Cmd {
    Accept { title: usize, repo: usize, parent: Option<JobId> },
    Move { job: JobId, mv: Move },
    Wait { minutes: u8 },
    Expire,
    Restart,
    Cancel(Target),
}

/// Where the model expects a new job to work. A worktree carries the repository's key, so
/// one repository named two ways shares a folder of worktrees.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Place {
    Scratch,
    InPlace(String),
    Worktree(String),
}

/// What the model expects of the command it just took.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Expect {
    Nothing,
    Accept { name: String, ordinal: Option<u8>, place: Place, kept: bool },
    Unknown,
    Illegal,
    Moved,
    Ids(Vec<JobId>),
    Cancelled(Option<JobId>),
}

/// A job as the model sees it.
#[derive(Debug, Clone)]
struct ModelJob {
    id: JobId,
    status: JobStatus,
    deadline: Option<i64>,
    parent: Option<JobId>,
    ordinal: Option<u8>,
    name: String,
    kind: JobKind,
    repo: Option<String>,
}

#[derive(Debug, Clone)]
struct Model {
    now: i64,
    minted: u64,
    jobs: Vec<ModelJob>,
    expect: Expect,
}

/// The id of the `n`th job minted.
fn id_of(n: u64) -> JobId {
    JobId::from_parts(n, u128::from(n))
}

/// A repository's key: case and separators do not count.
fn key(repo: &str) -> String {
    repo.trim().trim_end_matches(['\\', '/']).replace('/', "\\").to_lowercase()
}

impl Model {
    fn job_mut(&mut self, id: JobId) -> Option<&mut ModelJob> {
        self.jobs.iter_mut().find(|job| job.id == id)
    }

    fn accept(&mut self, title: usize, repo: usize, parent: Option<JobId>) -> Expect {
        self.minted += 1;
        let id = id_of(self.minted);
        let (_, kind, hinted) = TITLES[title];
        let repo = REPOS[repo].map(str::to_owned);
        let child = parent.filter(|parent| self.jobs.iter().any(|job| job.id == *parent));
        let ordinal = child.and_then(|parent| {
            let older = self.jobs.iter().filter(|job| job.parent == Some(parent)).count();
            u8::try_from(older + 1).ok().filter(|n| *n <= 8)
        });
        let name = ordinal.map_or(hinted, |n| AGENTS[usize::from(n) - 1]).to_owned();
        let place = match repo.as_deref().filter(|_| kind == JobKind::Coding) {
            None => Place::Scratch,
            Some(repo) => {
                let alongside = self.jobs.iter().any(|job| {
                    job.kind == JobKind::Coding
                        && open(job.status)
                        && job.repo.as_deref().is_some_and(|theirs| key(theirs) == key(repo))
                });
                if alongside || parent.is_some() {
                    Place::Worktree(key(repo))
                } else {
                    Place::InPlace(format!("refs/eileen/{id}"))
                }
            }
        };
        if self.jobs.len() >= KEPT {
            match self.jobs.iter().position(|job| !open(job.status)) {
                Some(oldest) => {
                    self.jobs.remove(oldest);
                }
                None => return Expect::Accept { name, ordinal, place, kept: false },
            }
        }
        let status = JobStatus::Queued;
        let job = ModelJob {
            id,
            status,
            deadline: None,
            parent,
            ordinal,
            name: name.clone(),
            kind,
            repo,
        };
        self.jobs.push(job);
        Expect::Accept { name, ordinal, place, kept: true }
    }

    fn take(&mut self, id: JobId, mv: Move) -> Expect {
        let now = self.now;
        let Some(job) = self.job_mut(id) else { return Expect::Unknown };
        let Some(to) = edge(job.status, mv) else { return Expect::Illegal };
        job.status = to;
        job.deadline = (mv == Move::Ask).then_some(now + DEADLINE_US);
        Expect::Moved
    }

    /// Every approval past its deadline is declined, and its job resumes.
    fn expire(&mut self) -> Expect {
        let now = self.now;
        let mut due = Vec::new();
        for job in &mut self.jobs {
            if job.status == JobStatus::AwaitingApproval && job.deadline.is_some_and(|d| d <= now) {
                job.status = JobStatus::Running;
                job.deadline = None;
                due.push(job.id);
            }
        }
        Expect::Ids(due)
    }

    /// No worker outlives the core: every unfinished job is interrupted.
    fn restart(&mut self) -> Expect {
        let mut interrupted = Vec::new();
        for job in self.jobs.iter_mut().filter(|job| open(job.status)) {
            job.status = JobStatus::Interrupted;
            job.deadline = None;
            interrupted.push(job.id);
        }
        Expect::Ids(interrupted)
    }

    /// The newest unfinished job the target names is cancelled.
    fn cancel(&mut self, target: &Target) -> Expect {
        let named = |job: &&mut ModelJob| match target {
            Target::Id(id) | Target::SlugOf(id) => job.id == *id,
            Target::Name(title) => job.name.eq_ignore_ascii_case(TITLES[*title].2),
            Target::Agent(n) => job.ordinal == Some(*n),
        };
        let found = self.jobs.iter_mut().rev().filter(|job| open(job.status)).find(named);
        let id = found.map(|job| {
            job.status = JobStatus::Cancelled;
            job.deadline = None;
            job.id
        });
        Expect::Cancelled(id)
    }
}

struct Lifecycle;

impl ReferenceStateMachine for Lifecycle {
    type State = Model;
    type Transition = Cmd;

    fn init_state() -> BoxedStrategy<Model> {
        Just(Model { now: START_US, minted: 0, jobs: Vec::new(), expect: Expect::Nothing }).boxed()
    }

    fn transitions(state: &Model) -> BoxedStrategy<Cmd> {
        let titles = 0..TITLES.len();
        let repos = 0..REPOS.len();
        let always = prop_oneof![
            4 => (titles.clone(), repos.clone())
                .prop_map(|(title, repo)| Cmd::Accept { title, repo, parent: None }),
            1 => (1..=20_u8).prop_map(|minutes| Cmd::Wait { minutes }),
            1 => Just(Cmd::Expire),
            1 => Just(Cmd::Restart),
            1 => (1..=3_u8).prop_map(|n| Cmd::Cancel(Target::Agent(n))),
            1 => titles.clone().prop_map(|title| Cmd::Cancel(Target::Name(title))),
        ];
        let ids: Vec<JobId> = (1..=state.minted).map(id_of).collect();
        if ids.is_empty() {
            return always.boxed();
        }
        prop_oneof![
            3 => always,
            3 => (titles, repos, select(ids.clone()))
                .prop_map(|(title, repo, parent)| Cmd::Accept { title, repo, parent: Some(parent) }),
            8 => (select(ids.clone()), select(MOVES.to_vec()))
                .prop_map(|(job, mv)| Cmd::Move { job, mv }),
            1 => select(ids.clone()).prop_map(|job| Cmd::Cancel(Target::Id(job))),
            1 => select(ids).prop_map(|job| Cmd::Cancel(Target::SlugOf(job))),
        ]
        .boxed()
    }

    fn apply(mut state: Model, cmd: &Cmd) -> Model {
        state.expect = match cmd {
            Cmd::Accept { title, repo, parent } => state.accept(*title, *repo, *parent),
            Cmd::Move { job, mv } => state.take(*job, *mv),
            Cmd::Wait { minutes } => {
                state.now += i64::from(*minutes) * MINUTE_US;
                Expect::Nothing
            }
            Cmd::Expire => state.expire(),
            Cmd::Restart => state.restart(),
            Cmd::Cancel(target) => state.cancel(target),
        };
        state
    }
}

/// The registry under test, and the folder of worktrees each repository key was given.
struct Sut {
    registry: Registry,
    folders: BTreeMap<String, String>,
}

/// A new job as the core builds one, before the registry keeps it.
fn job(
    id: JobId,
    naming: Naming,
    title: usize,
    repo: usize,
    parent: Option<JobId>,
    at: i64,
) -> Job {
    let (hint, kind, _) = TITLES[title];
    let spec = JobSpec {
        kind,
        task: "a task".to_owned(),
        title_hint: hint.map(str::to_owned),
        depth: Depth::Considered,
        route: None,
        model: None,
        fanout: 1,
        mode: FanoutMode::Compete,
        subtasks: Vec::new(),
        subquestions: Vec::new(),
        repo: REPOS[repo].map(|repo| Personal::new(repo.to_owned())),
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
        id,
        name: naming.name,
        slug: naming.slug,
        ordinal: naming.ordinal,
        spec,
        status: JobStatus::Queued,
        route: Route::Claude,
        model: "mid".to_owned(),
        session: None,
        created: WallTime(at),
        updated: WallTime(at),
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

impl Sut {
    fn accept(&mut self, model: &Model, title: usize, repo: usize, parent: Option<JobId>) {
        let Expect::Accept { name, ordinal, place, kept } = &model.expect else {
            panic!("the model accepted nothing: {:?}", model.expect);
        };
        let (hint, kind, _) = TITLES[title];
        let naming = self.registry.naming(hint, kind, parent, WallTime(model.now));
        assert_eq!(&naming.name, name, "the spoken name");
        assert_eq!(&naming.ordinal, ordinal, "the ordinal");
        if let (Some(n), Some(parent)) = (ordinal, parent) {
            let theirs = &self.registry.get(parent).expect("the parent is kept").slug;
            assert!(naming.slug.starts_with(&format!("{theirs}-{n}")), "{}", naming.slug);
        }
        let job = job(id_of(model.minted), naming, title, repo, parent, model.now);
        self.check_place(&job, place);
        let accepted = self.registry.accept(job);
        let wanted = if *kept { Ok(()) } else { Err(Refused::Full) };
        assert_eq!(accepted, wanted, "kept while there is room");
    }

    fn check_place(&mut self, job: &Job, place: &Place) {
        match (place, self.registry.checkout(job, false)) {
            (Place::Scratch, Checkout::Scratch) => {}
            (Place::InPlace(wanted), Checkout::InPlace { snapshot_ref, .. }) => {
                assert_eq!(&snapshot_ref, wanted, "the snapshot ref");
            }
            (Place::Worktree(key), Checkout::Worktree(spec)) => {
                assert_eq!(spec.branch, format!("eileen/{}", job.slug), "its own branch");
                assert_eq!(spec.base, "HEAD", "from the user's current branch");
                let path = spec.path.get();
                let folder = path.strip_suffix(&format!("/{}", job.slug)).expect("named by slug");
                let known = self.folders.entry(key.clone()).or_insert_with(|| folder.to_owned());
                assert_eq!(known, folder, "one folder of worktrees per repository");
                let shared = self.folders.iter().filter(|(_, theirs)| *theirs == folder).count();
                assert_eq!(shared, 1, "another repository's folder is another one");
            }
            (place, checkout) => panic!("expected {place:?}, got {checkout:?}"),
        }
    }

    fn take(&mut self, model: &Model, id: JobId, mv: Move) {
        let before = self.registry.get(id).map(|job| job.status);
        let step = match mv {
            Move::Start => Step::Start,
            Move::Block => Step::Block,
            Move::Ask => Step::AwaitApproval { deadline: WallTime(model.now + DEADLINE_US) },
            Move::Resume => Step::Resume,
            Move::Complete => Step::Complete,
            Move::Fail => Step::Fail,
            Move::Cancel => Step::Cancel,
            Move::Interrupt => Step::Interrupt,
        };
        let taken = self.registry.step(id, step, WallTime(model.now)).map(|job| job.status);
        match (&model.expect, taken, before) {
            (Expect::Moved, Ok(after), Some(before)) => {
                assert!(lifecycle::legal(before, after), "{before:?} to {after:?} is an edge");
            }
            (Expect::Unknown, Err(Refused::Unknown(job)), None) => assert_eq!(job, id, "the job"),
            (Expect::Illegal, Err(Refused::Illegal { from, .. }), Some(before)) => {
                assert_eq!(from, before, "refused from where it was");
            }
            (expect, taken, _) => panic!("expected {expect:?}, got {taken:?}"),
        }
    }

    fn cancel(&mut self, model: &Model, target: &Target) {
        let target = match target {
            Target::Id(id) => JobRef::Id(*id),
            Target::SlugOf(id) => {
                let slug = self.registry.get(*id).map(|job| job.slug.clone());
                JobRef::Slug(slug.unwrap_or_else(|| "a-job-no-longer-kept".to_owned()))
            }
            Target::Name(title) => JobRef::Name(TITLES[*title].2.to_uppercase()),
            Target::Agent(n) => JobRef::Ordinal(*n),
        };
        let found = self.registry.resolve(&target);
        assert_eq!(model.expect, Expect::Cancelled(found), "{target:?} names the same job");
        if let Some(id) = found {
            let cancelled = self.registry.step(id, Step::Cancel, WallTime(model.now));
            assert!(cancelled.is_ok(), "an unfinished job can be cancelled");
        }
    }
}

impl StateMachineTest for Lifecycle {
    type SystemUnderTest = Sut;
    type Reference = Self;

    fn init_test(_: &Model) -> Sut {
        Sut { registry: Registry::default(), folders: BTreeMap::new() }
    }

    fn apply(mut sut: Sut, model: &Model, cmd: Cmd) -> Sut {
        let at = WallTime(model.now);
        match cmd {
            Cmd::Accept { title, repo, parent } => sut.accept(model, title, repo, parent),
            Cmd::Move { job, mv } => sut.take(model, job, mv),
            Cmd::Wait { .. } => {
                assert_eq!(model.expect, Expect::Nothing, "waiting changes nothing")
            }
            Cmd::Expire => {
                let due = sut.registry.expired(at);
                assert_eq!(model.expect, Expect::Ids(due.clone()), "the approvals past due");
                for id in due {
                    assert!(
                        sut.registry.step(id, Step::Resume, at).is_ok(),
                        "declined, it resumes"
                    );
                }
            }
            Cmd::Restart => {
                let mut registry = Registry::resumed(sut.registry.jobs().to_vec());
                let interrupted = registry.restart(at);
                assert_eq!(model.expect, Expect::Ids(interrupted), "every unfinished job");
                sut.registry = registry;
            }
            Cmd::Cancel(target) => sut.cancel(model, &target),
        }
        sut
    }

    fn check_invariants(sut: &Sut, model: &Model) {
        let jobs = sut.registry.jobs();
        let ids: Vec<JobId> = jobs.iter().map(|job| job.id).collect();
        let wanted: Vec<JobId> = model.jobs.iter().map(|job| job.id).collect();
        assert_eq!(ids, wanted, "the same jobs, in accept order");
        for (job, theirs) in jobs.iter().zip(&model.jobs) {
            assert_eq!(job.status, theirs.status, "{}: its state", job.id);
            assert_eq!(job.approval_deadline, theirs.deadline.map(WallTime), "{}", job.id);
            assert_eq!((&job.name, job.ordinal), (&theirs.name, theirs.ordinal), "{}", job.id);
            let ascii =
                job.slug.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
            assert!(ascii && !job.slug.is_empty(), "a plain slug: {}", job.slug);
            // A child's slug is its parent's and its ordinal, and the model lets a child be a
            // parent, which the core never does: only a job with no parent is held to the length.
            if job.spec.parent.is_none() {
                assert!(job.slug.len() <= SLUG_MAX + 3, "a short slug: {}", job.slug);
            }
            let twins = jobs.iter().filter(|other| other.slug == job.slug).count();
            assert_eq!(twins, 1, "no other job of the week holds {}", job.slug);
        }
    }
}

prop_state_machine! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn the_registry_agrees_with_the_reference_model(sequential 1..60 => Lifecycle);
}

/// A slug is checked against seven days of jobs: one a week old is free again.
#[test]
fn a_slug_is_free_again_after_seven_days() {
    let mut registry = Registry::default();
    let naming =
        registry.naming(Some("the hello file"), JobKind::General, None, WallTime(START_US));
    assert_eq!(naming.slug, "hello-file", "the first");
    registry.accept(job(id_of(1), naming, 0, 0, None, START_US)).expect("kept");
    let soon = registry.naming(Some("the hello file"), JobKind::General, None, WallTime(START_US));
    assert_eq!(soon.slug, "hello-file-2", "taken within the week");
    let later = WallTime(START_US + SLUG_WINDOW_US + 1);
    let free = registry.naming(Some("the hello file"), JobKind::General, None, later);
    assert_eq!(free.slug, "hello-file", "free again after it");
}
