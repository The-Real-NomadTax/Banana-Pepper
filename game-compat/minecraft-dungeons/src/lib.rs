//! Minecraft Dungeons Arcade (Linux cabinet dump).
//!
//! Everything specific to this title lives in this crate: build
//! fingerprints, preparation, cabinet controls, settings and launchers.
pub mod prepare;
pub mod runtime;

use banana_api::{Action, BackendKind, CliCommand, Controls, GameCompat, KnownBuild, Status, SupportLevel};
use std::path::{Path, PathBuf};

pub struct MinecraftDungeons;
pub static GAME: MinecraftDungeons = MinecraftDungeons;

fn unsupported(action: &str) -> String {
    format!("Minecraft Dungeons Arcade has no {action} action")
}

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
        "Minecraft Dungeons Arcade/gamedump.exec"
    }
    fn locate_executable(&self, root: &Path) -> PathBuf {
        prepare::executable(root)
    }
    fn known_builds(&self) -> &'static [KnownBuild] {
        &[
        KnownBuild { name: "Supplied gamedump", sha256: prepare::DUMP_SHA256 }
        ]
    }
    fn finds_own_folder(&self) -> bool {
        true
    }
    fn status(&self) -> Status {
        Status { level: SupportLevel::Experimental, label: "Seeded desktop gameplay" }
    }
    fn compatibility_notes(&self) -> Option<&'static [&'static str]> {
        Some(&[
            "Windows uses WSL2 with WSLg; Linux uses the same isolated runner directly.",
            "Run startup diagnostics to check GLU, LAPACK, wireless/PCSC and FMOD 1.10 libraries.",
            "Play uses locally captured runtime/minecraft/seeds.json and prepared libraries; diagnostics retain the original HASP startup check.",
            "The seeded runner supplies virtual joysticks, keyboard input and card scanners. HUD issues remain; full gameplay is unverified.",
            "Supplied gamedump.exec is a pre-existing modified build; original game and cabinet boot scripts are never run.",
        ])
    }
    fn play_hint(&self) -> &'static str {
        "5 inserts a P1 coin; 1, Enter or Space starts. P1: arrows + Z/X/C. F1–F4 scan cards. Captured seeds and prepared libraries are required; HUD and gameplay issues remain."
    }
    fn backends(&self) -> &'static [BackendKind] {
        &[BackendKind::Native, BackendKind::Wsl]
    }
    fn extra_actions(&self) -> &'static [Action] {
        &[Action { id: "setup", label: "Prepare libraries" }]
    }
    fn cli_commands(&self) -> &'static [CliCommand] {
        &[
            CliCommand { flag: "--probe-minecraft", action: "probe", banner: "" },
            CliCommand { flag: "--launch-minecraft", action: "launch", banner: "" },
            CliCommand { flag: "--setup-minecraft-libs", action: "setup", banner: "" },
        ]
    }
    fn run(&self, root: &Path, action: &str) -> Result<String, String> {
        match action {
            "launch" | "probe" | "setup" => runtime::run(root, action),
            _ => Err(unsupported(action)),
        }
    }
}
