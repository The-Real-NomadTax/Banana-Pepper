//! Linux-specific process orchestration. No execution APIs in arcade-core.
use std::path::Path;

#[cfg(target_os = "linux")]
fn run_big_buck(games_root: &Path, output: &Path, desktop: bool) -> Result<String, String> {
    use std::{
        fs,
        process::{Command, Stdio},
    };
    let game = games_root
        .join("linux based/Big Bug Hunter Pro")
        .canonicalize()
        .map_err(|e| e.to_string())?;
    fs::create_dir_all(output).map_err(|e| e.to_string())?;
    let output = output.canonicalize().map_err(|e| e.to_string())?;
    let _session_lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(output.join("session.lock"))
        .map_err(|e| e.to_string())?;
    _session_lock.try_lock().map_err(|e| {
        format!(
            "Another Big Buck Hunter session is already using {}: {e}",
            output.display()
        )
    })?;
    let project = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let shim = project.join("target/i686-unknown-linux-gnu/debug/libarcade_linux_shim.so");
    if !shim.is_file() {
        return Err("Build the guest shim first: cargo build -p arcade-linux-shim --target i686-unknown-linux-gnu".into());
    }
    let xvfb = if Path::new("/usr/bin/Xvfb").is_file() {
        "/usr/bin/Xvfb".into()
    } else {
        project.join("runtime/tools/xvfb/usr/bin/Xvfb")
    };
    if !desktop && !xvfb.is_file() {
        return Err("Xvfb required. Install your distribution's Xvfb package or extract it under runtime/tools/xvfb.".into());
    }
    let source_hash = arcade_core::big_buck::SOURCE_SHA256;
    let prepared_hash = arcade_core::big_buck::prepare(&game.join("game"), &output.join("game"))?;
    arcade_core::big_buck::seed_saves(&game.join("bbh3user"), &output.join("bbh3user"))
        .map_err(|e| e.to_string())?;
    fs::copy(shim, output.join("libarcade_linux_shim.so")).map_err(|e| e.to_string())?;
    fs::write(
        output.join("probe.sh"),
        include_str!("../../../scripts/big-buck-probe.sh"),
    )
    .map_err(|e| e.to_string())?;
    for file in [
        "screen.png",
        "windows.txt",
        "game-exit-code.txt",
        "tracking.png",
        "after-start.png",
        "after-start2.png",
        "game-select.png",
        "after-shot.png",
        "mode-select.png",
        "adventure-select.png",
        "adventure-intro.png",
        "records.png",
        "instructions.png",
        "later-screen.png",
        "site-select.png",
        "hunt.png",
    ] {
        match fs::remove_file(output.join(file)) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    let log = fs::File::create(output.join("runner.log")).map_err(|e| e.to_string())?;
    let mut command = if desktop {
        Command::new("bwrap")
    } else {
        let mut command = Command::new("timeout");
        command.args(["-k", "2s", "53s", "bwrap"]);
        command
    };
    command.args([
        "--unshare-all",
        "--die-with-parent",
        "--clearenv",
        "--setenv",
        "PATH",
        "/usr/bin",
        "--ro-bind",
        "/usr",
        "/usr",
    ]);
    // Preserve the host's actual library layout (merged /usr and Debian layouts).
    for name in ["lib", "lib32", "lib64", "bin"] {
        let path = format!("/{name}");
        if let Ok(link) = fs::read_link(&path) {
            command.arg("--symlink").arg(link).arg(&path);
        } else if Path::new(&path).exists() {
            command.args(["--ro-bind", &path, &path]);
        }
    }
    command.args([
        "--dev",
        "/dev",
        "--proc",
        "/proc",
        "--tmpfs",
        "/tmp",
        "--ro-bind-try",
        "/etc/fonts",
        "/etc/fonts",
        "--ro-bind-try",
        "/etc/ld.so.cache",
        "/etc/ld.so.cache",
    ]);
    // Expose only the desktop playback socket; no microphone device or home tree.
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::MetadataExt;
        let runtime = std::env::var_os("XDG_RUNTIME_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::PathBuf::from(format!(
                    "/run/user/{}",
                    fs::metadata("/proc/self").map(|m| m.uid()).unwrap_or(1000)
                ))
            });
        let server = std::env::var("PULSE_SERVER").ok();
        let socket = server
            .as_deref()
            .and_then(|s| {
                s.strip_prefix("unix:")
                    .or_else(|| s.starts_with('/').then_some(s))
            })
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| runtime.join("pulse/native"));
        if socket.exists() {
            command.arg("--ro-bind").arg(socket).args([
                "/tmp/arcade-pulse/native",
                "--setenv",
                "PULSE_SERVER",
                "unix:/tmp/arcade-pulse/native",
                "--setenv",
                "ARCADE_BBH_AUDIO",
                "1",
            ]);
            if let Some(sink) = std::env::var_os("ARCADE_AUDIO_SINK") {
                command.args(["--setenv", "PULSE_SINK"]).arg(sink);
            }
            let cookie = std::env::var_os("PULSE_COOKIE")
                .map(std::path::PathBuf::from)
                .or_else(|| {
                    std::env::var_os("HOME")
                        .map(|h| std::path::PathBuf::from(h).join(".config/pulse/cookie"))
                });
            if let Some(cookie) = cookie.filter(|p| p.is_file()) {
                command.arg("--ro-bind").arg(cookie).args([
                    "/tmp/arcade-pulse/cookie",
                    "--setenv",
                    "PULSE_COOKIE",
                    "/tmp/arcade-pulse/cookie",
                ]);
            }
        }
    }
    command
        .arg("--ro-bind")
        .arg(&game)
        .arg("/game")
        .arg("--bind")
        .arg(&output)
        .arg("/work")
        .arg("--ro-bind")
        .arg(output.join("game"))
        .arg("/game/game")
        .arg("--ro-bind")
        .arg(game.join("bbh3data"))
        .arg("/bbh3data")
        .arg("--bind")
        .arg(output.join("bbh3user"))
        .arg("/bbh3user")
        .args(["--chdir", "/game"]);
    if desktop {
        let display = std::env::var("DISPLAY")
            .map_err(|_| "No X11 DISPLAY found. An X11 or XWayland desktop is required.")?;
        let number = display
            .strip_prefix(':')
            .and_then(|s| s.split('.').next())
            .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
            .ok_or("Desktop mode currently requires a local DISPLAY such as :0 or :1")?;
        let socket = format!("/tmp/.X11-unix/X{number}");
        if !Path::new(&socket).exists() {
            return Err(format!("X11 socket missing: {socket}"));
        }
        command.args([
            "--ro-bind",
            &socket,
            &socket,
            "--setenv",
            "DISPLAY",
            &display,
        ]);
        let auth = std::env::var_os("XAUTHORITY")
            .map(std::path::PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME").map(|p| std::path::PathBuf::from(p).join(".Xauthority"))
            });
        if let Some(auth) = auth.filter(|p| p.is_file()) {
            command.arg("--ro-bind").arg(auth).args([
                "/tmp/arcade.Xauthority",
                "--setenv",
                "XAUTHORITY",
                "/tmp/arcade.Xauthority",
            ]);
        }
        command.args([
            "--setenv",
            "HOME",
            "/tmp",
            "--setenv",
            "LIBGL_ALWAYS_SOFTWARE",
            "1",
            "--setenv",
            "__GLX_VENDOR_LIBRARY_NAME",
            "mesa",
            "--setenv",
            "ARCADE_BBH_ADAPTER",
            "1",
            "--setenv",
            "LD_PRELOAD",
            "/work/libarcade_linux_shim.so",
            "/game/game",
        ]);
    } else {
        command
            .arg("--ro-bind")
            .arg(xvfb)
            .arg("/work/Xvfb")
            .args(["/bin/bash", "/work/probe.sh"]);
    }
    command
        .stdin(Stdio::null())
        .stdout(log.try_clone().map_err(|e| e.to_string())?)
        .stderr(log);
    let status = command
        .status()
        .map_err(|e| format!("Cannot start probe (requires timeout and bwrap): {e}"))?;
    if desktop {
        return if status.success() {
            Ok("Big Buck Hunter closed. Desktop logs: runtime/big-buck-desktop/runner.log".into())
        } else {
            Err(format!(
                "Big Buck Hunter exited with {status}; see {}",
                output.join("runner.log").display()
            ))
        };
    }
    if !status.success() {
        return Err(format!(
            "Private startup failed ({status}); see {}",
            output.join("runner.log").display()
        ));
    }
    let windows = fs::read_to_string(output.join("windows.txt")).unwrap_or_default();
    let opened = !windows.trim().is_empty() && output.join("screen.png").is_file();
    let message = if opened {
        "Game window opened on the private display. Inspect screen.png; cabinet I/O and gameplay are not validated."
    } else {
        "Game did not create the expected window. Inspect stderr.log and xvfb.log."
    };
    let manifest = serde_json::json!({
        "source_sha256": source_hash, "prepared_sha256": prepared_hash,
        "window_created": opened, "message": message,
        "game_exit_code": fs::read_to_string(output.join("game-exit-code.txt")).unwrap_or_default().trim(),
        "display": "private Xvfb; software Mesa; no host display or network",
    });
    fs::write(
        output.join("result.json"),
        serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    if opened {
        Ok(format!("{message} Reports: {}", output.display()))
    } else {
        Err(format!("{message} Reports: {}", output.display()))
    }
}

pub fn probe_big_buck(games_root: &Path, output: &Path) -> Result<String, String> {
    #[cfg(target_os = "linux")]
    {
        run_big_buck(games_root, output, false)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (games_root, output);
        Err("Big Buck Hunter requires Linux".into())
    }
}
pub fn launch_big_buck(games_root: &Path) -> Result<String, String> {
    #[cfg(target_os = "linux")]
    {
        run_big_buck(games_root, Path::new("runtime/big-buck-desktop"), true)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = games_root;
        Err("Big Buck Hunter requires Linux".into())
    }
}
