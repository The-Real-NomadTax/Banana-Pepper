//! Wangan Midnight Maximum Tune 6R (Bandai Namco ES3 cabinet build).
//!
//! Everything specific to this title lives in this crate: the build
//! fingerprint, the executable transform, settings, cabinet controls and the
//! launch plan. The in-process cabinet devices (HASP dongle, USB finder,
//! card reader and JVS I/O board) are the `hook/` crate.
pub mod controls;
pub mod image;
mod plan;
pub mod settings;

use banana_api::{BackendKind, CliCommand, Controls, GameCompat, KnownBuild, Status, SupportLevel};

pub struct WanganMidnight6R;
pub static GAME: WanganMidnight6R = WanganMidnight6R;

impl GameCompat for WanganMidnight6R {
    fn id(&self) -> &'static str {
        "wmmt6r"
    }
    fn config_key(&self) -> &'static str {
        "wmmt6r"
    }
    fn title(&self) -> &'static str {
        "Wangan Midnight Maximum Tune 6R"
    }
    fn controls(&self) -> Controls {
        Controls::Driving
    }
    fn default_executable(&self) -> &'static str {
        "Wangan Midnight Maximum Tune 6R/wmn6r.exe"
    }
    fn source_roots(&self) -> &'static [&'static str] {
        &[
            "Wangan Midnight Maximum Tune 6R",
            "windows based/Wangan Midnight Maximum Tune 6R",
            "",
        ]
    }
    fn known_builds(&self) -> &'static [KnownBuild] {
        &[KnownBuild {
            name: "Cabinet build",
            sha256: image::SHA256,
        }]
    }
    fn status(&self) -> Status {
        Status {
            level: SupportLevel::Experimental,
            label: "Wine desktop bring-up",
        }
    }
    fn compatibility_notes(&self) -> Option<&'static [&'static str]> {
        Some(&[
            "Runs the SHA256-verified 64-bit wmn6r.exe under Wine on Linux. Windows hosts are not supported yet (the native view shim is 32-bit only).",
            "The hook DLL emulates the HASP dongle, the USB dongle finder and a JVS I/O board on COM3 (wheel, pedals, shifter, buttons). The card reader, touch panel and card vendor report as absent.",
            "Network play, card saves and the cabinet terminal are not emulated.",
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
            flag: "--launch-wmmt6r",
            action: "launch",
            banner: "Launching Wangan Midnight Maximum Tune 6R under Wine. Arrows or a controller drive, E/Q shift, Enter = view, 5 = coin, T = test, Esc or Back+Start = exit; bindings come from controls.cfg.",
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
