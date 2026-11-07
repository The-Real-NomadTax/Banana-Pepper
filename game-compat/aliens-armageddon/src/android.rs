use banana_api::{AndroidLaunch, AndroidProfile};
use std::path::Path;
pub fn profile() -> AndroidProfile {
    AndroidProfile {
        executable: "game",
        requirements: &[
            "usr/local/bin/box64",
            "usr/local/bin/afbridge",
            "usr/bin/pulseaudio",
            "usr/local/bin/afaudio",
            "opt/arcade/legacy-libs/libPhysXCore.so.1",
        ],
        status: "Experimental on Android",
        verified: false,
    }
}

pub fn prepare(source: &Path, work: &Path, _tools: &Path) -> Result<AndroidLaunch, String> {
    let bindings = banana_input::controls::Bindings::default();
    let mut settings = banana_settings::Settings::default().game("aliens").clone();
    settings.players = 1;
    settings.inputs[0] = banana_input::pad::Method::KeyboardMouse;
    crate::prepare::prepare(
        &source.join("game"),
        &work.join("game"),
        &bindings.codes("aliens")?,
        &settings.input_assignment(),
        &settings.aim_modes,
    )?;
    let saves = work.join("pmuser");
    banana_runtime::android::seed(&source.join("g6/aa/pmuser"), &saves)?;
    let mut plan = AndroidLaunch::new("/pm", &["box64", "/pm/game", "-f640x480"]);
    plan.bind(source, "/pm");
    plan.bind(work.join("game"), "/pm/game");
    plan.bind(saves, "/pm/g6/aa/pmuser");
    plan.env("BOX64_LD_PRELOAD", "/opt/arcade/libarcade_linux_shim.so");
    plan.env("BOX64_LD_LIBRARY_PATH", "/opt/arcade/legacy-libs");
    plan.env("ARCADE_ALIENS", "1");
    plan.env("SDL_VIDEO_X11_WMCLASS", "BananaPepperAliens");
    plan.env("SDL_VIDEO_X11_XRANDR", "0");
    plan.env("SDL_VIDEO_X11_VIDMODE", "0");
    plan.env("SDL_VIDEO_X11_DGAMOUSE", "0");
    plan.env("SDL12COMPAT_OPENGL_SCALING", "0");
    plan.env("AF_PULSE_AUDIO", "1");
    plan.env("SDL_AUDIODRIVER", "pulseaudio");
    Ok(plan)
}
