//! Changing a job the user names (BUILD.md section 2.5): `jobs_control` from the brain's turn in
//! flight steers, reroutes, focuses or cancels it, and a spoken "cancel" names it on the fast
//! path. Which job a request names is `lifecycle::Registry::pick`: an explicit name, slug or
//! ordinal, a kind word, a route word, then for a pronoun the job announced just before, the
//! focus job and the most recent. Several fitting jobs with none in focus mean one question,
//! and a name that fits none is said so.
//!
//! Steer is an interrupt followed by a new user turn with the words, sent to the job's bound
//! session; before the session is ready the words wait and go with the task. Reroute starts a
//! new job with the same task, name and working folder on the route, model or depth asked for,
//! then cancels the old one. Both are for a job a worker runs, and a fan-out's parent passes a
//! steer to each of its running workers.

use eileen_contracts::bus::{CancelSource, SpeakCause};
use eileen_contracts::core_io::{Effect, ToolCallRequest, ToolResult};
use eileen_contracts::harness::WorkerCmd;
use eileen_contracts::ids::{JobId, MonoTime, TurnId, WorkerSessionId};
use eileen_contracts::jobs::{Depth, JobKind, JobSpec, Route};
use eileen_contracts::tools::brain::{JobAction, JobRef, JobsControl};

use crate::job::{NewWorker, Routed, TASK_CHARS, cut, refusal, text_result};
use crate::lifecycle::Pick;
use crate::speech::Owner;
use crate::{Core, Cx, job_ctx};

/// What Eileen says when the words name no unfinished job.
pub(crate) const NOT_FOUND: &str = "I can't find that job.";

/// What Eileen says when she stops tracking an app's own work, which she cannot stop for it.
const APP_LEFT: &str = "I stopped tracking that; the app is still running it.";

/// The route, model and depth a reroute asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Asked {
    route: Option<Route>,
    model: Option<String>,
    depth: Option<Depth>,
}

/// The most turns one worker may owe, its bound task's included: past it a steer is refused
/// with a sentence until the worker catches up.
const STEERS_OWED: u8 = 8;

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
                self.cancel(cx, id, CancelSource::Voice);
                Ok(text_result(format!("Cancelled {name}."), false, None))
            }
            JobAction::Focus => {
                self.focus(cx, id);
                Ok(text_result(format!("Following {name} now."), false, None))
            }
            JobAction::Steer { text } => self.steer(cx, id, &text),
            JobAction::Reroute { route, model, depth } => {
                self.reroute(cx, turn, id, Asked { route, model, depth })
            }
            JobAction::Interrupt | JobAction::ShowChanges | JobAction::Keep | JobAction::Undo => {
                Err(refusal("I can't do that to a job yet."))
            }
        }
    }

    /// Steers the job `id`, or each running worker of a fan-out: an interrupt and the words as
    /// a new user turn for a bound session, the words kept for the bind otherwise. A worker
    /// that already owes `STEERS_OWED` turns is told nothing, and when no worker was told the
    /// brain hears why. The job takes the focus.
    fn steer(&mut self, cx: &mut Cx<'_>, id: JobId, text: &str) -> Result<ToolResult, ToolResult> {
        let text = cut(text.trim(), TASK_CHARS);
        if text.is_empty() {
            return Err(refusal("Say what to tell it."));
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
        let mut kept_for_bind = false;
        for worker in workers {
            let Some(live) = self.jobs.live.get_mut(&worker) else { continue };
            if live.bound {
                if live.owed < STEERS_OWED {
                    live.owed = live.owed.saturating_add(1);
                    sent.push((worker, live.session));
                }
            } else {
                let kept =
                    live.steer.take().map_or_else(|| text.clone(), |old| format!("{old}\n{text}"));
                live.steer = Some(cut(&kept, TASK_CHARS));
                kept_for_bind = true;
            }
        }
        if sent.is_empty() && !kept_for_bind {
            let sentence = format!("{name} is still catching up; tell it again in a moment.");
            return Err(text_result(sentence, true, None));
        }
        for (worker, session) in sent {
            cx.push(job_ctx(worker), Effect::Worker(session, WorkerCmd::Interrupt));
            let prompt = WorkerCmd::Prompt { text: text.clone() };
            cx.push(job_ctx(worker), Effect::Worker(session, prompt));
        }
        self.focus(cx, id);
        Ok(text_result(format!("Told {name}."), false, None))
    }

    /// Reroutes the job `id`: a new job with its task, name, ordinal and working folder on the
    /// route, model or depth asked for, recorded with the user's words, then the old job
    /// cancelled. The new job takes the focus.
    ///
    /// FIXME(#3): the old transcript is not attached yet: sessions run with no session
    /// persistence, so there is nothing to resume, and the harness keeps no transcript to
    /// hand over. The new worker has the task and the files the old one left in its folder.
    fn reroute(
        &mut self,
        cx: &mut Cx<'_>,
        turn: TurnId,
        id: JobId,
        asked: Asked,
    ) -> Result<ToolResult, ToolResult> {
        let Asked { route, model, depth } = asked;
        let (Some(old), Some(live)) =
            (self.jobs.registry.get(id).cloned(), self.jobs.live.get(&id).cloned())
        else {
            return Err(refusal("Only a job a worker runs can move to another model."));
        };
        if route.is_none() && model.is_none() && depth.is_none() {
            return Err(refusal("Say which model or route to move it to."));
        }
        let routed = self.route_for(old.spec.kind, route, model.clone(), depth).map_err(refusal)?;
        let naming = self.jobs.registry.successor(id, cx.at).map_err(|_| refusal(NOT_FOUND))?;
        let spec =
            JobSpec { depth: routed.depth, route, model, authorized_by: turn, ..old.spec.clone() };
        let reason = format!("rerouted from {}: {}", old.slug, routed.reason);
        let routed = Routed { reason, ..routed };
        let worker = NewWorker { spec, naming, folder: Some(live.folder), focus: true };
        let job = self
            .start_worker(cx, worker, &routed)
            .ok_or_else(|| refusal("Too many jobs are open; cancel one and ask again."))?;
        self.cancel(cx, id, CancelSource::Voice);
        let text = format!("Moved {} to the {} model.", job.name, job.model);
        Ok(text_result(text, false, None))
    }

    /// A spoken "cancel", with the words naming the job or none. Eileen says why when the
    /// words name no job, asks which when several fit, and says that an app's own work goes
    /// on when she only tracked it; a bare "cancel" with no job open says nothing.
    pub(crate) fn cancel_spoken(&mut self, cx: &mut Cx<'_>, turn: TurnId, target: Option<&JobRef>) {
        let line = match self.jobs.registry.pick(target, self.announced(cx.now)) {
            Pick::One(id) => {
                let kind = self.jobs.registry.get(id).map(|job| job.spec.kind);
                self.cancel(cx, id, CancelSource::Voice);
                (kind == Some(JobKind::AppTask)).then(|| APP_LEFT.to_owned())
            }
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
