//! Game-specific Wine preparation and launch policy. No interpreted harness.
use banana_runtime::process;
use std::{
    fs,
    path::{Path, PathBuf},
};

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

pub fn launch(root: &Path) -> Result<String, String> {
    if !cfg!(target_os = "linux") {
        return Err("Super Cars currently runs on Linux through Wine".into());
    }
    let root = banana_runtime::content::resolve(root)?
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let project = banana_runtime::project_root()?;
    let game = directory(&root);
    let bytes = fs::read(game.join("sdaemon.exe")).map_err(|e| e.to_string())?;
    if banana_runtime::dependencies::sha256(&bytes)
        != "0cddbdbf1d107d27dc668897d7851b6ac96b4349320551d800504b1c50b195ba"
    {
        return Err("Unsupported Super Cars build".into());
    }
    let settings = banana_runtime::game_settings(&project, "super_cars")?;
    let controls = banana_input::controls::Bindings::load(&project.join("controls.cfg"))?;
    let output = project.join("runtime/supercars");
    let _lock = process::lock(&output)?;
    let hook = banana_runtime::resource_path(
        &project,
        "adapters/windows-x86/arcade_wine_hook.dll",
        "target/i686-pc-windows-msvc/release/arcade_wine_hook.dll",
    );
    let mut wine =
        banana_wine::Prefix::new(&output.join("prefix"), &output.join("game.log"), "win64")?;
    let rawart = wine.path.join("drive_c/rawart");
    map_game(&game, &rawart)?;
    fs::copy(hook, rawart.join("dinput8.dll")).map_err(|e| e.to_string())?;
    let software = settings.renderer == banana_settings::Renderer::Software;
    let dxvk = std::env::var("ARCADE_SUPERCARS_DXVK").as_deref() == Ok("1") && !software;
    if dxvk {
        let directory =
            banana_wine::find_dxvk().ok_or("32-bit DXVK d3d8/d3d9 not found; set ARCADE_DXVK")?;
        for name in ["d3d8.dll", "d3d9.dll"] {
            fs::copy(
                directory.join(name),
                wine.path.join("drive_c/windows/syswow64").join(name),
            )
            .map_err(|e| e.to_string())?;
        }
    }
    let mut overrides = format!(
        "dinput8=n,b;mscoree,mshtml=;d3d8,d3d9={}",
        if dxvk { "n,b" } else { "b" }
    );
    if !settings.audio {
        overrides.push_str(";winepulse.drv,winealsa.drv=");
    }
    wine.env.insert("WINEDLLOVERRIDES".into(), overrides);
    for (k, v) in settings.guest_env() {
        if !k.starts_with("ARCADE_") {
            wine.env.insert(k.into(), v);
        }
    }
    if !settings.audio_sink.is_empty() {
        wine.env
            .insert("PULSE_SINK".into(), settings.audio_sink.clone());
    }
    wine.env.insert(
        "ARCADE_SUPERCARS_KEYS".into(),
        controls.env_value("super_cars")?,
    );
    wine.env
        .insert("ARCADE_RESOLUTION".into(), settings.resolution_text());
    wine.env
        .insert("ARCADE_INPUTS".into(), settings.inputs_env());
    for (k, v) in banana_runtime::pad_env(&project, "super_cars")? {
        wine.env.insert(k.into(), v);
    }
    if dxvk && settings.vsync != banana_settings::VSync::Default {
        wine.env.insert(
            "DXVK_CONFIG".into(),
            format!(
                "d3d9.presentInterval = {}",
                if settings.vsync == banana_settings::VSync::On {
                    1
                } else {
                    0
                }
            ),
        );
    }
    wine.env.insert(
        "DXVK_LOG_PATH".into(),
        output.to_string_lossy().into_owned(),
    );
    wine.env.insert("DXVK_LOG_LEVEL".into(), "warn".into());
    wine.desktop(
        "SuperCars",
        &settings.resolution_text(),
        settings.fullscreen,
    )?;
    if !WINDOW_SIZES.contains(&settings.resolution) {
        return Err("Unsupported Super Cars resolution".into());
    }
    let mut command = wine.command();
    command
        .args(["sdaemon.exe", "-noerr", RENDER_OPTION])
        .current_dir(&rawart);
    let code = wine.run(&mut command)?;
    if rawart.join("arcade-hook.log").is_file() {
        fs::copy(
            rawart.join("arcade-hook.log"),
            output.join("arcade-hook.log"),
        )
        .map_err(|e| e.to_string())?;
    }
    if !code.success() {
        return Err(format!(
            "Super Cars exited with {code}; inspect {}",
            output.join("game.log").display()
        ));
    }
    Ok(format!(
        "Super Cars session closed; logs: {}",
        output.display()
    ))
}
/// Window sizes offered for windowed play (the Wine virtual desktop).
const WINDOW_SIZES: &[(u32, u32)] = &[
    (640, 480),
    (800, 600),
    (1024, 768),
    (1280, 1024),
    (1360, 768),
    (1600, 1200),
    (1920, 1080),
];
/// The game's own -res option for 1360x768. With the operator setting Stretch
/// Screen it lays out each frame in 1360x768 and Present stretches that area
/// to the screen-sized backbuffer. Larger -res sizes enlarge the intro movie
/// and the race view but not that area, so they appear zoomed in.
const RENDER_OPTION: &str = "-res13";
fn map_game(game: &Path, destination: &Path) -> Result<(), String> {
    fs::create_dir_all(destination).map_err(|e| e.to_string())?;
    for e in fs::read_dir(game).map_err(|e| e.to_string())? {
        let e = e.map_err(|e| e.to_string())?;
        let name = e.file_name();
        let src = e.path();
        let dst = destination.join(&name);
        if [
            "MohkerzLoaderz.exe",
            "MohkerzLoaderz.ini",
            "MinHook.x86.dll",
            "teknoparrot.ini",
            "dxwnd.dxw",
            "errorlog.txt",
            "messages.txt",
            "dinput8.dll",
        ]
        .contains(&name.to_string_lossy().as_ref())
        {
            continue;
        }
        let small = src.is_file()
            && fs::metadata(&src).map_err(|e| e.to_string())?.len() < 1 << 20
            && !["exe", "dll"].contains(
                &src.extension()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_ascii_lowercase()
                    .as_str(),
            );
        if small {
            if !dst.exists() || dst.is_symlink() {
                if dst.is_symlink() {
                    fs::remove_file(&dst).map_err(|e| e.to_string())?;
                }
                fs::copy(&src, &dst).map_err(|e| e.to_string())?;
            }
        } else {
            link(&src, &dst)?;
        }
    }
    Ok(())
}
fn link(source: &Path, destination: &Path) -> Result<(), String> {
    if fs::read_link(destination).is_ok_and(|p| p == source) {
        return Ok(());
    }
    if destination.exists() || destination.is_symlink() {
        if destination.is_dir() && !destination.is_symlink() {
            return Err(format!("Unexpected directory {}", destination.display()));
        }
        fs::remove_file(destination).map_err(|e| e.to_string())?;
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(source, destination).map_err(|e| e.to_string())?;
    }
    #[cfg(not(unix))]
    {
        return Err("Wine game mapping requires Linux".into());
    }
    #[allow(unreachable_code)]
    Ok(())
}
