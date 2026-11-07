//! Aliens Armageddon (Linux cabinet build).
//!
//! Everything specific to this title lives in this crate: build
//! fingerprints, preparation, cabinet controls, settings and launchers.
mod android;
pub mod controls;
mod plan;
pub mod prepare;
pub mod settings;

use banana_api::{BackendKind, CliCommand, Controls, GameCompat, KnownBuild, Status, SupportLevel};

pub struct AliensArmageddon;
pub static GAME: AliensArmageddon = AliensArmageddon;

impl GameCompat for AliensArmageddon {
    fn android_profile(&self) -> Option<banana_api::AndroidProfile> {
        Some(android::profile())
    }
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
    fn source_roots(&self) -> &'static [&'static str] {
        &["Aliens Armageddon", "linux based/Aliens Armageddon", ""]
    }
    fn known_builds(&self) -> &'static [KnownBuild] {
        &[KnownBuild {
            name: "Original cabinet build",
            sha256: prepare::ORIGINAL_SHA256,
        }]
    }
    fn legacy_state(&self) -> &'static [banana_api::LegacyImport] {
        &[
            banana_api::LegacyImport {
                from: "sessions/aliens/pmuser",
                to: "g6/aa/pmuser",
                kind: banana_api::ImportKind::Overlay,
            },
            banana_api::LegacyImport {
                from: "runtime/aliens/pmuser",
                to: "g6/aa/pmuser",
                kind: banana_api::ImportKind::Overlay,
            },
        ]
    }
    fn status(&self) -> Status {
        Status {
            level: SupportLevel::Experimental,
            label: "Linux desktop gameplay",
        }
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
                flag: "--probe-aliens",
                action: "probe",
                banner: "",
            },
            CliCommand {
                flag: "--launch-aliens",
                action: "launch",
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
