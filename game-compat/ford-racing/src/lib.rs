//! Ford Racing Full Blown (Windows cabinet build).
//!
//! Everything specific to this title lives in this crate: build
//! fingerprints, preparation, cabinet controls, settings and launchers.
pub mod prepare;
pub mod runtime;
pub mod controls;
pub mod settings;
pub mod packet;

use banana_api::{BackendKind, CliCommand, Controls, GameCompat, KnownBuild, Status, SupportLevel};
use std::path::{Path, PathBuf};

pub struct FordRacing;
pub static GAME: FordRacing = FordRacing;

fn unsupported(action: &str) -> String {
    format!("Ford Racing Full Blown has no {action} action")
}

impl GameCompat for FordRacing {
    fn id(&self) -> &'static str {
        "ford"
    }
    fn config_key(&self) -> &'static str {
        "ford"
    }
    fn title(&self) -> &'static str {
        "Ford Racing Full Blown"
    }
    fn controls(&self) -> Controls {
        Controls::Driving
    }
    fn default_executable(&self) -> &'static str {
        "windows based/Ford Racing/FordRacing/fordracing.exe"
    }
    fn locate_executable(&self, root: &Path) -> PathBuf {
        prepare::directory(root).join("FordRacing/fordracing.exe")
    }
    fn known_builds(&self) -> &'static [KnownBuild] {
        &[
        KnownBuild { name: "Cabinet build", sha256: prepare::SOURCE_SHA256 }
        ]
    }
    fn status(&self) -> Status {
        Status { level: SupportLevel::Experimental, label: "Desktop cabinet adapter" }
    }
    fn compatibility_notes(&self) -> Option<&'static [&'static str]> {
        Some(&[
            "Native Windows or Linux Wine with the same custom cabinet adapter; original files and bundled loaders are not run.",
            "Keyboard driving; physical wheels, force feedback and cabinet networking remain unimplemented.",
        ])
    }
    fn play_hint(&self) -> &'static str {
        "Arrow keys or an XInput controller drive (stick steers, triggers are analog gas/brake). Enter or Start begins, 5 inserts a coin, Esc or Back+Start exits. Change bindings in Settings."
    }
    fn backends(&self) -> &'static [BackendKind] {
        &[BackendKind::Native, BackendKind::Wine]
    }
    fn settings_profile(&self) -> Option<&'static banana_api::SettingsProfile> {
        Some(&settings::SETTINGS)
    }
    fn control_layout(&self) -> Option<&'static banana_api::ControlLayout> {
        Some(&controls::CONTROLS)
    }
    fn cli_commands(&self) -> &'static [CliCommand] {
        &[
            CliCommand { flag: "--launch-ford", action: "launch", banner: "" },
        ]
    }
    fn run(&self, root: &Path, action: &str) -> Result<String, String> {
        match action {
            "launch" => runtime::launch(root),
            _ => Err(unsupported(action)),
        }
    }
}
