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
    if !matches!(mode, "probe" | "launch" | "setup") {
        return Err("Unknown Minecraft operation".into());
    }
    let game = arcade_core::minecraft::directory(root)
        .canonicalize()
        .map_err(|e| format!("Minecraft game folder was not found: {e}"))?;
    let executable = game.join("gamedump.exec");
    if mode != "setup" {
        arcade_core::minecraft::verify_dump(&executable)?;
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
    let mut child = command
        .args(["-", &game_arg, &output_arg, mode])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Cannot start Minecraft runner: {e}"))?;
    child
        .stdin
        .take()
        .ok_or("Guest stdin unavailable")?
        .write_all(include_bytes!("../../../scripts/minecraft-run.py"))
        .map_err(|e| e.to_string())?;
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
