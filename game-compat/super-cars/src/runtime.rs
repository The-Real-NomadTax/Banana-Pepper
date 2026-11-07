//! Fast and Furious Super Cars: Windows build run under Wine + DXVK by
//! scripts/supercars-run.py, with the Super Cars hook (game-compat/super-cars/hook) input adapter.
use std::path::{Path, PathBuf};

/// Game folders below a library root, newest layout first.
const FOLDERS: &[&str] = &[
    "The_Fast_and_the_Furious_SuperCars/The_Fast_and_the_Furious_SuperCars",
    "windows based/The_Fast_and_the_Furious_SuperCars/The_Fast_and_the_Furious_SuperCars",
    "The_Fast_and_the_Furious_SuperCars",
];

/// The folder holding sdaemon.exe for a library root or the game's own folder.
pub fn directory(root: &Path) -> PathBuf {
    FOLDERS
        .iter()
        .map(|f| root.join(f))
        .chain(std::iter::once(root.to_path_buf()))
        .find(|p| p.join("sdaemon.exe").is_file())
        .unwrap_or_else(|| root.join(FOLDERS[0]))
}

#[cfg(target_os = "linux")]
pub fn launch(root: &Path) -> Result<String, String> {
    let materialized = banana_runtime::content::resolve(root)?;
    let root = materialized.as_path();
    let project = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let controls = banana_input::controls::Bindings::load(&project.join("controls.cfg"))?;
    let settings = banana_runtime::game_settings(&project, "super_cars")?;
    let mut command = std::process::Command::new("python3");
    command
        .arg(project.join("scripts/supercars-run.py"))
        .arg("--games-root")
        .arg(root)
        .env("ARCADE_SUPERCARS_KEYS", controls.env_value("super_cars")?)
        .envs(banana_runtime::pad_env(&project, "super_cars")?);
    banana_runtime::settings_env(&mut command, &settings);
    let status = command
        .status()
        .map_err(|e| format!("Cannot launch the Super Cars Wine runtime: {e}"))?;
    if !status.success() {
        return Err(format!(
            "Super Cars runtime exited with {status}; inspect runtime/supercars/game.log"
        ));
    }
    Ok("Super Cars session closed; logs are in runtime/supercars/.".into())
}

#[cfg(not(target_os = "linux"))]
pub fn launch(_root: &Path) -> Result<String, String> {
    Err("Super Cars currently runs only on Linux through Wine".into())
}
