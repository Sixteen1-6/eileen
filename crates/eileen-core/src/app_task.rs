//! App tasks (BUILD.md section 2.5, "App tasks"): a job the core runs with no Claude session,
//! tracking work an app does on its own. While the apps sidecar's bridge to Resolve is up it
//! reports a render it did not start for a job (`AppEvent::RenderStarted`), then its progress
//! and its end; the core opens a job named "the render" for it, running at once, turns each
//! progress report into `JobProgressed` with the percent, and ends it completed or failed with
//! the usual announcement. The core stays sans-IO and only reacts.
//!
//! A render the user started in the app has no turn behind it: its job's `authorized_by` is
//! the nil turn, it carries no turn on the bus, and it takes the focus only when no other job is
//! open. Cancelling it stops the tracking, not the render, since the adapter has no op to stop
//! one. When the apps sidecar exits, every render it reported ends lost, since no `RenderDone`
//! can come for it, and a render still running when the sidecar is back opens a job of its own.
//! At most `RENDERS_KEPT` renders are remembered: the oldest whose job has ended is forgotten
//! first, and while every one remembered still runs no new render is tracked.
//!
//! FIXME(#3): a render started through Eileen (`apps_resolve` `render_start` from the brain's
//! `act` tools) opens its job from the call's `render_job` once the core forwards tool calls
//! to connectors; until then only the sidecar's report opens one.

use std::collections::BTreeMap;

use eileen_contracts::bus::BusEvent;
use eileen_contracts::connectors::AppEvent;
use eileen_contracts::ids::{JobId, TurnId, Untrusted};
use eileen_contracts::jobs::{Depth, FanoutMode, JobKind, JobSpec, Route};

use crate::job::{End, RENDERS_KEPT, new_job};
use crate::lifecycle::Step;
use crate::router::Routed;
use crate::{Core, Cx, job_ctx};

/// The turn an app task the user started in the app is authorized by: none.
const NO_TURN: TurnId = TurnId::from_parts(0, 0);

/// Why a tracked render ended when the apps sidecar stopped.
const BRIDGE_STOPPED: &str = "the app's bridge stopped";

impl Core {
    /// An app's own event, from the apps sidecar.
    pub(crate) fn app_event(&mut self, cx: &mut Cx<'_>, app: &str, event: AppEvent) {
        match event {
            AppEvent::RenderStarted { render_job } => self.render_started(cx, app, render_job),
            AppEvent::RenderProgress { render_job, pct } => {
                let Some(id) = self.render_job(&render_job) else { return };
                let Some(job) = self.jobs.registry.progress(id, pct, cx.at).cloned() else {
                    return;
                };
                let pct = job.progress_pct;
                let line = Untrusted::new(format!("{} percent", pct.unwrap_or(0)));
                cx.emit(job_ctx(id), BusEvent::JobProgressed { job: id, line, pct });
                self.persist_rows(cx, [id]);
            }
            AppEvent::RenderDone { render_job, ok } => {
                let Some(id) = self.render_job(&render_job) else { return };
                let end = if ok {
                    End::Completed { summary: Untrusted::new(String::new()) }
                } else {
                    End::Failed { why: format!("{app} reported that the render failed") }
                };
                self.end_job(cx, id, end);
            }
            AppEvent::VersionChanged { .. } | AppEvent::Other(_) => {}
        }
    }

    /// The job tracking the app's render `render_job`, if one is.
    fn render_job(&self, render_job: &str) -> Option<JobId> {
        let tracked = self.jobs.renders.iter().find(|(known, _)| known == render_job);
        tracked.map(|(_, id)| *id)
    }

    /// The apps sidecar exited: each render it reported that is still tracked ends lost, and
    /// none is remembered, so one still running when the sidecar is back opens a new job.
    pub(crate) fn apps_exited(&mut self, cx: &mut Cx<'_>) {
        let tracked: Vec<JobId> = self.jobs.renders.drain(..).map(|(_, id)| id).collect();
        for id in tracked {
            self.end_job(cx, id, End::Lost { why: BRIDGE_STOPPED.to_owned() });
        }
    }

    /// A render no job tracks yet: a running app task named "the render", with room made among
    /// the renders remembered first.
    fn render_started(&mut self, cx: &mut Cx<'_>, app: &str, render_job: String) {
        if self.render_job(&render_job).is_some() {
            return;
        }
        if self.jobs.renders.len() >= RENDERS_KEPT {
            let registry = &self.jobs.registry;
            let ended = |id: &JobId| registry.get(*id).is_none_or(|job| job.status.is_terminal());
            let Some(oldest) = self.jobs.renders.iter().position(|(_, id)| ended(id)) else {
                return;
            };
            self.jobs.renders.remove(oldest);
        }
        let Ok(naming) = self.jobs.registry.naming(Some("render"), JobKind::AppTask, None, cx.at)
        else {
            return;
        };
        let spec = JobSpec {
            kind: JobKind::AppTask,
            task: format!("Track the {app} render {render_job}"),
            title_hint: Some("render".to_owned()),
            depth: Depth::Instant,
            route: None,
            model: None,
            fanout: 1,
            mode: FanoutMode::Compete,
            subtasks: Vec::new(),
            subquestions: Vec::new(),
            repo: None,
            resume_session: None,
            skill: None,
            slots: BTreeMap::new(),
            topic: None,
            anticipated: false,
            authorized_by: NO_TURN,
            parent: None,
            chrome: false,
        };
        // The row needs a route; no model runs an app task.
        let routed = Routed {
            route: Route::Claude,
            model: String::new(),
            depth: Depth::Instant,
            reason: format!("{app} reported a render"),
            words: None,
        };
        let id = JobId::from_parts(cx.mint(), cx.random());
        let job = new_job(id, naming, spec, &routed, None, cx.at);
        if self.admit(cx, job, None, &routed.reason, false).is_none() {
            return;
        }
        self.jobs.renders.push_back((render_job, id));
        if self.jobs.registry.step(id, Step::Start, cx.at).is_ok() {
            cx.emit(job_ctx(id), BusEvent::JobStarted { job: id, warm: false });
            self.persist_rows(cx, [id]);
        }
        self.refocus(cx);
    }
}
