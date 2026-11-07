//! Windows Subsystem for Linux execution backend.
//!
//! WSL path conversion uses argument vectors, never interpolated shell paths.
use banana_api::{Availability, BackendKind, ExecutionBackend};
#[cfg(windows)]
use std::{path::Path, process::Command};

pub struct Wsl;
pub static BACKEND: Wsl = Wsl;

impl ExecutionBackend for Wsl {
    fn kind(&self) -> BackendKind {
        BackendKind::Wsl
    }
    fn name(&self) -> &'static str {
        "WSL2 / WSLg"
    }
    fn formats(&self) -> &'static [&'static str] {
        &["ELF"]
    }
    fn availability(&self) -> Availability {
        #[cfg(windows)]
        {
            match Command::new("wsl.exe").arg("--status").output() {
                Ok(out) if out.status.success() => Availability::Available,
                _ => {
                    Availability::Missing("wsl.exe is not installed or has no distribution".into())
                }
            }
        }
        #[cfg(not(windows))]
        {
            Availability::Unsupported
        }
    }
}

/// `wsl.exe [--distribution $ARCADE_WSL_DISTRO] --exec`, ready for a
/// program and its arguments.
#[cfg(windows)]
pub fn command() -> Command {
    let mut c = base_command();
    c.arg("--exec");
    c
}
#[cfg(windows)]
fn base_command() -> Command {
    let mut c = Command::new("wsl.exe");
    if let Some(distro) = std::env::var_os("ARCADE_WSL_DISTRO") {
        c.arg("--distribution").arg(distro);
    }
    c
}
/// Convert a Windows path to its absolute WSL path.
#[cfg(windows)]
pub fn path(p: &Path) -> Result<String, String> {
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
/// Run `program args...` inside the WSL checkout of `project`, with every
/// path argument already converted by [`path`].
#[cfg(windows)]
pub fn run_in_project(
    project: &Path,
    program: &str,
    args: &[String],
) -> Result<std::process::ExitStatus, String> {
    let project = path(project)?;
    base_command()
        .arg("--cd")
        .arg(project)
        .arg("--exec")
        .arg(program)
        .args(args)
        .status()
        .map_err(|e| format!("Cannot launch WSL2: {e}"))
}

/// Forward a registered game action through the compiled Linux registry host.
#[cfg(windows)]
pub fn run_game(
    project: &Path,
    host: &Path,
    game: &str,
    action: &str,
    source: &Path,
) -> Result<std::process::ExitStatus, String> {
    if !host.is_file() {
        return Err(
            "The compiled Linux game host is missing; build and package the WSL components first"
                .into(),
        );
    }
    run_in_project(
        project,
        &path(host)?,
        &[game.into(), action.into(), path(source)?],
    )
}
