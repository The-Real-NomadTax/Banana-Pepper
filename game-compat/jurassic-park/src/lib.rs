//! Jurassic Park 2015 (Linux cabinet build).
//!
//! Everything specific to this title lives in this crate: build
//! fingerprints, preparation, cabinet controls, settings and launchers.
mod android;
pub mod controls;
mod plan;
pub mod prepare;
pub mod settings;

use banana_api::{BackendKind, CliCommand, Controls, GameCompat, KnownBuild, Status, SupportLevel};

pub struct JurassicPark;
pub static GAME: JurassicPark = JurassicPark;

impl GameCompat for JurassicPark {
    fn android_profile(&self) -> Option<banana_api::AndroidProfile> {
        Some(android::profile())
    }
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
        "Jurassic Park 2015/game_legacy_patched"
    }
    fn executable_names(&self) -> &'static [&'static str] {
        &["game_legacy_patched", "game_original"]
    }
    fn source_roots(&self) -> &'static [&'static str] {
        &["Jurassic Park 2015", "linux based/Jurassic Park 2015", ""]
    }
    fn known_builds(&self) -> &'static [KnownBuild] {
        &[
            KnownBuild {
                name: "Original",
                sha256: "6bb8a70dc0ee3f9d7ac3672d7dc8cf4aadec44b0f96c050ace96a7633d26af11",
            },
            KnownBuild {
                name: "Legacy patched",
                sha256: "c18a9329e72a363937155d245440694e58b5a1e27984016ae7648af3413b4269",
            },
        ]
    }
    fn legacy_state(&self) -> &'static [banana_api::LegacyImport] {
        &[
            banana_api::LegacyImport {
                from: "sessions/jurassic/pmuser",
                to: "pmuser",
                kind: banana_api::ImportKind::Overlay,
            },
            banana_api::LegacyImport {
                from: "runtime/jurassic/desktop/pmuser",
                to: "pmuser",
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
            "Runs from a Rust-prepared desktop profile that omits unavailable cabinet security initialization.",
            "Mouse aims and fires for player one; main audio plays through PulseAudio/PipeWire.",
            "The historical runner reached scored gameplay; this mounted pipeline needs gameplay and save validation.",
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
    fn cli_commands(&self) -> &'static [CliCommand] {
        &[
            CliCommand {
                flag: "--launch-jurassic",
                action: "launch",
                banner: "Launching Jurassic Park on your display. Mouse or controller stick = aim, Esc or Back+Start = exit; keys and controller buttons come from controls.cfg. Audio plays through your desktop sound.",
            },
            CliCommand {
                flag: "--probe-jurassic",
                action: "probe",
                banner: "",
            },
            CliCommand {
                flag: "--probe-jurassic-legacy",
                action: "probe-legacy",
                banner: "",
            },
            CliCommand {
                flag: "--probe-jurassic-native",
                action: "probe-native",
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
