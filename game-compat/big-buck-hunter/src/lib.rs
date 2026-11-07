//! Big Buck Hunter Pro (Linux cabinet build).
//!
//! Everything specific to this title lives in this crate: build
//! fingerprints, preparation, cabinet controls, settings and launchers.
pub mod controls;
mod plan;
pub mod prepare;
pub mod settings;

use banana_api::{BackendKind, CliCommand, Controls, GameCompat, KnownBuild, Status, SupportLevel};

pub struct BigBuckHunter;
pub static GAME: BigBuckHunter = BigBuckHunter;

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
        "linux based/Big Bug Hunter Pro/game"
    }
    fn source_roots(&self) -> &'static [&'static str] {
        &[
            "linux based/Big Buck Hunter Pro",
            "Big Buck Hunter Pro",
            "Big Bug Hunter Pro",
        ]
    }
    fn known_builds(&self) -> &'static [KnownBuild] {
        &[KnownBuild {
            name: "Cabinet build",
            sha256: prepare::SOURCE_SHA256,
        }]
    }
    fn legacy_state(&self) -> &'static [banana_api::LegacyImport] {
        &[
            banana_api::LegacyImport {
                from: "runtime/big-buck-native/bbh3user",
                to: "bbh3user",
                kind: banana_api::ImportKind::Overlay,
            },
            banana_api::LegacyImport {
                from: "runtime/big-buck/bbh3user",
                to: "bbh3user",
                kind: banana_api::ImportKind::Overlay,
            },
        ]
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
            "The historical runner reached scored gameplay; this mounted pipeline needs gameplay and save validation.",
        ])
    }
    fn play_hint(&self) -> &'static str {
        "Mouse or controller aims and fires; right click or LT pumps. Start and coin bindings are under Settings. Esc or Back+Start exits."
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
    fn cli_commands(&self) -> &'static [CliCommand] {
        &[
            CliCommand {
                flag: "--launch-big-buck",
                action: "launch",
                banner: "Launching Big Buck Hunter on your display. Mouse = aim/fire, right click = pump; controller stick aims, RT fires, LT pumps; Esc or Back+Start = exit; bindings come from controls.cfg.",
            },
            CliCommand {
                flag: "--probe-big-buck",
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
