use std::path::Path;

#[cfg(target_os = "linux")]
pub fn run(root: &Path, launch: bool) -> Result<String, String> {
    let materialized = banana_runtime::content::resolve(root)?;
    let root = materialized.as_path();
    let project = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let output = project.join("runtime/aliens");
    std::fs::create_dir_all(&output).map_err(|e| e.to_string())?;
    let controls = banana_input::controls::Bindings::load(&project.join("controls.cfg"))?;
    // Fullscreen is applied by the guest shim (single monitor), not the image.
    let settings = banana_runtime::game_settings(&project, "aliens")?;
    let prepared = crate::prepare::prepare(
        &root.join("linux based/Aliens Armageddon/game"),
        &output.join("game-native"),
        &controls.codes("aliens")?,
        settings.players > 1,
    )?;
    // The harness only runs the image this preparer just wrote.
    std::fs::write(output.join("game-native.sha256"), format!("{prepared}\n"))
        .map_err(|e| e.to_string())?;
    let mut command = std::process::Command::new("python3");
    command
        .arg(project.join("scripts/aliens-run.py"))
        .arg("--games-root")
        .arg(root);
    if launch {
        command.arg("--launch");
        command.envs(banana_runtime::pad_env(&project, "aliens")?);
        command.envs(banana_runtime::pad_key_env(&project, "aliens")?);
        banana_runtime::settings_env(&mut command, &settings);
    }
    let status = command.status().map_err(|e| e.to_string())?;
    if !status.success() {
        return Err(format!(
            "Aliens runtime exited with {status}; inspect runtime/aliens/game.log"
        ));
    }
    Ok("Aliens session completed; inspect runtime/aliens/game.log".into())
}

#[cfg(not(target_os = "linux"))]
pub fn run(_root: &Path, _launch: bool) -> Result<String, String> {
    Err("Aliens native runtime currently requires Linux".into())
}
