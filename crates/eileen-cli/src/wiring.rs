//! The components `eileen run` starts (section 3.3.15): each crate's `component(cfg)`, with the
//! recording's responder in place of the harness under `--replay-models`. WP0's stubs return
//! components that accept their effects and do nothing.

use eileen_audio::AudioConfig;
use eileen_contracts::ports::Component;
use eileen_harness::HarnessConfig;
use eileen_learn::LearnConfig;
use eileen_runtime::cli::RunArgs;

/// The components for a run with these arguments.
#[must_use]
pub fn components(args: &RunArgs) -> Vec<Box<dyn Component>> {
    let models = match &args.replay_models {
        Some(dir) => eileen_replay::responder(dir),
        None => eileen_harness::component(&HarnessConfig),
    };
    let mut components =
        vec![models, eileen_audio::component(&AudioConfig), eileen_learn::component(&LearnConfig)];
    components.extend(platform_components());
    components
}

/// The shell: the tray, hotkeys, Raw Input and toasts.
#[cfg(windows)]
fn platform_components() -> Vec<Box<dyn Component>> {
    vec![eileen_shell_win::component(&eileen_shell_win::ShellConfig)]
}

/// None yet: WP10 wires `eileen-shell-linux` here.
#[cfg(not(windows))]
const fn platform_components() -> Vec<Box<dyn Component>> {
    Vec::new()
}
