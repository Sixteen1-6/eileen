//! Jobs (BUILD.md sections 2.2 step 7 and 2.5): the brain's `jobs_dispatch` accepts one job or
//! a fan-out of one to eight workers, the router (`router`) picks each job's route, model and
//! depth, a cold worker session starts for each worker (`WorkerCmd::Start`, then `Bind` once it
//! is ready, in `worker`), every job reaches one terminal state, and completions and failures
//! are announced. States, names, slugs, ordinals and the focus job come from
//! [`crate::lifecycle`]; a kind v0.1 does not run is refused with a `UserError` before anything
//! else is decided. Every job works in its session's own folder until the harness records an
//! in-place snapshot and creates worktrees. Steering, rerouting and cancelling by name are in
//! `control`, the render a Resolve bridge reports in `app_task`.
//!
//! Only the brain's own shim may dispatch, only for the brain turn in flight, and the job takes
//! the authority the core sent that turn with, never the authority the request claims (section
//! 2.3): a call from an ambient turn, from a turn the user stopped, or naming a turn the core
//! never sent is refused.
//!
//! A fan-out is a parent job with no session and one child per worker, "agent one" to "agent
//! eight" in accept order. The parent runs once a child runs and ends once every child has
//! ended: completed when any child completed, with one announcement for the whole fan-out.

use std::collections::{BTreeMap, VecDeque};

use eileen_contracts::bus::{BusEvent, CancelSource};
use eileen_contracts::core_io::{
    Caller, Content, Effect, Record, ShimRole, ToolCallRequest, ToolResult,
};
use eileen_contracts::harness::WorkerCmd;
use eileen_contracts::ids::{
    CallId, JobId, Personal, TraceCtx, TurnId, Untrusted, WallTime, WorkerSessionId,
};
use eileen_contracts::jobs::{
    CallOrigin, Cost, FanoutMode, Job, JobKind, JobSpec, JobStatus, Taint, TurnAuthority,
};
use eileen_contracts::tools::brain::{DispatchedJob, JobsDispatch, JobsDispatchOutput};

use crate::announce::{self, Announcement};
use crate::lifecycle::{self, Naming, Registry, Step};
use crate::router::{Ask, Routed};
use crate::worker::session_spec;
use crate::{Core, Cx, job_ctx, user_error};

/// Why a `jobs_dispatch` or `jobs_control` call that did not come from the brain's own shim,
/// for a brain turn, was refused.
const NOT_THE_CONVERSATION: &str = "Only the conversation can start or change a job.";

/// Why a dispatch or a reroute was refused when the kept jobs have no room for it.
pub(crate) const FULL: &str = "Too many jobs are open; cancel one and ask again.";

/// Why a dispatch was refused when too many workers run.
const BUSY: &str = "Too many workers are running; I can start more when some finish.";

/// The most worker sessions live at once, a fan-out's included. Memory admission (section
/// 2.5) decides each start once it is built; this bound holds until then and after.
pub(crate) const MAX_LIVE: usize = 16;

/// The most workers in a fan-out (section 3.3.2).
const FANOUT_MAX: u8 = 8;

/// The longest task a worker is given, in characters (section 3.3.2), steers included.
pub(crate) const TASK_CHARS: usize = 8_000;

/// The most characters of what the user added that a worker is given with its task, the
/// newest kept: the task is cut to make room for them, never the other way round.
pub(crate) const STEER_CHARS: usize = TASK_CHARS / 2;

/// The most renders the core remembers tracking, so a render's late events never open a
/// second job for it.
pub(crate) const RENDERS_KEPT: usize = 16;

/// The jobs of this run and the ones with a live worker.
#[derive(Debug, Default)]
pub(crate) struct Jobs {
    pub(crate) registry: Registry,
    pub(crate) live: BTreeMap<JobId, Live>,
    /// The app's render jobs tracked by app tasks, oldest first, at most [`RENDERS_KEPT`].
    pub(crate) renders: VecDeque<(String, JobId)>,
    /// How the children of each unfinished fan-out have ended so far.
    fanouts: BTreeMap<JobId, Tally>,
}

impl Jobs {
    /// The jobs a snapshot holds. None of them has a live worker in this run, and the core's
    /// first step interrupts the unfinished ones.
    pub(crate) fn resumed(all: Vec<Job>) -> Self {
        Self { registry: Registry::resumed(all), ..Self::default() }
    }

    /// Every job kept.
    pub(crate) fn all(&self) -> &[Job] {
        self.registry.jobs()
    }

    /// The job whose live worker is `session`.
    pub(crate) fn by_session(&self, session: WorkerSessionId) -> Option<(JobId, Live)> {
        let found = self.live.iter().find(|(_, live)| live.session == session);
        found.map(|(id, live)| (*id, live.clone()))
    }
}

/// A job's live worker session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Live {
    pub(crate) kind: JobKind,
    pub(crate) session: WorkerSessionId,
    /// The session whose folder the job works in: its own, or that of the job it was
    /// rerouted from, so a reroute keeps the working folder (section 2.5).
    pub(crate) folder: WorkerSessionId,
    pub(crate) bound: bool,
    /// The worker turns still to end: the bound task's and one per steer since. The job ends
    /// with the last; the turn a steer interrupted ends first.
    pub(crate) owed: u8,
    /// What the user added before the session was ready, given with the task at the bind.
    pub(crate) steer: Option<String>,
    /// Everything the user has told this worker, at most [`STEER_CHARS`] with the newest
    /// kept, which a job that replaces it (a reroute) is given with the task.
    pub(crate) told: String,
}

/// How a fan-out's children have ended so far, kept beside the registry, so a finished child
/// dropped to make room still counts when the last one ends.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Tally {
    /// The children accepted, a rerouted agent's successor included.
    accepted: usize,
    completed: usize,
    cancelled: usize,
    /// The children that ended any other way.
    failed: usize,
    /// The source that cancelled the last child cancelled.
    last_cancel: Option<CancelSource>,
}

impl Tally {
    /// How many children have ended.
    const fn ended(&self) -> usize {
        self.completed.saturating_add(self.cancelled).saturating_add(self.failed)
    }
}

/// How a job ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum End {
    /// Its worker, or its app, finished.
    Completed {
        summary: Untrusted<String>,
    },
    /// Every child of a fan-out ended, and `finished` of its `total` workers completed.
    Gathered {
        finished: usize,
        total: usize,
    },
    Failed {
        why: String,
    },
    /// Eileen can no longer follow it: an app task whose app's bridge stopped.
    Lost {
        why: String,
    },
    Cancelled {
        by: CancelSource,
    },
}

/// A tool result holding one sentence.
pub(crate) fn text_result(
    text: String,
    is_error: bool,
    output: Option<serde_json::Value>,
) -> ToolResult {
    ToolResult {
        content: vec![Content::Text(Untrusted::new(text))],
        is_error,
        verification: None,
        output: output.map(Untrusted::new),
        duration_ms: 0,
        rung: None,
    }
}

/// A refusal holding one sentence.
pub(crate) fn refusal(text: &str) -> ToolResult {
    text_result(text.to_owned(), true, None)
}

/// Whether the brain may dispatch a job of `kind`: the core's own kinds (act, app task,
/// librarian, replay) and the reserved one are not dispatched.
const fn dispatchable(kind: JobKind) -> bool {
    matches!(
        kind,
        JobKind::Coding
            | JobKind::ComputerUse
            | JobKind::General
            | JobKind::Research
            | JobKind::DeepAnswer
            | JobKind::ContinueSession
    )
}

/// At most `max` characters of `text`.
pub(crate) fn cut(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

/// A job a worker is to run: what it asks for, its name, the session whose folder it works in
/// when not its own (a reroute's), and whether it takes the focus.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct NewWorker {
    pub(crate) spec: JobSpec,
    pub(crate) naming: Naming,
    pub(crate) folder: Option<WorkerSessionId>,
    pub(crate) focus: bool,
    /// What the user told the job it replaces, given with the task at the bind.
    pub(crate) told: Option<String>,
    /// What the job it replaces had read (section 2.12): its folder holds what that job
    /// fetched and wrote, so the job that works there starts as tainted.
    pub(crate) taint: Taint,
}

impl NewWorker {
    /// A job that replaces none, in its own folder.
    fn fresh(spec: JobSpec, naming: Naming, focus: bool) -> Self {
        Self { spec, naming, folder: None, focus, told: None, taint: Taint::default() }
    }
}

/// The result of a dispatch that accepted `jobs`, the first being the one job or the fan-out's
/// parent: a sentence, and the structured output with the ack the brain says.
fn started(jobs: &[Job]) -> Option<ToolResult> {
    let first = jobs.first()?;
    let ack = if first.spec.fanout > 1 {
        let count = lifecycle::ORDINALS.get(usize::from(first.spec.fanout).saturating_sub(1));
        let count = count.copied().unwrap_or("several");
        format!("Starting {count} {} workers on {}.", first.route.display_name(), first.name)
    } else {
        format!("Starting {}.", first.name)
    };
    let output = JobsDispatchOutput {
        jobs: jobs
            .iter()
            .map(|job| DispatchedJob {
                job: job.id,
                name: job.name.clone(),
                ordinal: job.ordinal,
                queued: false,
            })
            .collect(),
        ack,
    };
    let text = format!("Started {} ({}).", first.name, first.id);
    Some(text_result(text, false, serde_json::to_value(output).ok()))
}

/// A new job's row, `queued` and not yet the focus.
pub(crate) fn new_job(
    id: JobId,
    naming: Naming,
    spec: JobSpec,
    routed: &Routed,
    session: Option<WorkerSessionId>,
    at: WallTime,
) -> Job {
    Job {
        id,
        name: naming.name,
        slug: naming.slug,
        ordinal: naming.ordinal,
        spec,
        status: JobStatus::Queued,
        route: routed.route,
        model: routed.model.clone(),
        session,
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
        override_words: routed.words.clone(),
        approval_deadline: None,
    }
}

impl Core {
    /// A tool call through the dispatcher. The core serves `jobs_dispatch` and `jobs_control`
    /// from an addressed brain turn and refuses everything else with a sentence.
    pub(crate) fn tool_call(&mut self, cx: &mut Cx<'_>, ctx: TraceCtx, request: ToolCallRequest) {
        let call = ctx.call.unwrap_or_else(|| CallId::from_parts(cx.mint(), cx.random()));
        let turn = match &request.origin {
            CallOrigin::Brain { turn, .. } | CallOrigin::FastPath { turn } => Some(*turn),
            CallOrigin::Worker { .. }
            | CallOrigin::Replay { .. }
            | CallOrigin::Session
            | CallOrigin::Core => None,
        };
        let outcome = match request.tool.as_str() {
            "jobs_dispatch" => self.dispatch(cx, &request, call),
            "jobs_control" => self.control(cx, &request),
            tool => Err(text_result(format!("{tool} is not available yet."), true, None)),
        };
        let result = match outcome {
            Ok(result) | Err(result) => result,
        };
        let reply_ctx = TraceCtx { turn, job: None, call: Some(call) };
        let caller: Caller = request.caller;
        cx.push(reply_ctx, Effect::ToolReply { caller, call, result });
    }

    /// The brain turn in flight that `request` comes from, when the brain's own shim sent it
    /// for that turn and the turn is addressed; otherwise an error result, with `ambient` the
    /// sentence for an ambient turn. The turn is the core's own record, never what the
    /// request claims.
    pub(crate) fn acting_turn(
        &self,
        request: &ToolCallRequest,
        ambient: &str,
    ) -> Result<TurnId, ToolResult> {
        let from_brain = matches!(request.caller, Caller::Shim { role: ShimRole::Brain, .. });
        let CallOrigin::Brain { turn, .. } = &request.origin else {
            return Err(refusal(NOT_THE_CONVERSATION));
        };
        if !from_brain {
            return Err(refusal(NOT_THE_CONVERSATION));
        }
        let in_flight = self.brain.live_turn().filter(|(live, _)| live == turn);
        let Some((turn, authority)) = in_flight else {
            return Err(refusal("That request was cancelled or has ended."));
        };
        if authority == TurnAuthority::Ambient {
            return Err(refusal(ambient));
        }
        Ok(turn)
    }

    /// `jobs_dispatch`: one job or a fan-out, or an error result saying why not. A kind v0.1
    /// does not run is refused before routing and admission: its `UserError` goes on the bus
    /// for the HUD, and its sentence is the error result's text. The result carries no
    /// structured output, since `output` must match the op's output schema,
    /// `JobsDispatchOutput`.
    fn dispatch(
        &mut self,
        cx: &mut Cx<'_>,
        request: &ToolCallRequest,
        call: CallId,
    ) -> Result<ToolResult, ToolResult> {
        let turn =
            self.acting_turn(request, "A job can only start from something the user asked.")?;
        let args: JobsDispatch = serde_json::from_value(request.args.get().clone())
            .map_err(|_| refusal("That job request was not valid."))?;
        if !args.kind.built_in_v0_1() {
            let error = user_error::job_kind_unavailable();
            let sentence = error.sentence.clone();
            let ctx = TraceCtx { turn: Some(turn), job: None, call: Some(call) };
            cx.emit(ctx, BusEvent::UserError(error));
            return Err(text_result(sentence, true, None));
        }
        if !dispatchable(args.kind) {
            return Err(refusal("I can't start a job of that kind."));
        }
        let fanout = args.fanout.unwrap_or(1);
        if fanout == 0 || fanout > FANOUT_MAX {
            return Err(refusal("A fan-out takes one to eight agents."));
        }
        let mode = args.mode.unwrap_or(FanoutMode::Compete);
        if fanout > 1 && mode == FanoutMode::Split && args.subtasks.len() != usize::from(fanout) {
            return Err(refusal("Give one sub-task for each agent."));
        }
        if self.jobs.live.len().saturating_add(usize::from(fanout)) > MAX_LIVE {
            return Err(refusal(BUSY));
        }
        let open = self.jobs.all().iter().filter(|job| !job.status.is_terminal()).count();
        let room = if fanout > 1 { usize::from(fanout).saturating_add(1) } else { 1 };
        if open.saturating_add(room) > lifecycle::KEPT {
            return Err(refusal(FULL));
        }
        // A depth the brain passes is its own choice unless the user said it (section 2.3).
        let ask = Ask {
            route: args.route,
            model: args.model.clone(),
            depth: args.depth,
            depth_said: false,
        };
        let routed = self.route_for(args.kind, &ask).map_err(refusal)?;
        let spec = JobSpec {
            kind: args.kind,
            task: cut(&args.task, TASK_CHARS),
            title_hint: args.title_hint,
            depth: routed.depth,
            route: args.route,
            model: args.model,
            fanout,
            mode,
            subtasks: args.subtasks,
            subquestions: args.subquestions,
            repo: args.repo.map(Personal::new),
            resume_session: None,
            skill: None,
            slots: BTreeMap::new(),
            topic: args.topic,
            anticipated: args.anticipated,
            authorized_by: turn,
            parent: None,
            chrome: false,
        };
        let jobs = if fanout > 1 {
            self.fan_out(cx, &spec, &routed)
        } else {
            let hint = spec.title_hint.clone();
            let naming = self.jobs.registry.naming(hint.as_deref(), spec.kind, None, cx.at).ok();
            naming
                .and_then(|naming| {
                    self.start_worker(cx, NewWorker::fresh(spec, naming, true), &routed)
                })
                .map(|job| vec![job])
        };
        jobs.as_deref().and_then(started).ok_or_else(|| refusal(FULL))
    }

    /// A fan-out: its parent, which takes the focus, then one child per worker, each with its
    /// own session, the whole task in `compete` mode and its own sub-task in `split` mode.
    /// `dispatch` has already made sure the registry has room for the parent and every child,
    /// and the parent stays unfinished while they are accepted, so once the parent is kept
    /// no child is refused.
    fn fan_out(&mut self, cx: &mut Cx<'_>, spec: &JobSpec, routed: &Routed) -> Option<Vec<Job>> {
        let id = JobId::from_parts(cx.mint(), cx.random());
        let hint = spec.title_hint.as_deref();
        let naming = self.jobs.registry.naming(hint, spec.kind, None, cx.at).ok()?;
        let parent = new_job(id, naming, spec.clone(), routed, None, cx.at);
        let mut jobs =
            vec![self.admit(cx, parent, Some(spec.authorized_by), &routed.reason, true)?];
        for n in 0..usize::from(spec.fanout) {
            let task = match spec.mode {
                FanoutMode::Split => {
                    spec.subtasks.get(n).map_or_else(String::new, |t| cut(t, TASK_CHARS))
                }
                FanoutMode::Compete => spec.task.clone(),
            };
            let child =
                JobSpec { task, fanout: 1, subtasks: Vec::new(), parent: Some(id), ..spec.clone() };
            let naming = self.jobs.registry.naming(None, spec.kind, Some(id), cx.at).ok()?;
            jobs.push(self.start_worker(cx, NewWorker::fresh(child, naming, false), routed)?);
        }
        Some(jobs)
    }

    /// Accepts a job a worker runs and starts its cold session, in its own folder or in the
    /// folder `worker` names. Nothing when [`MAX_LIVE`] workers run already, the one place
    /// that bound is kept, or when the registry refuses the job.
    pub(crate) fn start_worker(
        &mut self,
        cx: &mut Cx<'_>,
        worker: NewWorker,
        routed: &Routed,
    ) -> Option<Job> {
        if self.jobs.live.len() >= MAX_LIVE {
            return None;
        }
        let NewWorker { spec, naming, folder, focus, told, taint } = worker;
        let id = JobId::from_parts(cx.mint(), cx.random());
        let session = WorkerSessionId::from_parts(cx.mint(), cx.random());
        let turn = spec.authorized_by;
        let mut job = new_job(id, naming, spec, routed, Some(session), cx.at);
        job.taint = taint;
        let job = self.admit(cx, job, Some(turn), &routed.reason, focus)?;
        let folder = folder.unwrap_or(session);
        self.spans.open_session(id, job.route);
        let spec = session_spec(session, &job, folder);
        cx.push(job_ctx(id), Effect::Worker(session, WorkerCmd::Start(Box::new(spec))));
        let kind = job.spec.kind;
        let said = told.clone().unwrap_or_default();
        let live = Live { kind, session, folder, bound: false, owed: 0, steer: told, told: said };
        self.jobs.live.insert(id, live);
        Some(job)
    }

    /// Keeps a new job, gives it the focus when `focus` says so, and announces it on the bus
    /// with its row and the rows whose focus moved. Nothing when the registry refuses it.
    pub(crate) fn admit(
        &mut self,
        cx: &mut Cx<'_>,
        job: Job,
        turn: Option<TurnId>,
        reason: &str,
        focus: bool,
    ) -> Option<Job> {
        let id = job.id;
        self.jobs.registry.accept(job).ok()?;
        let moved = if focus { self.jobs.registry.focus_on(id, cx.at) } else { Vec::new() };
        let job = self.jobs.registry.get(id)?.clone();
        if let Some(parent) = job.spec.parent {
            let tally = self.jobs.fanouts.entry(parent).or_default();
            tally.accepted = tally.accepted.saturating_add(1);
        }
        let ctx = TraceCtx { turn, job: Some(id), call: None };
        cx.emit(
            ctx,
            BusEvent::JobAccepted {
                job: id,
                name: job.name.clone(),
                ordinal: job.ordinal,
                kind: job.spec.kind,
                route: job.route,
                model: job.model.clone(),
                reason: reason.to_owned(),
            },
        );
        cx.push(ctx, Effect::Persist(Record::JobUpsert(Box::new(job.clone()))));
        self.persist_rows(cx, moved.into_iter().filter(|moved| *moved != id));
        self.spans.open_job(id, job.spec.authorized_by, job.spec.kind, job.route);
        Some(job)
    }

    /// Persists the rows of `jobs` as they now are.
    pub(crate) fn persist_rows(&self, cx: &mut Cx<'_>, jobs: impl IntoIterator<Item = JobId>) {
        for id in jobs {
            if let Some(job) = self.jobs.registry.get(id) {
                cx.push(job_ctx(id), Effect::Persist(Record::JobUpsert(Box::new(job.clone()))));
            }
        }
    }

    /// Moves the focus to `id`, with the rows whose focus changed.
    pub(crate) fn focus(&mut self, cx: &mut Cx<'_>, id: JobId) {
        let moved = self.jobs.registry.focus_on(id, cx.at);
        self.persist_rows(cx, moved);
    }

    /// Once the focus job finished, the focus moves on, with the rows whose focus changed.
    pub(crate) fn refocus(&mut self, cx: &mut Cx<'_>) {
        let moved = self.jobs.registry.refocus(cx.at);
        self.persist_rows(cx, moved);
    }
}

impl Core {
    /// The job reaches its terminal state, once (`finish`), then its fan-out's parent ends when
    /// it was the last child running, and the focus moves on.
    pub(crate) fn end_job(&mut self, cx: &mut Cx<'_>, id: JobId, end: End) {
        let Some(job) = self.finish(cx, id, end) else { return };
        if let Some(parent) = job.spec.parent {
            self.settle_parent(cx, parent);
        }
        self.refocus(cx);
    }

    /// The job reaches its terminal state, once: its event, its row, its session's end, the
    /// announcement of a completion (not of a fan-out child's, which its parent's covers), a
    /// failure or a loss, and its fan-out's tally. Returns the job as it ended; nothing when it
    /// cannot take that step from where it is.
    fn finish(&mut self, cx: &mut Cx<'_>, id: JobId, end: End) -> Option<Job> {
        let step = match &end {
            End::Completed { .. } | End::Gathered { .. } => Step::Complete,
            End::Failed { .. } | End::Lost { .. } => Step::Fail,
            End::Cancelled { .. } => Step::Cancel,
        };
        let taken = self.jobs.registry.step(id, step, cx.at).ok().cloned();
        // A job that cannot take this step from where it is stays live, so a later event of its
        // session still ends it; one already finished is live no longer.
        let finished = self.jobs.registry.get(id).is_none_or(|job| job.status.is_terminal());
        let live = if finished { self.jobs.live.remove(&id) } else { None };
        let job = taken?;
        if let Some(parent) = job.spec.parent {
            let tally = self.jobs.fanouts.entry(parent).or_default();
            match &end {
                End::Completed { .. } | End::Gathered { .. } => {
                    tally.completed = tally.completed.saturating_add(1);
                }
                End::Cancelled { by } => {
                    tally.cancelled = tally.cancelled.saturating_add(1);
                    tally.last_cancel = Some(*by);
                }
                End::Failed { .. } | End::Lost { .. } => {
                    tally.failed = tally.failed.saturating_add(1);
                }
            }
        }
        self.jobs.fanouts.remove(&id);
        let child = job.spec.parent.is_some();
        let route = job.route.display_name();
        let (event, spoken) = match end {
            End::Completed { summary } => {
                let spoken = (!child).then(|| announce::completed(&job.name, route, summary.get()));
                (BusEvent::JobCompleted { job: id, summary, artifacts: Vec::new() }, spoken)
            }
            End::Gathered { finished, total } => {
                let summary = Untrusted::new(format!("{finished} of {total} agents finished."));
                let spoken = announce::gathered(&job.name, finished, total);
                (BusEvent::JobCompleted { job: id, summary, artifacts: Vec::new() }, Some(spoken))
            }
            End::Failed { why } => {
                let spoken = announce::failed(&job.name, &why);
                (BusEvent::JobFailed { job: id, error: why }, Some(spoken))
            }
            End::Lost { why } => {
                let spoken = announce::lost(&job.name, &why);
                (BusEvent::JobFailed { job: id, error: why }, Some(spoken))
            }
            End::Cancelled { by } => (BusEvent::JobCancelled { job: id, by }, None),
        };
        let ctx = job_ctx(id);
        cx.emit(ctx, event);
        cx.push(ctx, Effect::Persist(Record::JobUpsert(Box::new(job.clone()))));
        if let Some(live) = live {
            cx.push(ctx, Effect::Worker(live.session, WorkerCmd::Cancel));
        }
        self.spans.close_job(id);
        if let Some(text) = spoken {
            self.announcements.push(Announcement { job: id, text });
        }
        Some(job)
    }

    /// A fan-out ends once every child it accepted has, counted from its tally rather than
    /// from the children still kept: completed when any child completed, cancelled when the
    /// user cancelled every child (by the source that cancelled the last), and failed
    /// otherwise.
    fn settle_parent(&mut self, cx: &mut Cx<'_>, parent: JobId) {
        let Some(of) = self.jobs.registry.get(parent).filter(|job| !job.status.is_terminal())
        else {
            return;
        };
        let total = usize::from(of.spec.fanout);
        let Some(tally) = self.jobs.fanouts.get(&parent).copied() else { return };
        if tally.ended() < tally.accepted {
            return;
        }
        let end = if tally.completed > 0 {
            End::Gathered { finished: tally.completed.min(total), total }
        } else if tally.cancelled == tally.ended() {
            End::Cancelled { by: tally.last_cancel.unwrap_or(CancelSource::Voice) }
        } else {
            End::Failed { why: "no agent finished".to_owned() }
        };
        self.end_job(cx, parent, end);
    }

    /// Cancels the job `id` by `by`. A fan-out's unfinished children are cancelled first, by
    /// their parent, and the parent then ends once, after its last child.
    pub(crate) fn cancel(&mut self, cx: &mut Cx<'_>, id: JobId, by: CancelSource) {
        let children: Vec<JobId> = self
            .jobs
            .all()
            .iter()
            .filter(|job| job.spec.parent == Some(id) && !job.status.is_terminal())
            .map(|job| job.id)
            .collect();
        for child in children {
            self.finish(cx, child, End::Cancelled { by: CancelSource::Parent });
        }
        self.end_job(cx, id, End::Cancelled { by });
    }

    /// The first step after a restart: every job the snapshot held unfinished is interrupted,
    /// with its event and its row (invariant 12).
    pub(crate) fn interrupt_unfinished(&mut self, cx: &mut Cx<'_>) {
        for id in self.jobs.registry.restart(cx.at) {
            let Some(job) = self.jobs.registry.get(id) else { continue };
            let row = Record::JobUpsert(Box::new(job.clone()));
            cx.emit(job_ctx(id), BusEvent::JobInterrupted { job: id });
            cx.push(job_ctx(id), Effect::Persist(row));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::cut;

    #[test]
    fn a_task_is_cut_at_a_character_not_a_byte() {
        assert_eq!(cut("héllo", 2), "hé", "two characters");
        assert_eq!(cut("hi", 10), "hi", "shorter stays");
    }
}
