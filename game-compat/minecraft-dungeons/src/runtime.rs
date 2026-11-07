//! Minecraft's Rust runtime, shared by native Linux and the compiled WSL host.
#[cfg(not(windows))]
use std::fs;
use std::path::Path;
#[path = "image.rs"]
pub mod image;
#[cfg(any(target_os = "linux", test))]
#[path = "minecraft_native.rs"]
mod native;

pub fn run(root: &Path, mode: &str) -> Result<String, String> {
    if !matches!(mode, "launch" | "probe" | "setup") {
        return Err("Unknown Minecraft operation".into());
    }
    let project = banana_runtime::project_root()?;
    let root = banana_runtime::content::resolve(root)?;
    let game = crate::prepare::directory(&root)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    #[cfg(windows)]
    {
        let status = banana_wsl::run_game(
            &project,
            &banana_runtime::linux_host(&project),
            "minecraft",
            mode,
            &game,
        )?;
        return if status.success() {
            Ok("Minecraft WSL session completed".into())
        } else {
            Err(format!(
                "Minecraft WSL session failed ({status}); inspect runtime/minecraft"
            ))
        };
    }
    #[cfg(not(windows))]
    {
        if !cfg!(target_os = "linux") {
            return Err("Minecraft currently requires Linux or WSL".into());
        }
        #[cfg(target_os = "linux")]
        {
            native::run(&project, &game, mode)
        }
        #[cfg(not(target_os = "linux"))]
        {
            Err("Minecraft requires Linux or WSL".into())
        }
    }
}

pub fn scan_card(player: u8, code: &str) -> Result<String, String> {
    validate_card(player, code)?;
    let project = banana_runtime::project_root()?;
    #[cfg(windows)]
    {
        let status = banana_wsl::run_game(
            &project,
            &banana_runtime::linux_host(&project),
            "minecraft",
            &format!("scan:{player}:{code}"),
            &project,
        )?;
        if !status.success() {
            return Err("Minecraft card scan failed".into());
        }
    }
    #[cfg(not(windows))]
    {
        use std::io::Write;
        let output = project.join("runtime/minecraft-seeded");
        let lock = fs::OpenOptions::new()
            .read(true)
            .open(output.join("session.lock"))
            .map_err(|e| e.to_string())?;
        match lock.try_lock() {
            Ok(()) => return Err("Minecraft is not running".into()),
            Err(std::fs::TryLockError::WouldBlock) => {}
            Err(e) => return Err(e.to_string()),
        }
        let mut events = fs::OpenOptions::new()
            .append(true)
            .open(output.join("card-events"))
            .map_err(|e| e.to_string())?;
        writeln!(events, "{player} {code}").map_err(|e| e.to_string())?;
    }
    Ok(format!("Queued card {code} for player {player}"))
}

pub(super) fn validate_card(player: u8, code: &str) -> Result<(), String> {
    if !(1..=4).contains(&player)
        || code.len() != 8
        || !code.bytes().all(|b| b.is_ascii_alphanumeric())
    {
        return Err("Card requires player 1–4 and exactly eight ASCII letters/digits".into());
    }
    Ok(())
}
