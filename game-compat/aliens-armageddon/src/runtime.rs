//! Aliens preparation, cabinet policy and native session, entirely in Rust.
use banana_runtime::{
    Sandbox, process,
    session::{self, Plan, Program},
};
use std::{fs, path::Path};

pub fn run(root: &Path, launch: bool) -> Result<String, String> {
    if !cfg!(target_os = "linux") {
        return Err("Aliens native runtime currently requires Linux".into());
    }
    let root = banana_runtime::content::resolve(root)?
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let project = banana_runtime::project_root()?;
    let game = root.join("linux based/Aliens Armageddon");
    let output = project.join("runtime/aliens");
    let _lock = process::lock(&output)?;
    let controls = banana_input::controls::Bindings::load(&project.join("controls.cfg"))?;
    let settings = banana_runtime::game_settings(&project, "aliens")?;
    let prepared = crate::prepare::prepare(
        &game.join("game"),
        &output.join("game-native"),
        &controls.codes("aliens")?,
        &settings.input_assignment(),
        &settings.aim_modes,
    )?;
    if !output.join("pmuser").exists() {
        process::copy_tree(&game.join("g6/aa/pmuser"), &output.join("pmuser"))?;
    }
    let log_start = fs::metadata(output.join("pmuser/gamelog.txt"))
        .map(|m| m.len())
        .unwrap_or(0) as usize;
    let shim = banana_runtime::resource_path(
        &project,
        "adapters/linux-x86/libarcade_linux_shim.so",
        "target/i686-unknown-linux-gnu/debug/libarcade_linux_shim.so",
    );
    fs::copy(shim, output.join("shim.so")).map_err(|e| e.to_string())?;
    let mut sandbox = Sandbox::new();
    sandbox
        .bind(&game, "/pm", true)
        .bind(output.join("game-native"), "/pm/game", true)
        .bind(&output, "/work", false)
        .bind(output.join("pmuser"), "/pm/g6/aa/pmuser", false)
        .bind(output.join("shim.so"), "/shim.so", true)
        .bind(
            banana_runtime::dependencies::legacy_directory(&project),
            "/runtime-libs",
            true,
        )
        .env("LD_LIBRARY_PATH", "/runtime-libs:/usr/lib32")
        .env("ARCADE_ALIENS", "1")
        .env("SDL_VIDEO_X11_WMCLASS", "BananaPepperAliens")
        .env("SDL_VIDEO_X11_XRANDR", "0")
        .env("SDL_VIDEO_X11_VIDMODE", "0")
        .env("SDL_VIDEO_X11_DGAMOUSE", "0")
        .chdir("/pm");
    sandbox.desktop()?.gpu();
    sandbox.inert_tools(
        &output,
        &["nvidia-settings", "xrandr", "reboot", "shutdown"],
    )?;
    let audio = sandbox.audio(launch && settings.audio, &settings.audio_sink, "pulse");
    if launch {
        banana_runtime::sandbox_settings(&mut sandbox, &project, "aliens", &settings)?;
        for (key, value) in banana_runtime::pad_key_env(&project, "aliens")? {
            sandbox.env(key, value);
        }
    }
    let mut game_command = Program::new(
        "/pm/game",
        &[&format!(
            "-f{}",
            if launch {
                settings.resolution_text()
            } else {
                "1360x768".into()
            }
        )],
    );
    game_command
        .env
        .insert("LD_PRELOAD".into(), "/shim.so".into());
    if !launch {
        game_command = Program::new(
            "strace",
            &[
                "-f",
                "-s",
                "180",
                "-o",
                "/work/startup.strace",
                "-E",
                "LD_PRELOAD=/shim.so",
                "/pm/game",
                "-f1360x768",
            ],
        );
    }
    let mut plan = Plan::new(game_command, "[Aa][Ll][Ii][Ee][Nn][Ss]");
    plan.timeout_ms = (!launch).then_some(45000);
    plan.capture_until_ms = 40000;
    for e in fs::read_dir(&output).map_err(|e| e.to_string())? {
        let e = e.map_err(|e| e.to_string())?;
        if e.file_name().to_string_lossy().starts_with("screen-")
            && e.path().extension().is_some_and(|x| x == "png")
        {
            fs::remove_file(e.path()).map_err(|e| e.to_string())?;
        }
    }
    let _ = fs::remove_file(output.join("screen.png"));
    let code = session::launch(&mut sandbox, &project, &output, &plan)?;
    let log = fs::read_to_string(output.join("game.log")).unwrap_or_default();
    let initialization = fs::read(output.join("pmuser/gamelog.txt")).unwrap_or_default();
    let initialization =
        String::from_utf8_lossy(initialization.get(log_start..).unwrap_or_default());
    let closed_by = if log.contains("[aliens] Esc pressed; exiting") {
        Some("escape")
    } else if log.contains("X connection to") || log.contains("XIO:") {
        Some("window")
    } else {
        None
    };
    fs::write(output.join("initialization.log"), initialization.as_bytes())
        .map_err(|e| e.to_string())?;
    let report = serde_json::json!({"exit_code":code, "gameplay_verified":false, "audio_driver":audio,
        "closed_by":closed_by, "source_sha256":crate::prepare::ORIGINAL_SHA256, "runtime_sha256":prepared,
        "window_captured":output.join("screen.png").is_file(),
        "dongle_initialization_failed":initialization.contains("DongleInitialize() FAILED!"),
        "rtdt_definitions_failed":initialization.contains("Error Loading RTDT Defs File"),
        "segmentation_fault":initialization.contains("FATAL signal SIGSEGV"), "log":output.join("game.log")});
    fs::write(
        output.join("result.json"),
        serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    if code != 0 && !(launch && closed_by.is_some()) && !(!launch && code == 124) {
        return Err(format!(
            "Aliens exited with {code}; inspect {}",
            output.join("game.log").display()
        ));
    }
    Ok(format!(
        "Aliens session completed; logs: {}",
        output.display()
    ))
}
