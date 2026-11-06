//! Native Windows frontend -> WSL2 guest, or a Linux guest launched directly.
use std::{
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

#[cfg(windows)]
fn guest_command() -> Command {
    use std::os::windows::process::CommandExt;
    let mut command = Command::new("wsl.exe");
    command.creation_flags(0x08000000); // CREATE_NO_WINDOW: game windows come from WSLg.
    if let Some(distro) = std::env::var_os("ARCADE_WSL_DISTRO") {
        command.arg("--distribution").arg(distro);
    }
    command.arg("--exec");
    command
}

#[cfg(windows)]
fn guest_path(path: &Path) -> Result<String, String> {
    let path = path
        .to_str()
        .ok_or("The selected path must be valid Unicode")?;
    let path = path.strip_prefix(r"\\?\").unwrap_or(path);
    let output = guest_command()
        .args(["wslpath", "-a", "-u", path])
        .output()
        .map_err(|e| format!("Cannot start WSL2: {e}. Install Ubuntu with WSLg first."))?;
    if !output.status.success() {
        return Err(format!(
            "WSL path conversion failed: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().into())
}

#[cfg(not(windows))]
fn guest_path(path: &Path) -> Result<String, String> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| "Path must be valid Unicode".into())
}

pub fn run(root: &Path, mode: &str) -> Result<String, String> {
    let materialized = banana_runtime::content::resolve(root)?;
    let root = materialized.as_path();
    if !matches!(mode, "probe" | "launch" | "setup") {
        return Err("Unknown Minecraft operation".into());
    }
    let game = crate::prepare::directory(root)
        .canonicalize()
        .map_err(|e| format!("Minecraft game folder was not found: {e}"))?;
    let executable = game.join("gamedump.exec");
    if mode != "setup" {
        crate::prepare::verify_dump(&executable)?;
    }
    let project = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = project.join("runtime/minecraft");
    std::fs::create_dir_all(&output).map_err(|e| e.to_string())?;
    let output = output.canonicalize().map_err(|e| e.to_string())?;
    std::fs::write(
        output.join("minecraft_prepare.py"),
        include_bytes!("../../../scripts/minecraft-prepare.py"),
    )
    .map_err(|e| e.to_string())?;
    let game_arg = guest_path(&game)?;
    let output_arg = guest_path(&output)?;
    #[cfg(windows)]
    let mut command = {
        let mut c = guest_command();
        c.arg("python3");
        c
    };
    #[cfg(not(windows))]
    let mut command = Command::new("python3");
    if mode == "launch" {
        // Keep the seeded runner's helpers together, while sharing the existing
        // libraries, captured seeds and save directory with command-line runs.
        let scripts = output.join("scripts");
        std::fs::create_dir_all(&scripts).map_err(|e| e.to_string())?;
        for (name, bytes) in LAUNCH_SCRIPTS {
            std::fs::write(scripts.join(name), bytes).map_err(|e| e.to_string())?;
        }
        command.args(launch_args(
            &guest_path(&scripts)?,
            &game_arg,
            &guest_path(&project)?,
        ));
    } else {
        command.args(["-", &game_arg, &output_arg, mode]);
    }
    let mut child = command
        .stdin(if mode == "launch" {
            Stdio::null()
        } else {
            Stdio::piped()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Cannot start Minecraft runner: {e}"))?;
    if mode != "launch" {
        child
            .stdin
            .take()
            .ok_or("Guest stdin unavailable")?
            .write_all(include_bytes!("../../../scripts/minecraft-run.py"))
            .map_err(|e| e.to_string())?;
    }
    let result = child.wait_with_output().map_err(|e| e.to_string())?;
    let message = format!(
        "{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    if result.status.success() {
        Ok(message)
    } else {
        Err(message)
    }
}

const LAUNCH_SCRIPTS: &[(&str, &[u8])] = &[
    (
        "minecraft-seeded.py",
        include_bytes!("../../../scripts/minecraft-seeded.py"),
    ),
    (
        "minecraft-run.py",
        include_bytes!("../../../scripts/minecraft-run.py"),
    ),
    (
        "minecraft-prepare.py",
        include_bytes!("../../../scripts/minecraft-prepare.py"),
    ),
    (
        "minecraft-joysticks.py",
        include_bytes!("../../../scripts/minecraft-joysticks.py"),
    ),
    (
        "minecraft-cards.py",
        include_bytes!("../../../scripts/minecraft-cards.py"),
    ),
];

fn launch_args(scripts: &str, game: &str, project: &str) -> Vec<String> {
    vec![
        format!("{scripts}/minecraft-seeded.py"),
        game.into(),
        "--project-root".into(),
        project.into(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_uses_seeded_runner_and_preserves_paths_with_spaces() {
        assert_eq!(
            launch_args(
                "/project/runtime/minecraft/scripts",
                "/games/Minecraft Dungeons",
                "/my project"
            ),
            [
                "/project/runtime/minecraft/scripts/minecraft-seeded.py",
                "/games/Minecraft Dungeons",
                "--project-root",
                "/my project"
            ]
        );
        for name in [
            "minecraft-seeded.py",
            "minecraft-run.py",
            "minecraft-prepare.py",
            "minecraft-joysticks.py",
            "minecraft-cards.py",
        ] {
            assert!(
                LAUNCH_SCRIPTS
                    .iter()
                    .any(|(script, bytes)| *script == name && !bytes.is_empty())
            );
        }
    }
}
