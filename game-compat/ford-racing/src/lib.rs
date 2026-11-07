//! Ford Racing Full Blown (Windows cabinet build).
//!
//! Everything specific to this title lives in this crate: build
//! fingerprints, preparation, cabinet controls, settings and launchers.
mod android;
pub mod controls;
pub mod packet;
mod plan;
pub mod prepare;
pub mod settings;

use banana_api::{BackendKind, CliCommand, Controls, GameCompat, KnownBuild, Status, SupportLevel};

pub struct FordRacing;
pub static GAME: FordRacing = FordRacing;

impl GameCompat for FordRacing {
    fn android_profile(&self) -> Option<banana_api::AndroidProfile> {
        Some(android::profile())
    }
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
        "Ford Racing/FordRacing/fordracing.exe"
    }
    fn content_executable(&self) -> &'static str {
        "FordRacing/fordracing.exe"
    }
    fn source_roots(&self) -> &'static [&'static str] {
        &["Ford Racing", "windows based/Ford Racing", ""]
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
                from: "sessions/ford/data/GameSettings.ini",
                to: "data/GameSettings.ini",
                kind: banana_api::ImportKind::Overlay,
            },
            banana_api::LegacyImport {
                from: "runtime/ford-racing/data/GameSettings.ini",
                to: "data/GameSettings.ini",
                kind: banana_api::ImportKind::Overlay,
            },
        ]
    }
    fn status(&self) -> Status {
        Status {
            level: SupportLevel::Experimental,
            label: "Desktop cabinet adapter",
        }
    }
    fn compatibility_notes(&self) -> Option<&'static [&'static str]> {
        Some(&[
            "Native Windows or Linux Wine with the same custom cabinet adapter; original files and bundled loaders are not run.",
            "Keyboard driving; physical wheels, force feedback and cabinet networking remain unimplemented.",
        ])
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
        &[CliCommand {
            flag: "--launch-ford",
            action: "launch",
            banner: "",
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
