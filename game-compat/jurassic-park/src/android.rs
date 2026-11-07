use banana_api::{AndroidLaunch, AndroidProfile};
use std::path::Path;
pub fn profile() -> AndroidProfile {
    AndroidProfile {
        executable: "game_legacy_patched",
        requirements: &[
            "usr/local/bin/box64",
            "usr/local/bin/afbridge",
            "usr/bin/pulseaudio",
            "usr/local/bin/afaudio",
            "opt/arcade/legacy-libs/libPhysXCore.so.1",
            "opt/arcade/legacy-libs/libtheora.so.0",
        ],
        status: "Experimental on Android",
        verified: false,
    }
}

pub fn prepare(source: &Path, work: &Path, _tools: &Path) -> Result<AndroidLaunch, String> {
    fn check_scripts(root: &Path, backups: &Path) -> Result<usize, String> {
        let mut count = 0;
        for entry in std::fs::read_dir(root).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let kind = entry.file_type().map_err(|e| e.to_string())?;
            if kind.is_dir() {
                count += check_scripts(&entry.path(), &backups.join(entry.file_name()))?;
            } else if entry.path().extension().is_some_and(|s| s == "txt") {
                let data =
                    std::fs::read(backups.join(entry.file_name())).map_err(|e| e.to_string())?;
                if !data.starts_with(b"//\n") {
                    return Err("Missing plaintext script backup".into());
                }
                count += 1;
            }
        }
        Ok(count)
    }
    if check_scripts(
        &source.join("g5/dino/data/programs_enc"),
        &source.join("programs_dec"),
    )? == 0
    {
        return Err("No game scripts found".into());
    }
    crate::prepare::prepare_desktop(&source.join("game_legacy_patched"), &work.join("game"))?;
    let saves = work.join("pmuser");
    banana_runtime::android::seed(&source.join("pmuser"), &saves)?;
    let mut plan = AndroidLaunch::new(
        "/pm",
        &[
            "/bin/bash",
            "-c",
            "mkdir -p /etc/udev/rules.d && exec box64 /pm/game -f1360x768",
        ],
    );
    plan.bind(source, "/pm");
    plan.bind(work.join("game"), "/pm/game");
    for dest in ["/pmuser", "/pm/pmuser", "/pm/g5/dino/pmuser"] {
        plan.bind(&saves, dest);
    }
    plan.bind(source.join("programs_dec"), "/pm/g5/dino/data/programs_enc");
    for name in [
        "shader108",
        "shader108b",
        "shader108c_sk",
        "shader108d",
        "shader108d_sk",
        "include",
    ] {
        let relative = format!(
            "{name}/{}",
            if name == "include" {
                "frag_bloom.gls"
            } else {
                "frag.gls"
            }
        );
        let data = std::fs::read_to_string(source.join("g5/shaders").join(&relative))
            .map_err(|e| e.to_string())?;
        let patched = if name == "include" {
            if data.matches("out vec4 out_Bloom;").count() != 1 {
                return Err("Unsupported bloom shader".into());
            }
            data.replace("out vec4 out_Bloom;", "#ifndef ARCADE_BLOOM_OUTPUT_DECLARED\n#define ARCADE_BLOOM_OUTPUT_DECLARED\nout vec4 out_Bloom;\n#endif")
        } else {
            if !data.contains("gl_FragData[0]") || !data.contains("gl_FragData[1]") {
                return Err("Unsupported output shader".into());
            }
            format!(
                "out vec4 out_Color;\n#ifndef ARCADE_BLOOM_OUTPUT_DECLARED\n#define ARCADE_BLOOM_OUTPUT_DECLARED\nout vec4 out_Bloom;\n#endif\n{}",
                data.replace("gl_FragData[0]", "out_Color")
                    .replace("gl_FragData[1]", "out_Bloom")
            )
        };
        let out = work.join("shaders").join(&relative);
        std::fs::create_dir_all(out.parent().unwrap()).map_err(|e| e.to_string())?;
        std::fs::write(&out, patched).map_err(|e| e.to_string())?;
        plan.bind(out, &format!("/pm/g5/shaders/{relative}"));
    }
    plan.env("BOX64_LD_PRELOAD", "/opt/arcade/libarcade_linux_shim.so");
    plan.env("BOX64_LD_LIBRARY_PATH", "/opt/arcade/legacy-libs");
    plan.env("LD_BIND_NOW", "1");
    plan.env("ARCADE_JURASSIC", "1");
    plan.env("ARCADE_JURASSIC_LAUNCH", "1");
    plan.env("ARCADE_JURASSIC_DESKTOP", "1");
    plan.env("SDL12COMPAT_OPENGL_SCALING", "0");
    plan.env("ARCADE_RESOLUTION", "1360x768");
    plan.env("AF_FRAME_FPS", "30");
    plan.env("AF_PULSE_AUDIO", "1");
    plan.env("SDL_AUDIODRIVER", "pulseaudio");
    Ok(plan)
}
