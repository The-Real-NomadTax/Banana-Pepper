//! WSL path conversion uses argument vectors, never interpolated shell paths.
#![cfg(windows)]
use std::{path::Path, process::Command};
fn command() -> Command {
    let mut c = Command::new("wsl.exe");
    if let Some(distro) = std::env::var_os("ARCADE_WSL_DISTRO") {
        c.arg("--distribution").arg(distro);
    }
    c.arg("--exec");
    c
}
fn path(p: &Path) -> Result<String, String> {
    let value = p
        .to_str()
        .ok_or("Path must be Unicode")?
        .trim_start_matches(r"\\?\");
    let out = command()
        .args(["wslpath", "-a", "-u", value])
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).into());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().into())
}
pub fn big_buck(project: &Path, game: &Path, probe: bool) -> Result<String, String> {
    let project = path(project)?;
    let game = path(game)?;
    let status = command()
        .arg("bash")
        .arg("-c")
        .arg("cd -- \"$1\" && exec target/wsl/debug/arcade-desktop \"$2\" \"$3\"")
        .arg("arcade-foundry")
        .arg(&project)
        .arg(if probe {
            "--probe-big-buck"
        } else {
            "--launch-big-buck"
        })
        .arg(game)
        .status()
        .map_err(|e| format!("Cannot launch WSL2: {e}"))?;
    if !status.success() {
        return Err(format!(
            "WSL Big Buck failed ({status}). Build the Linux launcher and i686 shim inside WSL; see docs/big-buck-windows.md"
        ));
    }
    Ok("WSL2 Big Buck session closed".into())
}
