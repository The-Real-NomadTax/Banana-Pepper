//! Linux execution, dependency probes and cabinet input for Minecraft.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]
use super::image;
use banana_runtime::{Sandbox, process, session};
use std::{fs, io::Read, path::Path, time::Duration};
#[path = "dependencies.rs"]
mod dependencies;
#[cfg(target_os = "linux")]
#[path = "input.rs"]
mod input;

pub(super) fn run(project: &Path, game: &Path, mode: &str) -> Result<String, String> {
    let dependencies = project.join("runtime/minecraft");
    fs::create_dir_all(&dependencies).map_err(|e| e.to_string())?;
    let output = if mode == "launch" {
        project.join("runtime/minecraft-seeded")
    } else {
        dependencies.clone()
    };
    let _lock = process::lock(&output)?;
    let output = output.canonicalize().map_err(|e| e.to_string())?;
    if output.starts_with(game) || game.starts_with(&output) {
        return Err("Runtime and original game directories must be separate".into());
    }
    if mode == "setup" {
        dependencies::setup(&dependencies)?;
        return Ok("Minecraft runtime libraries prepared locally".into());
    }
    let source = fs::read(game.join("gamedump.exec")).map_err(|e| e.to_string())?;
    let prepared = image::prepare(&source)?;
    fs::create_dir_all(output.join("home")).map_err(|e| e.to_string())?;
    fs::create_dir_all(output.join("lib")).map_err(|e| e.to_string())?;
    if !output.join("pmuser").exists() {
        if game.join("pmuser").is_dir() {
            process::copy_tree(&game.join("pmuser"), &output.join("pmuser"))?;
        } else {
            fs::create_dir(output.join("pmuser")).map_err(|e| e.to_string())?;
        }
    }
    if mode == "launch" {
        launch(project, game, &output, &dependencies, &prepared)
    } else {
        probe(game, &output, &prepared)
    }
}
fn sandbox(game: &Path, output: &Path, executable: &Path) -> Sandbox {
    let mut s = Sandbox::new();
    s.bind(output, "/work", false);
    s.command.args(["--dir", "/pm/g7/minecraft"]);
    for name in ["data", "eFileMaps", "utils", "minecraft.hvc"] {
        let p = game.join(name);
        if p.exists() {
            s.bind(p, Path::new("/pm/g7/minecraft").join(name), true);
        }
    }
    s.command
        .args(["--symlink", "/work/pmuser", "/pm/g7/minecraft/pmuser"]);
    s.bind(executable,"/pm/g7/minecraft/game",true).chdir("/pm/g7/minecraft").env("HOME","/work/home")
        .env("LD_LIBRARY_PATH","/work/lib:/work/libraries/usr/lib/x86_64-linux-gnu:/work/libraries/usr/lib/x86_64-linux-gnu/lapack:/work/libraries/usr/lib/x86_64-linux-gnu/blas:/work/libraries/lib/x86_64-linux-gnu");
    s
}
fn capture(
    s: &mut Sandbox,
    args: &[&str],
    path: &Path,
    seconds: u64,
) -> Result<(i32, String), String> {
    s.command.args(args);
    let code = process::logged(&mut s.command, path, Some(Duration::from_secs(seconds)))?
        .map_or(124, |s| s.code().unwrap_or(1));
    Ok((code, fs::read_to_string(path).unwrap_or_default()))
}
fn probe(game: &Path, output: &Path, prepared: &[u8]) -> Result<String, String> {
    let executable = output.join("game");
    fs::write(&executable, prepared).map_err(|e| e.to_string())?;
    process::executable(&executable)?;
    let mut s = sandbox(game, output, &executable);
    let (code, mut deps) = capture(
        &mut s,
        &[
            "/lib64/ld-linux-x86-64.so.2",
            "--list",
            "/pm/g7/minecraft/game",
        ],
        &output.join("dependencies.log"),
        30,
    )?;
    let mut report = serde_json::json!({"sha256":crate::prepare::DUMP_SHA256,"mode":"probe","dependency_exit_code":code,"runtime_sha256":banana_runtime::dependencies::sha256(prepared),"reconstructed_imports":659,"gameplay_verified":false,"dependencies":deps});
    if code != 0 {
        let (_, inventory) = capture(
            &mut sandbox(game, output, &executable),
            &["/usr/bin/ldd", "/pm/g7/minecraft/game"],
            &output.join("dependency-inventory.log"),
            30,
        )?;
        deps = inventory + &deps;
        let missing: std::collections::BTreeSet<_> = deps
            .lines()
            .filter(|l| l.contains("=> not found"))
            .filter_map(|l| l.split_whitespace().next())
            .collect();
        report["dependencies"] = deps.clone().into();
        report["missing_libraries"] = serde_json::json!(missing);
        write_report(output, &report)?;
        return Err(format!(
            "Minecraft dependencies are missing or incompatible:\n{deps}"
        ));
    }
    let mut protected = Vec::new();
    for name in ["programs.zip", "scenes.zip"] {
        let p = game.join("data/prod/archives").join(name);
        if p.is_file() {
            let mut magic = [0u8; 4];
            let n = fs::File::open(&p)
                .and_then(|mut f| f.read(&mut magic))
                .map_err(|e| e.to_string())?;
            if n != 4 || ![b"PK\x03\x04", b"PK\x05\x06", b"PK\x07\x08"].contains(&&magic) {
                protected.push(name);
            }
        }
    }
    report["protected_archives"] = serde_json::json!(protected);
    if !protected.is_empty() {
        let licensing = license(game, output, &executable)?;
        let ok = licensing["hasp_status"] == 0;
        report["licensing"] = licensing;
        if !ok {
            write_report(output, &report)?;
            return Err(format!(
                "Minecraft licensing initialization failed; inspect {}",
                output.join("license-probe.log").display()
            ));
        }
    }
    let mut s = sandbox(game, output, &executable);
    s.desktop()?.gpu();
    s.command
        .args(["/pm/g7/minecraft/game", "-f1280x720", "-n"]);
    let status = process::logged(
        &mut s.command,
        &output.join("runner.log"),
        Some(Duration::from_secs(25)),
    )?;
    report["probe_timeout"] = status.is_none().into();
    report["exit_code"] = status.and_then(|s| s.code()).into();
    write_report(output, &report)?;
    if status.is_some_and(|s| !s.success()) {
        return Err("Minecraft startup probe failed; inspect runtime/minecraft/runner.log".into());
    }
    Ok("Minecraft startup probe completed; process survival does not certify gameplay".into())
}
fn write_report(output: &Path, value: &serde_json::Value) -> Result<(), String> {
    fs::write(
        output.join("report.json"),
        serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}
fn license(game: &Path, output: &Path, executable: &Path) -> Result<serde_json::Value, String> {
    let commands = "set pagination off\nset confirm off\nset print thread-events off\nset disable-randomization off\nset startup-with-shell off\nbreak *0x559470\nrun\nif $pc == 0x559470\nprintf \"ARCADE_HASP_STATUS=%u\\n\", *(unsigned int*)0x17bad58\nprintf \"ARCADE_WRAPPER_STATUS=%u\\n\", $esi\nend\n";
    fs::write(output.join("license-probe.gdb"), commands).map_err(|e| e.to_string())?;
    let mut s = sandbox(game, output, executable);
    s.desktop()?.gpu();
    let (code, trace) = capture(
        &mut s,
        &[
            "gdb",
            "--batch",
            "--nx",
            "--nh",
            "-x",
            "/work/license-probe.gdb",
            "--args",
            "/pm/g7/minecraft/game",
            "-f1280x720",
            "-n",
        ],
        &output.join("license-probe.log"),
        30,
    )?;
    let value = |key: &str| {
        trace
            .lines()
            .find_map(|l| l.strip_prefix(key))
            .and_then(|v| v.parse::<u32>().ok())
    };
    let status = value("ARCADE_HASP_STATUS=");
    Ok(
        serde_json::json!({"hasp_status":status,"wrapper_status":value("ARCADE_WRAPPER_STATUS="),"gdb_exit_code":code,"instructions_modified":false,"status":match status{Some(0)=>"no_api_error_at_diagnostic_point",Some(7)=>"key_not_found",Some(_)=>"api_error",None=>"diagnostic_failed"}}),
    )
}
fn launch(
    project: &Path,
    game: &Path,
    output: &Path,
    dependencies: &Path,
    prepared: &[u8],
) -> Result<String, String> {
    let card = std::env::var("ARCADE_MINECRAFT_CARD").unwrap_or_else(|_| "a0000000".into());
    super::validate_card(1, &card)?;
    let records = dependencies.join("seeds.json");
    let seeds: serde_json::Value = serde_json::from_slice(
        &fs::read(&records)
            .map_err(|e| format!("Captured seed records missing ({}): {e}", records.display()))?,
    )
    .map_err(|e| e.to_string())?;
    let image = image::seeded(prepared, &seeds)?;
    let path = output.join("game.seeded");
    fs::write(&path, image).map_err(|e| e.to_string())?;
    process::executable(&path)?;
    let adapter = banana_runtime::resource_path(
        project,
        "adapters/linux-x86_64/libarcade_minecraft_io.so",
        "target/debug/libarcade_minecraft_io.so",
    );
    fs::copy(adapter, output.join("cards.so")).map_err(|e| e.to_string())?;
    for name in ["card-events", "key-events"] {
        fs::write(output.join(name), []).map_err(|e| e.to_string())?;
    }
    let mut s = sandbox(game, output, &game.join("game"));
    s.bind(game.join("Patch"), "/pm/g7/minecraft/Patch", true)
        .bind(dependencies.join("lib"), "/work/lib", true);
    if dependencies.join("libraries").is_dir() {
        s.bind(dependencies.join("libraries"), "/work/libraries", true);
    }
    s.desktop()?.gpu();
    s.inert_tools(output, &["nvidia-settings", "xrandr", "reboot", "shutdown"])?;
    s.env("LD_PRELOAD", "/work/cards.so")
        .env("ARCADE_MINECRAFT_CARDS", "1")
        .env("ARCADE_MINECRAFT_KEYBOARD", "1")
        .env("ARCADE_MINECRAFT_CARD", card);
    #[cfg(target_os = "linux")]
    let _input = {
        let bridge = input::Bridge::new(
            output,
            std::env::var("ARCADE_MINECRAFT_HOST_KEYBOARD").as_deref() != Ok("0"),
        )?;
        s.command.args(["--dir", "/dev/input"]);
        for path in bridge.paths() {
            s.device(path);
        }
        bridge
    };
    s.command.args(["/work/game.seeded", "-f1920x1080", "-n"]);
    let mut command = gamescope(project, &s.command)?;
    let code = process::logged(&mut command, &output.join("runner.log"), None)?
        .ok_or("Unexpected launch timeout")?;
    if !code.success() {
        return Err(format!(
            "Minecraft exited with {code}; inspect {}",
            output.join("runner.log").display()
        ));
    }
    Ok("Minecraft session closed".into())
}
fn gamescope(
    project: &Path,
    command: &std::process::Command,
) -> Result<std::process::Command, String> {
    let found = std::env::var_os("PATH")
        .is_some_and(|p| std::env::split_paths(&p).any(|d| d.join("gamescope").is_file()));
    let mut args: Vec<_> = command.get_args().map(std::ffi::OsStr::to_owned).collect();
    if !found || std::env::var("ARCADE_MINECRAFT_NO_GAMESCOPE").as_deref() == Ok("1") {
        let mut c = std::process::Command::new(command.get_program());
        c.args(args);
        return Ok(c);
    }
    for i in 0..args.len().saturating_sub(2) {
        if args[i] == "--setenv" && args[i + 1] == "DISPLAY" {
            args[i + 2] = "@DISPLAY@".into();
        }
    }
    let mut c = std::process::Command::new("gamescope");
    c.args([
        "-W", "1920", "-H", "1080", "-w", "1920", "-h", "1080", "-r", "60",
    ]);
    if let Some(backend) = std::env::var_os("ARCADE_GAMESCOPE_BACKEND") {
        c.arg("--backend").arg(backend);
    }
    c.arg("--")
        .arg(session::helper(project))
        .arg("--display-exec")
        .arg(command.get_program())
        .args(args);
    Ok(c)
}
