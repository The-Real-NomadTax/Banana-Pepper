//! Typed, shell-free guest supervision. Games declare timed probe events;
//! the shared supervisor owns subprocesses, display lifetime and captures.
use crate::{
    Sandbox,
    process::{ChildGuard, logged},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Program {
    pub executable: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
}
impl Program {
    pub fn new(executable: &str, args: &[&str]) -> Self {
        Self {
            executable: executable.into(),
            args: args.iter().map(|a| (*a).into()).collect(),
            env: BTreeMap::new(),
        }
    }
    pub fn command(&self) -> Command {
        let mut c = Command::new(&self.executable);
        c.args(&self.args).envs(&self.env);
        c
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Action {
    Key(String),
    Click(i32, i32, u8),
    Capture(String),
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Event {
    pub at_ms: u64,
    pub action: Action,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Plan {
    pub game: Program,
    pub timeout_ms: Option<u64>,
    pub window: String,
    pub log: String,
    pub events: Vec<Event>,
    pub capture_every_ms: Option<u64>,
    pub capture_until_ms: u64,
    pub private_display: Option<(String, u32, u32)>,
}
impl Plan {
    pub fn new(game: Program, window: &str) -> Self {
        Self {
            game,
            timeout_ms: None,
            window: window.into(),
            log: "game.log".into(),
            events: Vec::new(),
            capture_every_ms: Some(10000),
            capture_until_ms: 60000,
            private_display: None,
        }
    }
}
pub fn helper(project: &Path) -> PathBuf {
    let p = crate::resource_path(
        project,
        "helpers/linux/banana-session",
        "target/release/banana-session",
    );
    if p.is_file() || crate::is_distribution(project) {
        p
    } else {
        project.join("target/debug/banana-session")
    }
}
pub fn launch(
    sandbox: &mut Sandbox,
    project: &Path,
    output: &Path,
    plan: &Plan,
) -> Result<i32, String> {
    let mut captures = std::collections::BTreeSet::from([
        "screen.png".to_owned(),
        "windows.txt".to_owned(),
        "window-geometry.txt".to_owned(),
        "game-exit-code.txt".to_owned(),
    ]);
    for event in &plan.events {
        if let Action::Capture(name) = &event.action {
            if Path::new(name).components().count() != 1
                || !matches!(
                    Path::new(name).components().next(),
                    Some(std::path::Component::Normal(_))
                )
            {
                return Err("Capture names must be filenames within the session directory".into());
            }
            captures.insert(name.clone());
        }
    }
    if let Some(step) = plan.capture_every_ms {
        if step == 0 {
            return Err("Capture interval must be positive".into());
        }
        for time in (step..=plan.capture_until_ms).step_by(step as usize) {
            captures.insert(format!("screen-{}.png", time / 1000));
        }
    }
    for name in captures {
        match fs::remove_file(output.join(name)) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    let helper = helper(project);
    if !helper.is_file() {
        return Err("The compiled Linux session supervisor is missing; run just build".into());
    }
    fs::write(
        output.join("session.json"),
        serde_json::to_vec(plan).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    sandbox.bind(helper, "/banana-session", true);
    sandbox
        .command
        .args(["/banana-session", "/work/session.json"]);
    let status = logged(
        &mut sandbox.command,
        &output.join("runner.log"),
        plan.timeout_ms.map(|n| Duration::from_millis(n + 15000)),
    )?;
    Ok(status.map_or(124, |s| s.code().unwrap_or(1)))
}
fn window(pattern: &str) -> Option<String> {
    let o = crate::process::bounded_output(
        Command::new("xdotool").args(["search", "--onlyvisible", "--name", pattern]),
        Duration::from_secs(1),
        65536,
    )
    .ok()??;
    String::from_utf8_lossy(&o.stdout)
        .lines()
        .next()
        .map(str::to_owned)
}
fn capture(pattern: &str, name: &str) {
    let Some(target) = window(pattern) else {
        return;
    };
    let path = Path::new("/work").join(name);
    let _ = crate::process::bounded_output(
        Command::new("import").args(["-window", &target]).arg(&path),
        Duration::from_secs(2),
        65536,
    );
    if path.is_file() {
        let _ = fs::copy(&path, "/work/screen.png");
    }
    if target != "root" {
        for (args, file) in [
            (vec!["getwindowname", &target], "windows.txt"),
            (
                vec!["getwindowgeometry", "--shell", &target],
                "window-geometry.txt",
            ),
        ] {
            if let Ok(Some(o)) = crate::process::bounded_output(
                Command::new("xdotool").args(args),
                Duration::from_secs(1),
                65536,
            ) {
                let _ = fs::write(Path::new("/work").join(file), o.stdout);
            }
        }
    }
}
pub fn supervise(mut plan: Plan) -> Result<i32, String> {
    let _display = if let Some((program, width, height)) = &plan.private_display {
        fs::create_dir_all("/tmp/.X11-unix").map_err(|e| e.to_string())?;
        let mut c = Command::new(program);
        c.args([
            ":0",
            "-screen",
            "0",
            &format!("{width}x{height}x24"),
            "-nolisten",
            "tcp",
            "-ac",
            "+iglx",
        ]);
        let log = fs::File::create("/work/xvfb.log").map_err(|e| e.to_string())?;
        c.stdout(log.try_clone().map_err(|e| e.to_string())?)
            .stderr(log);
        let mut child = ChildGuard::spawn(&mut c)?;
        let started = Instant::now();
        while !Path::new("/tmp/.X11-unix/X0").exists() {
            if started.elapsed() > Duration::from_secs(5)
                || child.0.try_wait().map_err(|e| e.to_string())?.is_some()
            {
                return Err("Private X server failed; inspect xvfb.log".into());
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        Some(child)
    } else {
        None
    };
    let mut command = plan.game.command();
    let log = fs::File::create(Path::new("/work").join(&plan.log)).map_err(|e| e.to_string())?;
    command
        .stdout(log.try_clone().map_err(|e| e.to_string())?)
        .stderr(log);
    let mut child = ChildGuard::spawn(&mut command)?;
    plan.events.sort_by_key(|e| e.at_ms);
    let started = Instant::now();
    let mut index = 0;
    let mut next_capture = plan.capture_every_ms;
    let code = loop {
        if let Some(s) = child.0.try_wait().map_err(|e| e.to_string())? {
            break s.code().unwrap_or(1);
        }
        let elapsed = started.elapsed().as_millis() as u64;
        if plan.timeout_ms.is_some_and(|n| elapsed >= n) {
            child.stop();
            break 124;
        }
        while let Some(event) = plan.events.get(index).filter(|e| elapsed >= e.at_ms) {
            match &event.action {
                Action::Capture(name) => capture(&plan.window, name),
                Action::Key(key) => {
                    if let Some(w) = window(&plan.window) {
                        let _ = crate::process::bounded_output(
                            Command::new("xdotool").args(["key", "--window", &w, key]),
                            Duration::from_secs(1),
                            65536,
                        );
                    }
                }
                Action::Click(x, y, button) => {
                    if let Some(w) = window(&plan.window) {
                        let _ = crate::process::bounded_output(
                            Command::new("xdotool").args([
                                "mousemove",
                                "--window",
                                &w,
                                &x.to_string(),
                                &y.to_string(),
                                "click",
                                "--window",
                                &w,
                                &button.to_string(),
                            ]),
                            Duration::from_secs(1),
                            65536,
                        );
                    }
                }
            }
            index += 1;
        }
        if let Some(n) = next_capture.filter(|n| elapsed >= *n && *n <= plan.capture_until_ms) {
            capture(&plan.window, &format!("screen-{}.png", n / 1000));
            next_capture = plan
                .capture_every_ms
                .map(|step| n.saturating_add(step.max(1)));
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    fs::write("/work/game-exit-code.txt", format!("{code}\n")).map_err(|e| e.to_string())?;
    Ok(code)
}
