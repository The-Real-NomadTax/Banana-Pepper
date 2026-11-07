//! Aliens Armageddon (Linux cabinet build).
//!
//! Everything specific to this title lives in this crate: build
//! fingerprints, preparation, cabinet controls, settings and launchers.
pub mod controls;
mod plan;
pub mod prepare;
pub mod settings;

use banana_api::{BackendKind, CliCommand, Controls, GameCompat, KnownBuild, Status, SupportLevel};

pub struct AliensArmageddon;
pub static GAME: AliensArmageddon = AliensArmageddon;

impl GameCompat for AliensArmageddon {
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
        "linux based/Aliens Armageddon/game"
    }
    fn known_builds(&self) -> &'static [KnownBuild] {
        &[KnownBuild {
            name: "Original cabinet build",
            sha256: prepare::ORIGINAL_SHA256,
        }]
    }
    fn legacy_state(&self) -> &'static [banana_api::LegacyImport] {
        &[banana_api::LegacyImport {
            from: "runtime/aliens/pmuser",
            to: "g6/aa/pmuser",
            kind: banana_api::ImportKind::Overlay,
        }]
    }
    fn status(&self) -> Status {
        Status {
            level: SupportLevel::Experimental,
            label: "Linux desktop gameplay",
        }
    }
    fn play_hint(&self) -> &'static str {
        "Mouse or controller sticks aim, RT fires. Start, coin and operator controls are under Settings. Esc or Back+Start exits."
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
