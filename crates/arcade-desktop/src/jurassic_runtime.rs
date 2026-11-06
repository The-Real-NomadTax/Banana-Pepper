//! Reproducible native startup diagnosis. No existing arcade loaders are invoked.
use arcade_core::jurassic::Build;
use std::path::Path;

#[cfg(target_os = "linux")]
pub fn launch(root: &Path) -> Result<String, String> {
    let materialized = crate::content::resolve(root)?;
    let root = materialized.as_path();
    let project = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let output = project.join("runtime/jurassic");
    std::fs::create_dir_all(&output).map_err(|e| e.to_string())?;
    arcade_core::jurassic::prepare_desktop(
        &root.join("linux based/Jurassic Park 2015/game_legacy_patched"),
        &output.join("game-desktop"),
    )?;
    let controls = arcade_core::controls::Bindings::load(&project.join("controls.cfg"))?;
    let settings = crate::runtime::game_settings(&project, "jurassic")?;
    let mut command = std::process::Command::new("python3");
    command
        .arg(project.join("scripts/jurassic-explore.py"))
        .args(["--launch", "--desktop", "--games-root"])
        .arg(root)
        .env("ARCADE_JURASSIC_KEYS", controls.env_value("jurassic")?);
    crate::runtime::settings_env(&mut command, &settings);
    let status = command
        .status()
        .map_err(|e| format!("Cannot launch Jurassic desktop runtime: {e}"))?;
    if !status.success() {
        return Err(format!("Jurassic desktop runtime exited with {status}"));
    }
    Ok("Jurassic Park desktop session closed.".into())
}

#[cfg(not(target_os = "linux"))]
pub fn launch(_root: &Path) -> Result<String, String> {
    Err("Jurassic Park native desktop launch currently requires Linux".into())
}

fn conclusion(trace: &str, log: &str) -> &'static str {
    if trace.contains("SIGSEGV") {
        "Jurassic Park crashed during initialization; inspect the startup trace."
    } else if log.contains("Game dongle initialization failed!") {
        "Jurassic Park reached game initialization, then stopped at the game dongle check (src/game_sys.c:760). No game window has been demonstrated."
    } else if trace.contains("htons(1947)") && trace.contains("ECONNREFUSED") {
        "Jurassic Park stopped while connecting to the Sentinel/HASP licence service at localhost:1947. No game window has been demonstrated."
    } else {
        "Native diagnostic completed. This does not establish window creation or gameplay; inspect the trace and runner log."
    }
}

#[cfg(target_os = "linux")]
pub fn probe(root: &Path, build: Build) -> Result<String, String> {
    let materialized = crate::content::resolve(root)?;
    let root = materialized.as_path();
    use std::{
        fs,
        process::{Command, Stdio},
    };
    let game = root
        .join("linux based/Jurassic Park 2015")
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let output = Path::new("runtime/jurassic");
    fs::create_dir_all(output).map_err(|e| e.to_string())?;
    let output = output.canonicalize().map_err(|e| e.to_string())?;
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(output.join("session.lock"))
        .map_err(|e| e.to_string())?;
    lock.try_lock()
        .map_err(|e| format!("Another Jurassic Park diagnostic is running: {e}"))?;
    for name in [
        "libtheora.so.0",
        "libPhysXLoader.so.1",
        "libPhysXCore.so.1",
        "libNxCooking.so.1",
    ] {
        let library = output.join("libs").join(name);
        let info = arcade_core::inspect_file(&library).map_err(|e| {
            format!(
                "Missing/invalid legacy library {}: {e}. Run scripts/setup-jurassic-libs.sh.",
                library.display()
            )
        })?;
        if info.format != "ELF" || info.bits != 32 || info.machine != 3 {
            return Err(format!("{} must be ELF32/i386", library.display()));
        }
    }
    let kind = match build {
        Build::Original => "original",
        Build::Legacy => "legacy",
        Build::Native => "native",
    };
    let snapshot = output.join(format!("game-{kind}"));
    arcade_core::jurassic::prepare(&game.join(build.filename()), &snapshot, build)?;
    let logpath = output.join(format!("{kind}-runner.log"));
    let tracepath = output.join(format!("{kind}-startup.strace"));
    let log = fs::File::create(&logpath).map_err(|e| e.to_string())?;
    let mut command = Command::new("strace");
    command
        .args(["-f", "-s", "200", "-o"])
        .arg(&tracepath)
        .args([
            "-e",
            "trace=execve,open,openat,access,write,connect,socket,exit_group",
            "timeout",
            "-k",
            "1s",
            "10s",
            "bwrap",
            "--unshare-all",
            "--die-with-parent",
            "--clearenv",
            "--ro-bind",
            "/usr",
            "/usr",
        ]);
    for name in ["lib", "lib32", "lib64", "bin"] {
        let path = format!("/{name}");
        if let Ok(link) = fs::read_link(&path) {
            command.arg("--symlink").arg(link).arg(&path);
        } else if Path::new(&path).exists() {
            command.args(["--ro-bind", &path, &path]);
        }
    }
    if matches!(build, Build::Native) {
        let shim = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/i686-unknown-linux-gnu/debug/libarcade_linux_shim.so")
            .canonicalize()
            .map_err(|e| e.to_string())?;
        command.arg("--ro-bind").arg(shim).args([
            "/shim.so",
            "--setenv",
            "LD_BIND_NOW",
            "1",
            "--setenv",
            "LD_PRELOAD",
            "/shim.so",
            "--setenv",
            "ARCADE_JURASSIC",
            "1",
        ]);
    }
    command
        .args(["--dev", "/dev", "--proc", "/proc", "--tmpfs", "/tmp"])
        .arg("--ro-bind")
        .arg(&game)
        .arg("/pm")
        .arg("--ro-bind")
        .arg(&snapshot)
        .arg("/pm/game")
        .arg("--ro-bind")
        .arg(output.join("libs"))
        .arg("/runtime-libs")
        .args([
            "--setenv",
            "LD_LIBRARY_PATH",
            "/runtime-libs",
            "--chdir",
            "/pm",
            "/pm/game",
        ])
        .stdin(Stdio::null())
        .stdout(log.try_clone().map_err(|e| e.to_string())?)
        .stderr(log);
    let status = command
        .status()
        .map_err(|e| format!("Cannot run native diagnostic: {e}"))?;
    let trace = fs::read_to_string(&tracepath).unwrap_or_default();
    let runner_log = fs::read_to_string(&logpath).unwrap_or_default();
    let licence = trace.contains("htons(1947)") && trace.contains("ECONNREFUSED");
    let crashed = trace.contains("SIGSEGV");
    let conclusion = conclusion(&trace, &runner_log);
    let result = serde_json::json!({"mode":kind,"build":build.filename(),"source_sha256":build.sha256(),"exit_code":status.code(),
        "licence_service_refused":licence,"segmentation_fault":crashed,"conclusion":conclusion,
        "game_dongle_initialization_failed":runner_log.contains("Game dongle initialization failed!"),
        "display_attached":false,"trace":tracepath,"runner_log":logpath});
    fs::write(
        output.join(format!("{kind}-result.json")),
        serde_json::to_vec_pretty(&result).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    Err(format!("{conclusion} Reports: {}", output.display()))
}

#[cfg(test)]
mod tests {
    use super::conclusion;

    #[test]
    fn game_assertion_is_more_specific_than_service_failure() {
        let trace = "connect htons(1947) ECONNREFUSED; exit_group(0)";
        assert!(
            conclusion(trace, "Game dongle initialization failed!").contains("src/game_sys.c:760")
        );
        assert!(conclusion(trace, "").contains("licence service"));
    }

    #[test]
    fn later_crash_is_not_hidden_by_earlier_assertion() {
        assert!(conclusion("SIGSEGV", "Game dongle initialization failed!").contains("crashed"));
    }
}

#[cfg(not(target_os = "linux"))]
pub fn probe(_root: &Path, _build: Build) -> Result<String, String> {
    Err("Jurassic Park native diagnosis currently requires Linux".into())
}
