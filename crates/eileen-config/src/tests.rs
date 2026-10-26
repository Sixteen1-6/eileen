use std::path::Path;

use eileen_contracts::config::{Config, SettingChange};
use eileen_contracts::ports::SettingsValidator;

use crate::{ConfigError, Layers, Validator, apply, explain, load, registry, validate};

fn rename() -> SettingChange {
    SettingChange { key: "assistant.name".to_owned(), value: serde_json::json!("Nova") }
}

#[test]
fn the_stub_loads_validates_and_applies_nothing() {
    assert!(registry().is_empty(), "no settings registered yet");
    assert!(
        matches!(load(Path::new("config")), Err(ConfigError::Unavailable { .. })),
        "nothing is read"
    );
    let changes = [rename()];
    assert!(validate(&Config::default(), &changes).is_err(), "nothing is validated");
    assert!(apply("[assistant]\n", &changes).is_err(), "nothing is written");
    assert!(explain(&Config::default(), &Layers).is_empty(), "nothing is explained");
}

#[test]
fn the_stub_validator_refuses_every_change() {
    let refusal = Validator.validate(&Config::default(), &[rename()]).unwrap_err();
    assert!(refusal.message.contains("not built yet"), "{}", refusal.message);
    assert!(Validator.specs().is_empty(), "no specs yet");
}
