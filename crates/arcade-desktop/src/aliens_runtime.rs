use std::path::Path;

#[cfg(target_os = "linux")]
pub fn run(root: &Path, launch: bool) -> Result<String, String> {
    let project = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let output = project.join("runtime/aliens");
    std::fs::create_dir_all(&output).map_err(|e| e.to_string())?;
    arcade_core::aliens::prepare(
        &root.join("linux based/Aliens Armageddon/game"),
        &output.join("game-native"),
    )?;
    let mut command = std::process::Command::new("python3");
    command
        .arg(project.join("scripts/aliens-run.py"))
        .arg("--games-root")
        .arg(root);
    if launch {
        command.arg("--launch");
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
