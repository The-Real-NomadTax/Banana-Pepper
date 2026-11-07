//! Big Buck Hunter Pro (Linux cabinet build).
//!
//! Everything specific to this title lives in this crate: build
//! fingerprints, preparation, cabinet controls, settings and launchers.
pub mod android;
pub mod prepare;
pub mod runtime;
pub mod controls;
pub mod settings;
#[cfg(target_os = "linux")]
pub mod linux;

use banana_api::{BackendKind, CliCommand, Controls, GameCompat, KnownBuild, Status, SupportLevel};
use std::path::{Path, PathBuf};

pub struct BigBuckHunter;
pub static GAME: BigBuckHunter = BigBuckHunter;

fn unsupported(action: &str) -> String {
    format!("Big Buck Hunter Pro has no {action} action")
}

impl GameCompat for BigBuckHunter {
    fn id(&self) -> &'static str {
        "big-buck"
    }
    fn config_key(&self) -> &'static str {
        "big_buck"
    }
    fn title(&self) -> &'static str {
        "Big Buck Hunter Pro"
    }
    fn controls(&self) -> Controls {
        Controls::LightGuns
    }
    fn default_executable(&self) -> &'static str {
        "Big Bug Hunter Pro/game"
    }
    fn locate_executable(&self, root: &Path) -> PathBuf {
        prepare::directory(root).join("game")
    }
    fn known_builds(&self) -> &'static [KnownBuild] {
        &[
        KnownBuild { name: "Cabinet build", sha256: prepare::SOURCE_SHA256 }
        ]
    }
    fn status(&self) -> Status {
        Status { level: SupportLevel::Verified, label: "Desktop gameplay" }
    }
    fn compatibility_notes(&self) -> Option<&'static [&'static str]> {
        Some(&[
            "Linux desktop and Water ELF32 backends support the fingerprinted build; Windows also offers WSL2.",
            "Virtual JAMMA API supports switch edges and mouse gun callbacks; physical serial protocol is not implemented.",
            "OSS playback is bridged to PulseAudio/PipeWire.",
            "Confirmed: the game fully loads into scored gameplay and high scores save correctly.",
        ])
    }
    fn backends(&self) -> &'static [BackendKind] {
        &[BackendKind::Native, BackendKind::Water, BackendKind::Wsl]
    }
    fn settings_profile(&self) -> Option<&'static banana_api::SettingsProfile> {
        Some(&settings::SETTINGS)
    }
    fn control_layout(&self) -> Option<&'static banana_api::ControlLayout> {
        Some(&controls::CONTROLS)
    }
    fn cli_commands(&self) -> &'static [CliCommand] {
        &[
            CliCommand { flag: "--launch-big-buck", action: "launch", banner: "Launching Big Buck Hunter on your display. Mouse = aim/fire, right click = pump; controller stick aims, RT fires, LT pumps; Esc or Back+Start = exit; bindings come from controls.cfg." },
            CliCommand { flag: "--probe-big-buck", action: "probe", banner: "" },
        ]
    }
    fn android_profile(&self) -> Option<banana_api::AndroidProfile> { Some(android::profile()) }
    fn prepare_android(&self, source: &Path, work: &Path, tools: &Path) -> Result<banana_api::AndroidLaunch, String> {
        android::prepare(source, work, tools)
    }
    fn run(&self, root: &Path, action: &str) -> Result<String, String> {
        match action {
            "launch" => runtime::run(root, false),
            "probe" => runtime::run(root, true),
            _ => Err(unsupported(action)),
        }
    }
}
