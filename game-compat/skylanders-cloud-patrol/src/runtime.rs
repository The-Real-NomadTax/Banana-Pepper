//! Skylanders Cloud Patrol: the Windows Unity build run under Wine (or
//! natively on Windows) from a staged copy, with the touch adapter in place
//! of the PQ Labs client. Saves, the Wine prefix and logs live in
//! runtime/skylanders.
use std::{fs, path::Path, process::Command};

fn project() -> Result<std::path::PathBuf, String> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .map_err(|e| e.to_string())
}

/// Unity player arguments for the configured window.
fn player_args(settings: &banana_settings::GameSettings) -> Vec<String> {
    vec![
        "-screen-width".into(),
        settings.resolution.0.to_string(),
        "-screen-height".into(),
        settings.resolution.1.to_string(),
        "-screen-fullscreen".into(),
        if settings.fullscreen { "1" } else { "0" }.into(),
    ]
}

pub fn launch(root: &Path) -> Result<String, String> {
    let materialized = banana_runtime::content::resolve(root)?;
    let project = project()?;
    let source = crate::prepare::directory(materialized.as_path());
    let output = project.join("runtime/skylanders");
    fs::create_dir_all(&output).map_err(|e| e.to_string())?;
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(output.join("session.lock"))
        .map_err(|e| e.to_string())?;
    lock.try_lock()
        .map_err(|e| format!("Another Skylanders session is running: {e}"))?;
    let game = output.join("game");
    crate::prepare::stage(
        &source,
        &game,
        &project.join("target/i686-pc-windows-msvc/release/PQMTClient.dll"),
    )?;
    let settings = banana_runtime::game_settings(&project, "skylanders")?;
    let controls = banana_input::controls::Bindings::load(&project.join("controls.cfg"))?;

    #[cfg(windows)]
    let mut command = Command::new(game.join(crate::prepare::EXECUTABLE));
    #[cfg(not(windows))]
    let mut command = {
        let mut c = Command::new(std::env::var_os("ARCADE_WINE").unwrap_or_else(|| "wine".into()));
        c.arg(game.join(crate::prepare::EXECUTABLE));
        let prefix = output.join("prefix");
        fs::create_dir_all(&prefix).map_err(|e| e.to_string())?;
        // No Wine Mono/Gecko prompts (Unity ships its own Mono). With audio
        // off, Wine has no sound driver and Unity runs silently.
        let mut overrides = String::from("mscoree,mshtml=");
        if !settings.audio {
            overrides.push_str(";winepulse.drv,winealsa.drv,wineoss.drv=d");
        }
        c.env("WINEPREFIX", prefix)
            .env("WINEDLLOVERRIDES", overrides)
            .env("WINEDEBUG", std::env::var("WINEDEBUG").unwrap_or_else(|_| "-all".into()));
        if !settings.audio_sink.is_empty() {
            c.env("PULSE_SINK", &settings.audio_sink);
        }
        c
    };
    command
        .args(player_args(&settings))
        .current_dir(&game)
        .envs(settings.guest_env())
        .env("ARCADE_INPUTS", settings.inputs_env())
        .env("ARCADE_SKYLANDERS_KEYS", controls.env_value("skylanders")?)
        .envs(banana_runtime::pad_env(&project, "skylanders")?);
    let status = command
        .status()
        .map_err(|e| format!("Cannot start Skylanders under Wine: {e}"))?;
    if !status.success() {
        return Err(format!(
            "Skylanders exited with {status}; inspect runtime/skylanders/game/Skylanders_Data/output_log.txt and arcade-skylanders.log"
        ));
    }
    Ok("Skylanders session closed; saves and logs are under runtime/skylanders.".into())
}

#[cfg(test)]
mod tests {
    #[test]
    fn player_args_follow_settings() {
        let profile = &crate::settings::SETTINGS;
        let mut settings = banana_settings::GameSettings::defaults(profile);
        settings.resolution = (1280, 720);
        settings.fullscreen = true;
        assert_eq!(
            super::player_args(&settings),
            ["-screen-width", "1280", "-screen-height", "720", "-screen-fullscreen", "1"]
        );
    }
}
