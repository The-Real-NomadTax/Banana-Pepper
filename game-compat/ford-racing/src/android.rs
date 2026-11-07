use banana_api::{AndroidLaunch, AndroidProfile};
use std::path::Path;
pub fn profile() -> AndroidProfile {
    AndroidProfile {
        executable: "FordRacing/fordracing.exe",
        requirements: &[
            "usr/local/bin/box64",
            "usr/local/bin/afbridge",
            "usr/bin/pulseaudio",
            "usr/local/bin/afaudio",
            "opt/arcade/wine/bin/wine",
            "opt/arcade/x64-libs/libgcc_s.so.1",
            "opt/arcade/wine/lib/wine/x86_64-unix/ntdll.so",
        ],
        status: "Experimental Wine WoW64 runtime",
        verified: false,
    }
}

pub fn prepare(source: &Path, work: &Path, tools: &Path) -> Result<AndroidLaunch, String> {
    crate::prepare::prepare(source, work, &tools.join("inpout32.dll"))?;
    let ini = work.join("data/GameSettings.ini");
    let text = std::fs::read_to_string(&ini).map_err(|e| e.to_string())?;
    let text = text
        .lines()
        .map(|line| {
            if line.starts_with("Resolution=") {
                "Resolution=640x480"
            } else if line.starts_with("Fullscreen=") {
                "Fullscreen=off"
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\r\n");
    std::fs::write(ini, text).map_err(|e| e.to_string())?;
    let mut plan = AndroidLaunch::new(
        "/work/FordRacing",
        &[
            "box64",
            "/opt/arcade/wine/bin/wine",
            "/work/FordRacing/fordracing.exe",
        ],
    );
    plan.bind(work, "/work");
    plan.env("BOX64_LD_LIBRARY_PATH", "/opt/arcade/x64-libs");
    plan.env("BOX64_ANDROID_NOEXEC", "1");
    plan.env("AF_PULSE_AUDIO", "1");
    plan.env("WINEPREFIX", "/work/prefix");
    plan.env("WINEDLLOVERRIDES", "inpout32=n,b;mscoree,mshtml=");
    plan.env(
        "ARCADE_FORD_KEYS",
        &banana_input::controls::Bindings::default().env_value("ford")?,
    );
    Ok(plan)
}
