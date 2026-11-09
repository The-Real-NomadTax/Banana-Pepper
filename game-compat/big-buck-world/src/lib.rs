//! Big Buck World (Linux cabinet build).
//!
//! Everything specific to this title lives in this crate: build
//! fingerprints, preparation, cabinet controls, settings and launchers.
mod android;
pub mod controls;
mod plan;
pub mod prepare;
pub mod settings;

use banana_api::{BackendKind, CliCommand, Controls, GameCompat, KnownBuild, Status, SupportLevel};

pub struct BigBuckWorld;
pub static GAME: BigBuckWorld = BigBuckWorld;

impl GameCompat for BigBuckWorld {
    fn android_profile(&self) -> Option<banana_api::AndroidProfile> {
        Some(android::profile())
    }
    fn id(&self) -> &'static str {
        "big-buck-world"
    }
    fn config_key(&self) -> &'static str {
        "big_buck_world"
    }
    fn title(&self) -> &'static str {
        "Big Buck World"
    }
    fn controls(&self) -> Controls {
        Controls::LightGuns
    }
    fn default_executable(&self) -> &'static str {
        "Big Buck World (1.20)/game"
    }
    fn source_roots(&self) -> &'static [&'static str] {
        &[
            "Big Buck World (1.20)",
            "Big Buck World",
            "linux based/Big Buck World (1.20)",
            "linux based/Big Buck World",
            "",
        ]
    }
    fn known_builds(&self) -> &'static [KnownBuild] {
        &[KnownBuild {
            name: "World 1.20",
            sha256: prepare::SOURCE_SHA256,
        }]
    }
    fn status(&self) -> Status {
        Status {
            level: SupportLevel::Experimental,
            label: "Mounted session validation pending",
        }
    }
    fn compatibility_notes(&self) -> Option<&'static [&'static str]> {
        Some(&[
            "Linux desktop and Water ELF32 backends support the fingerprinted build; Windows uses Water.",
            "Virtual JAMMA API supports switch edges and mouse gun callbacks; physical serial protocol is not implemented.",
            "OSS playback is bridged to PulseAudio/PipeWire.",
            "World 1.20 uses the shared virtual JAMMA and GLUT input adapter; gameplay validation is in progress.",
        ])
    }
    fn backends(&self) -> &'static [BackendKind] {
        &[BackendKind::Native, BackendKind::Water]
    }
    fn settings_profile(&self) -> Option<&'static banana_api::SettingsProfile> {
        Some(&settings::SETTINGS)
    }
    fn control_layout(&self) -> Option<&'static banana_api::ControlLayout> {
        Some(&controls::CONTROLS)
    }
    fn extra_actions(&self) -> &'static [banana_api::Action] {
        &[banana_api::Action {
            id: "setup",
            label: "Prepare libraries",
        }]
    }
    fn cli_commands(&self) -> &'static [CliCommand] {
        &[
            CliCommand {
                flag: "--launch-big-buck-world",
                action: "launch",
                banner: "Launching Big Buck World on your display. Mouse = aim/fire, right click = pump; controller stick aims, RT fires, LT pumps; Esc or Back+Start = exit; bindings come from controls.cfg.",
            },
            CliCommand {
                flag: "--probe-big-buck-world",
                action: "probe",
                banner: "",
            },
        ]
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
