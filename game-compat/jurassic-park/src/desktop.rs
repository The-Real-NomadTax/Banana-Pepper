//! Jurassic's desktop asset, shader and save policy. Shared Rust mechanisms
//! handle isolation, input hand-off, audio, captures and process lifetime.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]
use banana_runtime::{
    Sandbox, process,
    session::{self, Plan, Program},
};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub fn launch(root: &Path) -> Result<String, String> {
    let root = banana_runtime::content::resolve(root)?;
    let project = banana_runtime::project_root()?;
    let game = crate::prepare::directory(&root)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let runtime = project.join("runtime/jurassic");
    let _lock = process::lock(&runtime)?;
    crate::prepare::prepare_desktop(
        &game.join("game_legacy_patched"),
        &runtime.join("game-desktop"),
    )?;
    let output = runtime.join("desktop");
    fs::create_dir_all(&output).map_err(|e| e.to_string())?;
    let programs = validate_programs(&game)?;
    let overrides = shaders(&game, &output)?;
    let controls = banana_input::controls::Bindings::load(&project.join("controls.cfg"))?;
    let settings = banana_runtime::game_settings(&project, "jurassic")?;
    let shim = banana_runtime::resource_path(
        &project,
        "adapters/linux-x86/libarcade_linux_shim.so",
        "target/i686-unknown-linux-gnu/debug/libarcade_linux_shim.so",
    );
    fs::copy(shim, output.join("shim.so")).map_err(|e| e.to_string())?;
    fs::write(
        output.join("usb.rules"),
        "# Cabinet USB automount disabled in this private filesystem.\n",
    )
    .map_err(|e| e.to_string())?;
    let mut s = Sandbox::new();
    s.bind(&game, "/pm", true)
        .bind(runtime.join("game-desktop"), "/pm/game", true)
        .bind(&output, "/work", false)
        .bind(output.join("shim.so"), "/shim.so", true)
        .bind(
            banana_runtime::dependencies::legacy_directory(&project),
            "/runtime-libs",
            true,
        )
        .bind(
            game.join("programs_dec"),
            "/pm/g5/dino/data/programs_enc",
            true,
        )
        .bind(
            output.join("usb.rules"),
            "/etc/udev/rules.d/10-usb-automount.rules",
            true,
        )
        .env("LD_LIBRARY_PATH", "/runtime-libs")
        .env("LD_BIND_NOW", "1")
        .env("ARCADE_JURASSIC", "1")
        .env("ARCADE_JURASSIC_LAUNCH", "1")
        .env("ARCADE_JURASSIC_DESKTOP", "1")
        .env("SDL12COMPAT_OPENGL_SCALING", "0")
        .env("ARCADE_JURASSIC_KEYS", controls.env_value("jurassic")?)
        .chdir("/pm");
    save_mounts(&mut s, &game, &output)?;
    for (source, destination) in overrides {
        s.bind(source, destination, true);
    }
    s.desktop()?.gpu();
    s.inert_tools(&output, &["nvidia-settings"])?;
    let audio = s.audio(settings.audio, &settings.audio_sink, "pulseaudio");
    banana_runtime::sandbox_settings(&mut s, &project, "jurassic", &settings)?;
    let mut program = Program::new("/pm/game", &[&format!("-f{}", settings.resolution_text())]);
    program.env.insert("LD_PRELOAD".into(), "/shim.so".into());
    let plan = Plan::new(program, "Jurassic Park Arcade");
    let _ = fs::remove_file(output.join("screen.png"));
    let code = session::launch(&mut s, &project, &output, &plan)?;
    let log = fs::read_to_string(output.join("game.log")).unwrap_or_default();
    fs::write(output.join("result.json"),serde_json::to_vec_pretty(&serde_json::json!({"mode":"desktop_startup","exit_code":code,"gameplay_verified":false,"dongle_supported":false,"plaintext_program_backups":programs,"audio_driver":audio,"segmentation_fault":log.contains("SIGSEGV"),"window_captured":output.join("screen.png").is_file()})).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
    if code != 0 {
        return Err(format!(
            "Jurassic exited with {code}; logs: {}",
            output.display()
        ));
    }
    Ok(format!(
        "Jurassic session closed; logs: {}",
        output.display()
    ))
}
fn files(directory: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    for e in fs::read_dir(directory).map_err(|e| e.to_string())? {
        let e = e.map_err(|e| e.to_string())?;
        if e.file_type().map_err(|e| e.to_string())?.is_dir() {
            files(&e.path(), out)?;
        } else if e.path().extension().is_some_and(|x| x == "txt") {
            out.push(e.path());
        }
    }
    Ok(())
}
fn validate_programs(game: &Path) -> Result<usize, String> {
    let base = game.join("g5/dino/data/programs_enc");
    let mut programs = Vec::new();
    files(&base, &mut programs)?;
    if programs.is_empty() {
        return Err("No Jurassic program files found".into());
    }
    for p in &programs {
        let relative = p.strip_prefix(&base).map_err(|e| e.to_string())?;
        let backup = game.join("programs_dec").join(relative);
        if !fs::read(&backup).is_ok_and(|b| b.starts_with(b"//\n")) {
            return Err(format!(
                "Missing/unsupported plaintext backup: {}",
                relative.display()
            ));
        }
    }
    Ok(programs.len())
}
fn save_mounts(s: &mut Sandbox, game: &Path, output: &Path) -> Result<(), String> {
    let saves = output.join("pmuser");
    if !saves.exists() {
        process::copy_tree(&game.join("pmuser"), &saves)?;
    }
    s.bind(&saves, "/pm/pmuser", false)
        .bind(&saves, "/pmuser", false);
    let nested = game.join("g5/dino/pmuser");
    if !nested.is_symlink() {
        if !nested.exists() {
            s.command.args(["--tmpfs", "/pm/g5/dino"]);
            for e in fs::read_dir(nested.parent().unwrap()).map_err(|e| e.to_string())? {
                let e = e.map_err(|e| e.to_string())?;
                s.bind(e.path(), Path::new("/pm/g5/dino").join(e.file_name()), true);
            }
        }
        s.bind(&saves, "/pm/g5/dino/pmuser", false);
    }
    Ok(())
}
fn shaders(game: &Path, output: &Path) -> Result<Vec<(PathBuf, PathBuf)>, String> {
    let prefix = "out vec4 out_Color;\r\n#ifndef ARCADE_BLOOM_OUTPUT_DECLARED\r\n#define ARCADE_BLOOM_OUTPUT_DECLARED\r\nout vec4 out_Bloom;\r\n#endif\r\n";
    let mut staged = Vec::new();
    for name in [
        "shader108",
        "shader108b",
        "shader108c_sk",
        "shader108d",
        "shader108d_sk",
    ] {
        let relative = Path::new(name).join("frag.gls");
        let source = fs::read_to_string(game.join("g5/shaders").join(&relative))
            .map_err(|e| e.to_string())?;
        if !source.contains("gl_FragData[0]") || !source.contains("gl_FragData[1]") {
            return Err(format!(
                "Unsupported shader variant: {}",
                relative.display()
            ));
        }
        staged.push((
            relative,
            format!(
                "{prefix}{}",
                source
                    .replace("gl_FragData[0]", "out_Color")
                    .replace("gl_FragData[1]", "out_Bloom")
            ),
        ));
    }
    let relative = PathBuf::from("include/frag_bloom.gls");
    let source =
        fs::read_to_string(game.join("g5/shaders").join(&relative)).map_err(|e| e.to_string())?;
    if source.matches("out vec4 out_Bloom;").count() != 1 {
        return Err("Unsupported shared bloom shader".into());
    }
    staged.push((relative,source.replace("out vec4 out_Bloom;","#ifndef ARCADE_BLOOM_OUTPUT_DECLARED\r\n#define ARCADE_BLOOM_OUTPUT_DECLARED\r\nout vec4 out_Bloom;\r\n#endif")));
    staged
        .into_iter()
        .map(|(relative, bytes)| {
            let dst = output.join("shader-overrides").join(&relative);
            fs::create_dir_all(dst.parent().unwrap()).map_err(|e| e.to_string())?;
            fs::write(&dst, bytes).map_err(|e| e.to_string())?;
            Ok((dst, Path::new("/pm/g5/shaders").join(relative)))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn materialized_and_missing_save_mountpoints_keep_persistent_copies() {
        let temp = tempfile::tempdir().unwrap();
        let game = temp.path().join("game");
        let output = temp.path().join("runtime");
        fs::create_dir_all(game.join("pmuser")).unwrap();
        fs::create_dir_all(game.join("g5/dino/data")).unwrap();
        fs::create_dir(&output).unwrap();
        fs::write(game.join("pmuser/settings"), b"factory defaults").unwrap();
        fs::write(game.join("g5/dino/data/asset"), b"original asset").unwrap();
        let mut s = Sandbox::new();
        save_mounts(&mut s, &game, &output).unwrap();
        let args: Vec<_> = s
            .command
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert!(args.windows(2).any(|a| a == ["--tmpfs", "/pm/g5/dino"]));
        assert!(args.windows(3).any(|a| a[0] == "--bind"
            && Path::new(&a[1]) == output.join("pmuser")
            && a[2] == "/pm/g5/dino/pmuser"));
        fs::write(output.join("pmuser/settings"), b"calibrated").unwrap();
        fs::create_dir_all(game.join("g5/dino/pmuser")).unwrap();
        save_mounts(&mut Sandbox::new(), &game, &output).unwrap();
        assert_eq!(
            fs::read(output.join("pmuser/settings")).unwrap(),
            b"calibrated"
        );
        assert_eq!(
            fs::read(game.join("pmuser/settings")).unwrap(),
            b"factory defaults"
        );
    }
}
