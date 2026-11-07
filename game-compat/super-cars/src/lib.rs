//! The Fast and the Furious Super Cars (Windows cabinet build).
//!
//! Everything specific to this title lives in this crate: build
//! fingerprints, preparation, cabinet controls, settings and launchers.
mod android;
pub mod controls;
mod plan;
pub mod settings;

use banana_api::{BackendKind, CliCommand, Controls, GameCompat, Status, SupportLevel};

pub struct SuperCars;
pub static GAME: SuperCars = SuperCars;

impl GameCompat for SuperCars {
    fn android_profile(&self) -> Option<banana_api::AndroidProfile> {
        Some(android::profile())
    }
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
        "The_Fast_and_the_Furious_SuperCars/The_Fast_and_the_Furious_SuperCars/sdaemon.exe"
    }
    fn source_roots(&self) -> &'static [&'static str] {
        &[
            "The_Fast_and_the_Furious_SuperCars/The_Fast_and_the_Furious_SuperCars",
            "windows based/The_Fast_and_the_Furious_SuperCars/The_Fast_and_the_Furious_SuperCars",
            "The_Fast_and_the_Furious_SuperCars",
            "",
        ]
    }
    fn known_builds(&self) -> &'static [banana_api::KnownBuild] {
        &[banana_api::KnownBuild {
            name: "Cabinet build",
            sha256: "0cddbdbf1d107d27dc668897d7851b6ac96b4349320551d800504b1c50b195ba",
        }]
    }
    fn legacy_state(&self) -> &'static [banana_api::LegacyImport] {
        &[banana_api::LegacyImport {
            from: "sessions/super-cars/game/prefs",
            to: "prefs",
            kind: banana_api::ImportKind::Overlay,
        }]
    }
    fn status(&self) -> Status {
        Status {
            level: SupportLevel::Experimental,
            label: "Wine desktop gameplay",
        }
    }
    fn compatibility_notes(&self) -> Option<&'static [&'static str]> {
        Some(&[
            "Runs under Wine (WineD3D) for one SHA256-verified sdaemon.exe; keyboard drives the cabinet inputs through the Super Cars dinput8.dll hook.",
            "Steering is digital (ramped); analog wheels/gamepads, force feedback, LED display and the H2O board are not emulated.",
            "The game logs DONGLE MISSING periodically; races still run, but long sessions and audits are unverified.",
        ])
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
        &[CliCommand {
            flag: "--launch-super-cars",
            action: "launch",
            banner: "Launching Super Cars under Wine. Arrows or an XInput controller (stick steers, triggers = gas/brake) drive, Enter = start, 5 = coin, Esc or Back+Start = exit; bindings come from controls.cfg.",
        }]
    }
    fn plan(
        &self,
        build: &banana_api::IdentifiedBuild,
        context: &banana_api::PlanContext,
        action: &banana_api::ActionRequest,
    ) -> Result<banana_api::LaunchPlan, String> {
        plan::make(build, context, action)
    }
}
