//! Jurassic Park 2015 (Linux cabinet build).
//!
//! Everything specific to this title lives in this crate: build
//! fingerprints, preparation, cabinet controls, settings and launchers.
pub mod prepare;
pub mod runtime;
pub mod controls;
pub mod settings;

use banana_api::{BackendKind, CliCommand, Controls, GameCompat, KnownBuild, Status, SupportLevel};
use std::path::{Path, PathBuf};
use prepare::Build;

pub struct JurassicPark;
pub static GAME: JurassicPark = JurassicPark;

fn unsupported(action: &str) -> String {
    format!("Jurassic Park 2015 has no {action} action")
}

impl GameCompat for JurassicPark {
    fn id(&self) -> &'static str {
        "jurassic"
    }
    fn config_key(&self) -> &'static str {
        "jurassic"
    }
    fn title(&self) -> &'static str {
        "Jurassic Park 2015"
    }
    fn controls(&self) -> Controls {
        Controls::LightGuns
    }
    fn default_executable(&self) -> &'static str {
        "Jurassic Park 2015/game_original"
    }
    fn locate_executable(&self, root: &Path) -> PathBuf {
        prepare::directory(root).join("game_original")
    }
    fn known_builds(&self) -> &'static [KnownBuild] {
        &[
        KnownBuild { name: "Original", sha256: "6bb8a70dc0ee3f9d7ac3672d7dc8cf4aadec44b0f96c050ace96a7633d26af11" },
        KnownBuild { name: "Legacy patched", sha256: "c18a9329e72a363937155d245440694e58b5a1e27984016ae7648af3413b4269" }
        ]
    }
    fn status(&self) -> Status {
        Status { level: SupportLevel::Verified, label: "Desktop gameplay" }
    }
    fn compatibility_notes(&self) -> Option<&'static [&'static str]> {
        Some(&[
            "Runs from a Rust-prepared desktop profile that omits unavailable cabinet security initialization.",
            "Mouse aims and fires for player one; main audio plays through PulseAudio/PipeWire.",
            "Confirmed: the game fully loads into scored gameplay and high scores save correctly.",
        ])
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
            CliCommand { flag: "--launch-jurassic", action: "launch", banner: "Launching Jurassic Park on your display. Mouse or controller stick = aim, Esc or Back+Start = exit; keys and controller buttons come from controls.cfg. Audio plays through your desktop sound." },
            CliCommand { flag: "--probe-jurassic", action: "probe", banner: "" },
            CliCommand { flag: "--probe-jurassic-legacy", action: "probe-legacy", banner: "" },
            CliCommand { flag: "--probe-jurassic-native", action: "probe-native", banner: "" },
        ]
    }
    fn run(&self, root: &Path, action: &str) -> Result<String, String> {
        match action {
            "launch" => runtime::launch(root),
            "probe" => runtime::probe(root, Build::Original),
            "probe-legacy" => runtime::probe(root, Build::Legacy),
            "probe-native" => runtime::probe(root, Build::Native),
            _ => Err(unsupported(action)),
        }
    }
}
