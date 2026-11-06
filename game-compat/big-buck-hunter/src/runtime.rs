//! Select the host backend while preserving a single cabinet/game profile.
use std::path::Path;
#[cfg(windows)]
pub fn run(root: &Path, probe: bool) -> Result<String, String> {
    let materialized = banana_runtime::content::resolve(root)?;
    let root = materialized.as_path();
    use banana_settings::Backend;
    use std::{fs, process::Command};
    let project = banana_runtime::project_root()?;
    let settings = banana_runtime::game_settings(&project, "big_buck")?;
    let backend = match std::env::var("ARCADE_BBH_BACKEND").as_deref() {
        Ok("water" | "native") => Backend::Water,
        Ok("wsl") => Backend::Wsl,
        Ok("auto") | Err(_) => settings.backend,
        Ok(other) => {
            return Err(format!(
                "Unknown Big Buck backend {other}; use water or wsl"
            ));
        }
    };
    let game = crate::prepare::directory(root)
        .canonicalize()
        .map_err(|e| format!("Big Buck game folder: {e}"))?;
    if backend == Backend::Wsl {
        return wsl(&project, &game, probe);
    }
    let worker = banana_runtime::resource_path(
        &project,
        "helpers/water-buck.exe",
        "target/i686-pc-windows-msvc/release/water-buck.exe",
    );
    if !worker.is_file() {
        return Err("Build Water first: just build".into());
    }
    let output = project.join(if probe {
        "runtime/big-buck-native-probe"
    } else {
        "runtime/big-buck-native"
    });
    fs::create_dir_all(&output).map_err(|e| e.to_string())?;
    // Water only runs the staged, fingerprint-verified copy.
    crate::prepare::prepare(&game.join("game"), &output.join("game"))?;
    crate::prepare::seed_saves(&game.join("bbh3user"), &output.join("bbh3user"))
        .map_err(|e| e.to_string())?;
    let mut command = Command::new(worker);
    command.arg(game).arg("--work").arg(&output);
    command.env("ARCADE_AUDIO", if settings.audio { "1" } else { "0" });
    command.env("ARCADE_RESOLUTION", settings.resolution_text());
    command.env(
        "ARCADE_FULLSCREEN",
        if settings.fullscreen { "1" } else { "0" },
    );
    command.env("ARCADE_VSYNC", settings.vsync.id());
    command.env(
        "ARCADE_RENDERER",
        if settings.renderer == banana_settings::Renderer::Software {
            "software"
        } else {
            "gpu"
        },
    );
    command.env(
        "ARCADE_BBH_KEYS",
        banana_input::controls::Bindings::load(&project.join("controls.cfg"))?
            .env_value("big_buck")?,
    );
    command.envs(banana_runtime::pad_env(&project, "big_buck")?);
    command.env("ARCADE_PLAYERS", settings.players.to_string());
    command.env("ARCADE_INPUTS", settings.inputs_env());
    match settings.crosshair.0 {
        Some(style) => command
            .env("ARCADE_CROSSHAIR", style)
            .env("ARCADE_CROSSHAIR_SIZE", settings.crosshair_size.to_string()),
        None => command
            .env_remove("ARCADE_CROSSHAIR")
            .env_remove("ARCADE_CROSSHAIR_SIZE"),
    };
    command
        .env_remove("ARCADE_NATIVE_TRACE")
        .env_remove("ARCADE_NATIVE_PROBE")
        .env_remove("ARCADE_NATIVE_CAPTURE_ALL");
    if probe {
        command
            .env("ARCADE_AUDIO", "0")
            .env("ARCADE_NATIVE_PROBE", "1")
            .arg("--timeout")
            .arg("96")
            .env("ARCADE_RENDERER", "gpu")
            .env("ARCADE_RESOLUTION", "640x480")
            .env("ARCADE_FULLSCREEN", "0");
    }
    let status = command.status().map_err(|e| e.to_string())?;
    if !status.success() {
        return Err(format!(
            "Water Big Buck session failed ({status}); inspect {}",
            output.join("runner.log").display()
        ));
    }
    Ok(format!(
        "Water Big Buck session closed. Logs and captures: {}",
        output.display()
    ))
}

/// The compiled registry host inside WSL, built by `just build-wsl`.
#[cfg(windows)]
fn wsl(project: &Path, game: &Path, probe: bool) -> Result<String, String> {
    let status = banana_wsl::run_game(
        project,
        &banana_runtime::linux_host(project),
        "big-buck",
        if probe { "probe" } else { "launch" },
        game,
    )?;
    if !status.success() {
        return Err(format!(
            "WSL Big Buck failed ({status}). Build the Linux launcher and i686 shim inside WSL; see docs/big-buck-windows.md"
        ));
    }
    Ok("WSL2 Big Buck session closed".into())
}

#[cfg(not(windows))]
pub fn run(root: &Path, probe: bool) -> Result<String, String> {
    let materialized = banana_runtime::content::resolve(root)?;
    let root = materialized.as_path();
    if probe {
        crate::linux::probe_big_buck(
            root,
            &banana_runtime::project_root()?.join("runtime/big-buck"),
        )
    } else {
        crate::linux::launch_big_buck(root)
    }
}
