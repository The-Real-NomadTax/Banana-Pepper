//! Minecraft Dungeons Arcade (Linux cabinet dump).
//!
//! Everything specific to this title lives in this crate: build
//! fingerprints, preparation, cabinet controls, settings and launchers.
mod dependencies;
mod image;
mod plan;
pub mod prepare;

use banana_api::{
    Action, BackendKind, CliCommand, Controls, GameCompat, KnownBuild, Status, SupportLevel,
};

pub struct MinecraftDungeons;
pub static GAME: MinecraftDungeons = MinecraftDungeons;

impl GameCompat for MinecraftDungeons {
    fn id(&self) -> &'static str {
        "minecraft"
    }
    fn config_key(&self) -> &'static str {
        "minecraft"
    }
    fn title(&self) -> &'static str {
        "Minecraft Dungeons Arcade"
    }
    fn controls(&self) -> Controls {
        Controls::Joysticks
    }
    fn default_executable(&self) -> &'static str {
        "linux based/Minecraft Dungeons Arcade/gamedump.exec"
    }
    fn executable_names(&self) -> &'static [&'static str] {
        &["gamedump.exec", "game"]
    }
    fn known_builds(&self) -> &'static [KnownBuild] {
        &[KnownBuild {
            name: "Supplied gamedump",
            sha256: prepare::DUMP_SHA256,
        }]
    }
    fn session_actions(&self) -> &'static [banana_api::SessionAction] {
        &[banana_api::SessionAction {
            id: "scan-card",
            label: "Scan card",
            schema: r#"{"type":"object","required":["player","code"],"additionalProperties":false,"properties":{"player":{"type":"integer","minimum":1,"maximum":4},"code":{"type":"string","pattern":"^[A-Za-z0-9]{8}$"}}}"#,
        }]
    }
    fn validate_session_action(&self, a: &banana_api::ActionRequest) -> Result<(), String> {
        if a.id == "cancel" {
            return Ok(());
        }
        if a.id != "scan-card" {
            return Err("Unsupported Minecraft session action".into());
        }
        let p = a
            .payload
            .as_object()
            .ok_or("Card scan requires player and code")?;
        if p.len() != 2
            || !p
                .get("player")
                .and_then(|p| p.as_u64())
                .is_some_and(|n| (1..=4).contains(&n))
            || !p
                .get("code")
                .and_then(|p| p.as_str())
                .is_some_and(|s| s.len() == 8 && s.bytes().all(|b| b.is_ascii_alphanumeric()))
        {
            return Err(
                "Card scan requires player 1–4 and an eight-character alphanumeric code".into(),
            );
        }
        Ok(())
    }
    fn legacy_state(&self) -> &'static [banana_api::LegacyImport] {
        &[
            banana_api::LegacyImport {
                from: "runtime/minecraft/seeds.json",
                to: "seeds.json",
                kind: banana_api::ImportKind::State,
            },
            banana_api::LegacyImport {
                from: "runtime/minecraft-seeded/home",
                to: "home",
                kind: banana_api::ImportKind::State,
            },
        ]
    }
    fn status(&self) -> Status {
        Status {
            level: SupportLevel::Experimental,
            label: "Seeded desktop gameplay",
        }
    }
    fn compatibility_notes(&self) -> Option<&'static [&'static str]> {
        Some(&[
            "Windows uses Water when the required ELF64 capabilities are available; Linux runs natively.",
            "Run startup diagnostics to check GLU, LAPACK, wireless/PCSC and FMOD 1.10 libraries.",
            "Play uses locally captured per-user captured seed records and prepared libraries; diagnostics retain the original HASP startup check.",
            "The seeded runner supplies virtual joysticks, keyboard input and card scanners. HUD issues remain; full gameplay is unverified.",
            "Supplied gamedump.exec is a pre-existing modified build; original game and cabinet boot scripts are never run.",
        ])
    }
    fn play_hint(&self) -> &'static str {
        "5 inserts a P1 coin; 1, Enter or Space starts. P1: arrows + Z/X/C. F1â€“F4 scan cards. Captured seeds and prepared libraries are required; HUD and gameplay issues remain."
    }
    fn backends(&self) -> &'static [BackendKind] {
        &[BackendKind::Native, BackendKind::Water]
    }
    fn extra_actions(&self) -> &'static [Action] {
        &[Action {
            id: "setup",
            label: "Prepare libraries",
        }]
    }
    fn cli_commands(&self) -> &'static [CliCommand] {
        &[
            CliCommand {
                flag: "--probe-minecraft",
                action: "probe",
                banner: "",
            },
            CliCommand {
                flag: "--launch-minecraft",
                action: "launch",
                banner: "",
            },
            CliCommand {
                flag: "--setup-minecraft-libs",
                action: "setup",
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
