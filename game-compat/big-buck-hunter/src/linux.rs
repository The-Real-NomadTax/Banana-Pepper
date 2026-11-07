//! Native Linux sessions and typed startup probe events for Big Buck.
use banana_runtime::{
    Sandbox, process,
    session::{self, Action, Event, Plan, Program},
};
use std::{fs, path::Path};

fn run(games_root: &Path, output: &Path, desktop: bool) -> Result<String, String> {
    if !cfg!(target_os = "linux") {
        return Err("Big Buck native execution requires Linux".into());
    }
    let root = banana_runtime::content::resolve(games_root)?;
    let game = crate::prepare::directory(&root)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let _lock = process::lock(output)?;
    let output = output.canonicalize().map_err(|e| e.to_string())?;
    let project = banana_runtime::project_root()?;
    let settings = if desktop {
        banana_runtime::game_settings(&project, "big_buck")?
    } else {
        banana_settings::GameSettings {
            audio: false,
            ..banana_settings::GameSettings::defaults(&crate::settings::SETTINGS)
        }
    };
    let hash = crate::prepare::prepare(&game.join("game"), &output.join("game"))?;
    crate::prepare::seed_saves(&game.join("bbh3user"), &output.join("bbh3user"))
        .map_err(|e| e.to_string())?;
    let shim = banana_runtime::resource_path(
        &project,
        "adapters/linux-x86/libarcade_linux_shim.so",
        "target/i686-unknown-linux-gnu/debug/libarcade_linux_shim.so",
    );
    fs::copy(shim, output.join("libarcade_linux_shim.so")).map_err(|e| e.to_string())?;
    for name in ["screen.png", "windows.txt", "game-exit-code.txt"] {
        let _ = fs::remove_file(output.join(name));
    }
    let mut s = Sandbox::new();
    s.bind(&game, "/game", true)
        .bind(&output, "/work", false)
        .bind(output.join("game"), "/game/game", true)
        .bind(game.join("bbh3data"), "/bbh3data", true)
        .bind(output.join("bbh3user"), "/bbh3user", false)
        .chdir("/game");
    let audio = s.audio(desktop && settings.audio, &settings.audio_sink, "pulse");
    s.env("ARCADE_BBH_AUDIO", if audio == "dummy" { "0" } else { "1" });
    let mut program = Program::new("/game/game", &[]);
    program
        .env
        .insert("LD_PRELOAD".into(), "/work/libarcade_linux_shim.so".into());
    program.env.insert("ARCADE_BBH_ADAPTER".into(), "1".into());
    let mut plan = Plan::new(program, "Banana pepper - Big Buck Hunter Pro");
    if desktop {
        s.desktop()?;
        if settings.renderer == banana_settings::Renderer::Gpu {
            s.gpu();
        }
        banana_runtime::sandbox_settings(&mut s, &project, "big_buck", &settings)?;
        s.env(
            "ARCADE_BBH_KEYS",
            banana_input::controls::Bindings::load(&project.join("controls.cfg"))?
                .env_value("big_buck")?,
        );
    } else {
        let xvfb = Path::new("/usr/bin/Xvfb");
        if !xvfb.is_file() {
            return Err("Xvfb is required for private startup diagnostics".into());
        }
        s.bind(xvfb, "/Xvfb", true)
            .env("DISPLAY", ":0")
            .env("LIBGL_ALWAYS_SOFTWARE", "1")
            .env("__GLX_VENDOR_LIBRARY_NAME", "mesa")
            .env(
                "__EGL_VENDOR_LIBRARY_FILENAMES",
                "/usr/share/glvnd/egl_vendor.d/50_mesa.json",
            );
        plan.private_display = Some(("/Xvfb".into(), 1024, 768));
        plan.timeout_ms = Some(45000);
        plan.capture_every_ms = None;
        plan.game = Program::new(
            "strace",
            &[
                "-f",
                "-s",
                "160",
                "-o",
                "/work/startup.strace",
                "-e",
                "trace=open,openat,connect,ioctl,write,execve",
                "-E",
                "ARCADE_BBH_ADAPTER=1",
                "-E",
                "LD_PRELOAD=/work/libarcade_linux_shim.so",
                "/game/game",
            ],
        );
        plan.events = probe_events();
    }
    let code = session::launch(&mut s, &project, &output, &plan)?;
    let opened = fs::read_to_string(output.join("windows.txt")).is_ok_and(|s| !s.trim().is_empty())
        && output.join("screen.png").is_file();
    fs::write(output.join("result.json"),serde_json::to_vec_pretty(&serde_json::json!({"source_sha256":crate::prepare::SOURCE_SHA256,"prepared_sha256":hash,"exit_code":code,"window_created":opened,"gameplay_verified":false,"display":if desktop{"desktop"}else{"private Xvfb"}})).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
    if code != 0 && !(code == 124 && !desktop) {
        return Err(format!(
            "Big Buck exited with {code}; inspect {}",
            output.join("game.log").display()
        ));
    }
    if !desktop && !opened {
        return Err(format!(
            "No game window captured; inspect {}",
            output.display()
        ));
    }
    Ok(format!(
        "Big Buck session closed; logs and captures: {}",
        output.display()
    ))
}
fn probe_events() -> Vec<Event> {
    let mut out = Vec::new();
    let mut event = |second: u64, action| {
        out.push(Event {
            at_ms: second * 1000,
            action,
        })
    };
    event(2, Action::Key("t".into()));
    event(3, Action::Capture("tracking.png".into()));
    event(3, Action::Key("1".into()));
    event(4, Action::Capture("after-start.png".into()));
    event(4, Action::Key("1".into()));
    event(6, Action::Capture("after-start2.png".into()));
    event(6, Action::Key("5".into()));
    event(7, Action::Key("1".into()));
    for (time, name, x, y) in [
        (10, "game-select.png", 160, 220),
        (12, "after-shot.png", 160, 220),
        (15, "mode-select.png", 160, 140),
        (18, "adventure-select.png", 160, 180),
        (21, "adventure-intro.png", 160, 180),
        (24, "records.png", 160, 180),
    ] {
        event(time, Action::Capture(name.into()));
        event(time, Action::Click(x, y, 1));
    }
    event(27, Action::Capture("instructions.png".into()));
    event(32, Action::Click(320, 240, 1));
    event(34, Action::Capture("site-select.png".into()));
    event(34, Action::Click(120, 150, 1));
    event(38, Action::Capture("hunt.png".into()));
    out
}
pub fn probe_big_buck(root: &Path, output: &Path) -> Result<String, String> {
    run(root, output, false)
}
pub fn launch_big_buck(root: &Path) -> Result<String, String> {
    run(
        root,
        &banana_runtime::project_root()?.join("runtime/big-buck-desktop"),
        true,
    )
}
