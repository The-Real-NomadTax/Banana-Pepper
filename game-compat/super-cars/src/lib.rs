//! The Fast and the Furious Super Cars (Windows cabinet build).
//!
//! Everything specific to this title lives in this crate: build
//! fingerprints, preparation, cabinet controls, settings and launchers.
pub mod runtime;
pub mod controls;
pub mod settings;

use banana_api::{BackendKind, CliCommand, Controls, GameCompat, Status, SupportLevel};
use std::path::Path;

pub struct SuperCars;
pub static GAME: SuperCars = SuperCars;

fn unsupported(action: &str) -> String {
    format!("The Fast and the Furious Super Cars has no {action} action")
}

impl GameCompat for SuperCars {
    fn id(&self) -> &'static str {
        "super-cars"
    }
    fn config_key(&self) -> &'static str {
        "super_cars"
    }
    fn title(&self) -> &'static str {
        "The Fast and the Furious Super Cars"
    }
    fn controls(&self) -> Controls {
        Controls::Driving
    }
    fn default_executable(&self) -> &'static str {
        "windows based/The_Fast_and_the_Furious_SuperCars/The_Fast_and_the_Furious_SuperCars/sdaemon.exe"
    }
    fn status(&self) -> Status {
        Status { level: SupportLevel::Experimental, label: "Wine desktop gameplay" }
    }
    fn compatibility_notes(&self) -> Option<&'static [&'static str]> {
        Some(&[
            "Runs under Wine (WineD3D) for one SHA256-verified sdaemon.exe; keyboard drives the cabinet inputs through the Super Cars dinput8.dll hook.",
            "Steering is digital (ramped); analog wheels/gamepads, force feedback, LED display and the H2O board are not emulated.",
            "The game logs DONGLE MISSING periodically; races still run, but long sessions and audits are unverified.",
        ])
    }
    fn play_hint(&self) -> &'static str {
        "Arrow keys or a controller steer, accelerate and brake (stick and triggers are analog). Enter or Start begins, 5 inserts a coin, Esc or Back+Start exits. Change bindings in Settings."
    }
    fn backends(&self) -> &'static [BackendKind] {
        &[BackendKind::Wine]
    }
    fn settings_profile(&self) -> Option<&'static banana_api::SettingsProfile> {
        Some(&settings::SETTINGS)
    }
    fn control_layout(&self) -> Option<&'static banana_api::ControlLayout> {
        Some(&controls::CONTROLS)
    }
    fn cli_commands(&self) -> &'static [CliCommand] {
        &[
            CliCommand { flag: "--launch-super-cars", action: "launch", banner: "Launching Super Cars under Wine. Arrows or an XInput controller (stick steers, triggers = gas/brake) drive, Enter = start, 5 = coin, Esc or Back+Start = exit; bindings come from controls.cfg." },
        ]
    }
    fn run(&self, root: &Path, action: &str) -> Result<String, String> {
        match action {
            "launch" => runtime::launch(root),
            _ => Err(unsupported(action)),
        }
    }
}
