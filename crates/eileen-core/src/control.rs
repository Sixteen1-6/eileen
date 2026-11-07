//! Changing a job the user names (BUILD.md section 2.5): `jobs_control` from the brain's turn in
//! flight steers, reroutes, focuses or cancels it, and a spoken "cancel" names it on the fast
//! path. Which job a request names is `lifecycle::Registry::pick`: an explicit name, slug or
//! ordinal, a kind word, a route word, then for a pronoun the job announced just before, the
//! focus job and the most recent. Several fitting jobs with none in focus mean one question,
//! and a name that fits none is said so.
//!
//! Steer is an interrupt followed by a new user turn with the words, sent to the job's bound
//! session; before the session is ready the words wait and go with the task. Each worker
//! keeps what the user told it. Reroute ends the old worker's session, starts a new job with
//! the same task, name, working folder and taint, and with what the user told the old worker,
//! on the route, model or depth asked for, then cancels the old job. Both are for a job a
//! worker runs, and a fan-out's parent passes a steer to each of its running workers. However
//! a render is cancelled, Eileen says the app is still running it.

use eileen_contracts::bus::{CancelSource, SpeakCause};
use eileen_contracts::core_io::{Effect, ToolCallRequest, ToolResult};
use eileen_contracts::harness::WorkerCmd;
use eileen_contracts::ids::{JobId, MonoTime, TurnId, WorkerSessionId};
use eileen_contracts::jobs::{JobKind, JobSpec};
use eileen_contracts::tools::brain::{JobAction, JobRef, JobsControl};

use crate::job::{FULL, NewWorker, STEER_CHARS, TASK_CHARS, cut, refusal, text_result};
use crate::lifecycle::{KEPT, Pick};
use crate::router::{Ask, Routed};
use crate::speech::Owner;
use crate::{Core, Cx, job_ctx};

/// What Eileen says when the words name no unfinished job.
pub(crate) const NOT_FOUND: &str = "I can't find that job.";

/// What Eileen says when she stops tracking an app's own work, which she cannot stop for it.
const APP_LEFT: &str = "I stopped tracking that; the app is still running it.";

/// Why a steer with no words was refused.
const SAY_WHAT: &str = "Say what to tell it.";

/// Why a reroute of a job no worker runs (a fan-out's parent, an app's render) was refused.
const NO_WORKER: &str = "Only a job a worker runs can move to another model.";

/// Why a reroute that asks for no route, model or depth was refused.
const SAY_WHERE: &str = "Say which model or route to move it to.";

/// Why `show_changes`, `keep`, `undo` and `interrupt` are refused until they are built.
const NOT_YET: &str = "I can't do that to a job yet.";

/// The most turns one worker may owe, its bound task's included: past it a steer is refused
/// with a sentence until the worker catches up.
const STEERS_OWED: u8 = 8;

/// `old` and then `new` on a line of its own, keeping the newest `STEER_CHARS` characters.
fn added(old: &str, new: &str) -> String {
    let joined = if old.is_empty() { new.to_owned() } else { format!("{old}\n{new}") };
    let count = joined.chars().count();
    joined.chars().skip(count.saturating_sub(STEER_CHARS)).collect()
}

/// Names as said aloud: "a", "a and b", "a, b and c".
fn spoken_list(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

impl Core {
    /// `jobs_control`: what the brain asks to do to a job the user named, or an error result
    /// with a sentence the brain can say.
    pub(crate) fn control(
        &mut self,
        cx: &mut Cx<'_>,
        request: &ToolCallRequest,
    ) -> Result<ToolResult, ToolResult> {
        let turn = self.acting_turn(request, "Only something the user asked can change a job.")?;
        let args: JobsControl = serde_json::from_value(request.args.get().clone())
            .map_err(|_| refusal("That job request was not valid."))?;
        let id = match self.jobs.registry.pick(Some(&args.job), self.announced(cx.now)) {
            Pick::One(id) => id,
            Pick::Ask(ids) => return Err(text_result(self.which_one(&ids), true, None)),
            Pick::Nothing => return Err(refusal(NOT_FOUND)),
        };
        let name = self.jobs.registry.get(id).map(|job| job.name.clone()).unwrap_or_default();
        match args.action {
            JobAction::Cancel => {
                let said = self.cancel_named(cx, id, CancelSource::Voice);
                let said = said.map_or_else(|| format!("Cancelled {name}."), str::to_owned);
                Ok(text_result(said, false, None))
            }
            JobAction::Focus => {
                self.focus(cx, id);
                Ok(text_result(format!("Following {name} now."), false, None))
            }
            JobAction::Steer { text } => self.steer(cx, id, &text),
            JobAction::Reroute { route, model, depth } => {
                self.reroute(cx, turn, id, Ask { route, model, depth, depth_said: true })
            }
            JobAction::Interrupt | JobAction::ShowChanges | JobAction::Keep | JobAction::Undo => {
                Err(refusal(NOT_YET))
            }
        }
    }

    /// Cancels the job `id` by `by`. For an app's own work, which Eileen only tracked, what
    /// she says about it: that the app is still running it.
    fn cancel_named(
        &mut self,
        cx: &mut Cx<'_>,
        id: JobId,
        by: CancelSource,
    ) -> Option<&'static str> {
        let app = self.jobs.registry.get(id).is_some_and(|job| job.spec.kind == JobKind::AppTask);
        self.cancel(cx, id, by);
        app.then_some(APP_LEFT)
    }

    /// Steers the job `id`, or each running worker of a fan-out: an interrupt and the words as
    /// a new user turn for a bound session, the words kept for the bind otherwise, and the
    /// words recorded on each worker told. A worker that already owes `STEERS_OWED` turns is
    /// told nothing: the brain hears why when no worker was told, and which workers are still
    /// catching up when only some were. The job takes the focus.
    fn steer(&mut self, cx: &mut Cx<'_>, id: JobId, text: &str) -> Result<ToolResult, ToolResult> {
        let text = cut(text.trim(), TASK_CHARS);
        if text.is_empty() {
            return Err(refusal(SAY_WHAT));
        }
        let workers: Vec<JobId> = if self.jobs.live.contains_key(&id) {
            vec![id]
        } else {
            let children = self.jobs.all().iter().filter(|job| job.spec.parent == Some(id));
            children.map(|job| job.id).filter(|child| self.jobs.live.contains_key(child)).collect()
        };
        let name = self.jobs.registry.get(id).map(|job| job.name.clone()).unwrap_or_default();
        if workers.is_empty() {
            return Err(text_result(format!("{name} has no worker I can tell."), true, None));
        }
        let mut sent: Vec<(JobId, WorkerSessionId)> = Vec::new();
        let mut told_any = false;
        let mut behind: Vec<JobId> = Vec::new();
        for worker in workers {
            let Some(live) = self.jobs.live.get_mut(&worker) else { continue };
            if live.bound && live.owed >= STEERS_OWED {
                behind.push(worker);
                continue;
            }
            if live.bound {
                live.owed = live.owed.saturating_add(1);
                sent.push((worker, live.session));
            } else {
                live.steer = Some(added(live.steer.as_deref().unwrap_or_default(), &text));
            }
            live.told = added(&live.told, &text);
            told_any = true;
        }
        if !told_any {
            let sentence = format!("{name} is still catching up; tell it again in a moment.");
            return Err(text_result(sentence, true, None));
        }
        for (worker, session) in sent {
            cx.push(job_ctx(worker), Effect::Worker(session, WorkerCmd::Interrupt));
            let prompt = WorkerCmd::Prompt { text: text.clone() };
            cx.push(job_ctx(worker), Effect::Worker(session, prompt));
        }
        self.focus(cx, id);
        let behind: Vec<String> = behind
            .iter()
            .filter_map(|worker| self.jobs.registry.get(*worker))
            .map(|job| job.name.clone())
            .collect();
        let said = match behind.as_slice() {
            [] => format!("Told {name}."),
            [one] => format!("Told {name}; {one} is still catching up."),
            many => format!("Told {name}; {} are still catching up.", spoken_list(many)),
        };
        Ok(text_result(said, false, None))
    }

    /// Reroutes the job `id`: the old worker's session ends, then a new job starts with its
    /// task, name, ordinal, working folder and taint, and with what the user told the old
    /// worker, on the route, model or depth asked for and recorded with the user's words; then
    /// the old job is cancelled. The old session ends first, so no more than `MAX_LIVE`
    /// workers run even for a moment, and the old job ends last, so a fan-out's parent never
    /// sees all its children end in between. The new job takes the focus.
    ///
    /// FIXME(#3): the old transcript is not attached yet: sessions run with no session
    /// persistence, so there is nothing to resume, and the harness keeps no transcript to
    /// hand over. The new worker has the task, what the user told the old one, and the files
    /// the old one left in its folder.
    fn reroute(
        &mut self,
        cx: &mut Cx<'_>,
        turn: TurnId,
        id: JobId,
        ask: Ask,
    ) -> Result<ToolResult, ToolResult> {
        let (Some(old), Some(live)) =
            (self.jobs.registry.get(id).cloned(), self.jobs.live.get(&id).cloned())
        else {
            return Err(refusal(NO_WORKER));
        };
        if ask.route.is_none() && ask.model.is_none() && ask.depth.is_none() {
            return Err(refusal(SAY_WHERE));
        }
        let routed = self.route_for(old.spec.kind, &ask).map_err(refusal)?;
        let naming = self.jobs.registry.successor(id, cx.at).map_err(|_| refusal(NOT_FOUND))?;
        let open = self.jobs.all().iter().filter(|job| !job.status.is_terminal()).count();
        if open >= KEPT {
            return Err(refusal(FULL));
        }
        let Ask { route, model, .. } = ask;
        let spec =
            JobSpec { depth: routed.depth, route, model, authorized_by: turn, ..old.spec.clone() };
        let reason = format!("rerouted from {}: {}", old.slug, routed.reason);
        let routed = Routed { reason, ..routed };
        self.jobs.live.remove(&id);
        cx.push(job_ctx(id), Effect::Worker(live.session, WorkerCmd::Cancel));
        let told = (!live.told.is_empty()).then_some(live.told);
        let worker = NewWorker {
            spec,
            naming,
            folder: Some(live.folder),
            focus: true,
            told,
            taint: old.taint.clone(),
        };
        let started = self.start_worker(cx, worker, &routed);
        self.cancel(cx, id, CancelSource::Voice);
        // The room was checked above and the old session freed a worker's place, so the new
        // job starts; were it refused, the old one has stopped, and the brain hears so.
        let job = started.ok_or_else(|| {
            text_result(format!("I couldn't move {}, so it has stopped.", old.name), true, None)
        })?;
        let text = format!("Moved {} to the {} model.", job.name, job.model);
        Ok(text_result(text, false, None))
    }

    /// A spoken "cancel", with the words naming the job or none. Eileen says why when the
    /// words name no job, asks which when several fit, and says that an app's own work goes
    /// on when she only tracked it; a bare "cancel" with no job open says nothing.
    pub(crate) fn cancel_spoken(&mut self, cx: &mut Cx<'_>, turn: TurnId, target: Option<&JobRef>) {
        let line = match self.jobs.registry.pick(target, self.announced(cx.now)) {
            Pick::One(id) => self.cancel_named(cx, id, CancelSource::Voice).map(str::to_owned),
            Pick::Ask(ids) => Some(self.which_one(&ids)),
            Pick::Nothing => target.map(|_| NOT_FOUND.to_owned()),
        };
        if let Some(line) = line {
            self.say(cx, Owner::turn(turn), SpeakCause::Reply, &line);
        }
    }

    /// The job Eileen announced last, while "it" can still mean it: no reply since, and
    /// within the follow-up window of the announcement's end.
    pub(crate) fn announced(&self, now: MonoTime) -> Option<JobId> {
        let last = self.last_spoken.as_ref()?;
        let window = u64::from(self.config.config.conversation.follow_up_s);
        let fresh = now.0.saturating_sub(last.at.0) <= window.saturating_mul(1_000_000_000);
        last.job.filter(|_| fresh)
    }

    /// The one question for several fitting jobs, naming the newest three.
    fn which_one(&self, ids: &[JobId]) -> String {
        let names: Vec<&str> = ids
            .iter()
            .take(3)
            .filter_map(|id| self.jobs.registry.get(*id))
            .map(|job| job.name.as_str())
            .collect();
        match names.as_slice() {
            [one, two] if one != two => format!("Do you mean {one} or {two}?"),
            [one, two, three] => format!("Do you mean {one}, {two} or {three}?"),
            [one, ..] => format!("Which of the jobs called {one} do you mean?"),
            [] => "Which job do you mean?".to_owned(),
        }
    }
}
