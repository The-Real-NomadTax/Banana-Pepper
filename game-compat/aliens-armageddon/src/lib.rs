//! Aliens Armageddon (Linux cabinet build).
//!
//! Everything specific to this title lives in this crate: build
//! fingerprints, preparation, cabinet controls, settings and launchers.
pub mod android;
pub mod prepare;
pub mod runtime;
pub mod controls;
pub mod settings;

use banana_api::{BackendKind, CliCommand, Controls, GameCompat, KnownBuild, Status, SupportLevel};
use std::path::{Path, PathBuf};

pub struct AliensArmageddon;
pub static GAME: AliensArmageddon = AliensArmageddon;

fn unsupported(action: &str) -> String {
    format!("Aliens Armageddon has no {action} action")
}

impl GameCompat for AliensArmageddon {
    fn id(&self) -> &'static str {
        "aliens"
    }
    fn config_key(&self) -> &'static str {
        "aliens"
    }
    fn title(&self) -> &'static str {
        "Aliens Armageddon"
    }
    fn controls(&self) -> Controls {
        Controls::LightGuns
    }
    fn default_executable(&self) -> &'static str {
        "Aliens Armageddon/game"
    }
    fn locate_executable(&self, root: &Path) -> PathBuf {
        prepare::directory(root).join("game")
    }
    fn known_builds(&self) -> &'static [KnownBuild] {
        &[
        KnownBuild { name: "Original cabinet build", sha256: prepare::ORIGINAL_SHA256 }
        ]
    }
    fn status(&self) -> Status {
        Status { level: SupportLevel::Experimental, label: "Linux desktop gameplay" }
    }
    fn play_hint(&self) -> &'static str {
        "Mouse or controller sticks aim, RT fires. Start, coin and operator controls are under Settings. Esc or Back+Start exits."
    }
    fn backends(&self) -> &'static [BackendKind] {
        &[BackendKind::Native]
    }
    fn settings_profile(&self) -> Option<&'static banana_api::SettingsProfile> {
        Some(&settings::SETTINGS)
    }
    fn control_layout(&self) -> Option<&'static banana_api::ControlLayout> {
        Some(&controls::CONTROLS)
    }
    fn cli_commands(&self) -> &'static [CliCommand] {
        &[
            CliCommand { flag: "--probe-aliens", action: "probe", banner: "" },
            CliCommand { flag: "--launch-aliens", action: "launch", banner: "" },
        ]
    }
    fn android_profile(&self) -> Option<banana_api::AndroidProfile> { Some(android::profile()) }
    fn prepare_android(&self, source: &Path, work: &Path, tools: &Path) -> Result<banana_api::AndroidLaunch, String> {
        android::prepare(source, work, tools)
    }
    fn run(&self, root: &Path, action: &str) -> Result<String, String> {
        match action {
            "launch" => runtime::run(root, true),
            "probe" => runtime::run(root, false),
            _ => Err(unsupported(action)),
        }
    }
}
