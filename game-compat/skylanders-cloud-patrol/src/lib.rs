//! Skylanders Cloud Patrol (2014 Windows touch-screen cabinet build).
//!
//! Everything specific to this title lives in this crate: build
//! fingerprints, virtual layout, cabinet controls, settings and the launch plan. The
//! touch adapter DLL that replaces the PQ Labs client is in `hook/`.
pub mod controls;
mod plan;
pub mod prepare;
pub mod settings;

use banana_api::{BackendKind, CliCommand, Controls, GameCompat, KnownBuild, Status, SupportLevel};

pub struct SkylandersCloudPatrol;
pub static GAME: SkylandersCloudPatrol = SkylandersCloudPatrol;

impl GameCompat for SkylandersCloudPatrol {
    fn id(&self) -> &'static str {
        "skylanders"
    }
    fn config_key(&self) -> &'static str {
        "skylanders"
    }
    fn title(&self) -> &'static str {
        "Skylanders Cloud Patrol"
    }
    fn controls(&self) -> Controls {
        Controls::LightGuns
    }
    fn default_executable(&self) -> &'static str {
        "skylanders_cloud_patrol/Skylanders.exe"
    }
    fn known_builds(&self) -> &'static [KnownBuild] {
        &[KnownBuild {
            name: "Cabinet build (Unity 4.2.2)",
            sha256: prepare::SOURCE_SHA256,
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
            "Runs the Unity cabinet build under Wine from a writable virtual filesystem; the cabinet's updater, SSH tunnel and reboot script are never run.",
            "The PQ Labs touch screen is replaced by the Banana Pepper adapter: the mouse, keys and controllers touch the game window.",
            "The Kaboom coin/ticket board is emulated: the coin action inserts coins and tickets pay out instantly. The Matrix dongle is not emulated.",
        ])
    }
    fn backends(&self) -> &'static [BackendKind] {
        &[BackendKind::Wine, BackendKind::Native]
    }
    fn settings_profile(&self) -> Option<&'static banana_api::SettingsProfile> {
        Some(&settings::SETTINGS)
    }
    fn control_layout(&self) -> Option<&'static banana_api::ControlLayout> {
        Some(&controls::CONTROLS)
    }
    fn cli_commands(&self) -> &'static [CliCommand] {
        &[CliCommand {
            flag: "--launch-skylanders",
            action: "launch",
            banner: "Launching Skylanders Cloud Patrol under Wine. 5 / controller Back inserts a coin, click or Space / controller A touches, Esc or Back+Start exits; bindings come from controls.cfg.",
        }]
    }
    fn source_roots(&self) -> &'static [&'static str] {
        prepare::ROOTS
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
