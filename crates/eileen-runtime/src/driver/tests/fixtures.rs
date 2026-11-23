//! The fixtures of the driver's tests: one effect of each family and the values inside them.

use eileen_contracts::bus::BusEvent;
use eileen_contracts::config::SettingsCmd;
use eileen_contracts::connectors::{SidecarCmd, SidecarId};
use eileen_contracts::core_io::{
    AudioCmd, Caller, Effect, EffectFamily, IncidentTrigger, Record, TimerId, Toast, ToolResult,
};
use eileen_contracts::harness::{BrainCmd, BrainSlot, HookDecision, HookReply, WorkerCmd};
use eileen_contracts::hud::{Corner, HudLayout, HudState, Indicators, MicState};
use eileen_contracts::ids::{CallId, ConnectorId, SpeakId, ToolName, WorkerSessionId};
use eileen_contracts::jobs::PlanUsage;
use eileen_contracts::learn::LearnCmd;
use eileen_contracts::speech::SpeechCmd;
use serde_json::Value;

const fn usage() -> PlanUsage {
    PlanUsage { five_hour_pct: Some(12), seven_day_pct: None, resets_at: None }
}

pub(super) fn hud() -> HudState {
    HudState {
        v: 1,
        layout: HudLayout {
            corner: Corner::TopRight,
            scale_pct: 100,
            opacity_pct: 90,
            show_transcript: true,
            high_contrast: false,
        },
        mic: MicState::Listening,
        conversation: None,
        heard: None,
        speaking: None,
        jobs: Vec::new(),
        approvals: Vec::new(),
        intent: None,
        input_lock: None,
        lease: None,
        watch: None,
        panel: None,
        suggestions: Vec::new(),
        first_run: None,
        indicators: Indicators::default(),
    }
}

/// One effect of `family`. The match is exhaustive, so a new family fails to build here until
/// this test covers it.
pub(super) fn sample(family: EffectFamily) -> Effect {
    let session = WorkerSessionId::from_parts(1, 2);
    match family {
        EffectFamily::Speech => {
            Effect::Speech(SpeechCmd::Cancel { speak: SpeakId::from_parts(1, 3) })
        }
        EffectFamily::Audio => Effect::Audio(AudioCmd::Unduck),
        EffectFamily::Brain => Effect::Brain(BrainCmd::Stop { slot: BrainSlot::Fallback }),
        EffectFamily::Worker => Effect::Worker(session, WorkerCmd::Interrupt),
        EffectFamily::Hook => Effect::Hook(
            session,
            HookReply::PreToolUse { request: 4, decision: HookDecision::None },
        ),
        EffectFamily::ToolForward => Effect::ToolForward {
            call: CallId::from_parts(1, 5),
            connector: ConnectorId::new("pc.apps").unwrap(),
            tool: ToolName::new("apps_open_url").unwrap(),
            args: Value::Null,
            deadline_ms: 5_000,
        },
        EffectFamily::ToolReply => Effect::ToolReply {
            caller: Caller::Internal,
            call: CallId::from_parts(1, 6),
            result: ToolResult {
                content: Vec::new(),
                is_error: false,
                verification: None,
                output: None,
                duration_ms: 3,
                rung: None,
            },
        },
        EffectFamily::Sidecar => Effect::Sidecar(SidecarId::Desktop, SidecarCmd::Restart),
        EffectFamily::Hud => Effect::Hud(Box::new(hud())),
        EffectFamily::Learn => Effect::Learn(LearnCmd::RunLibrarian { batch: Vec::new() }),
        EffectFamily::Settings => Effect::Settings(SettingsCmd::ApplyLive(Vec::new())),
        EffectFamily::Persist => Effect::Persist(Record::PlanUsage(usage())),
        EffectFamily::Emit => Effect::Emit(BusEvent::PlanUsage(usage())),
        EffectFamily::Timer => Effect::CancelTimer(TimerId(99)),
        EffectFamily::Incident => Effect::Incident(IncidentTrigger::UserReport),
        EffectFamily::Notify => Effect::Notify(Toast {
            title: "Render".to_owned(),
            body: "The render finished.".to_owned(),
            job: None,
        }),
    }
}
