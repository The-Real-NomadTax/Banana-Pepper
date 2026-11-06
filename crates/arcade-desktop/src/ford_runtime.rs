//! One Windows game and cabinet adapter, used directly or through Wine.
use std::{fs, path::Path, process::Command};
pub fn launch(root: &Path) -> Result<String, String> {
    let project = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let source = arcade_core::ford::directory(root)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let output = project.join("runtime/ford-racing");
    fs::create_dir_all(&output).map_err(|e| e.to_string())?;
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(output.join("session.lock"))
        .map_err(|e| e.to_string())?;
    lock.try_lock()
        .map_err(|e| format!("Another Ford session uses this save directory: {e}"))?;
    arcade_core::ford::prepare(
        &source,
        &output,
        &project.join("target/i686-pc-windows-msvc/release/inpout32.dll"),
    )?;
    let settings = crate::runtime::game_settings(&project, "ford")?;
    if ![
        (640, 480),
        (800, 600),
        (1024, 768),
        (1280, 1024),
        (1600, 1200),
    ]
    .contains(&settings.resolution)
    {
        return Err("Ford supports 640x480, 800x600, 1024x768, 1280x1024 or 1600x1200".into());
    }
    let ini = output.join("data/GameSettings.ini");
    let text = fs::read_to_string(&ini).map_err(|e| e.to_string())?;
    let original =
        fs::read_to_string(source.join("data/GameSettings.ini")).map_err(|e| e.to_string())?;
    let text = text
        .lines()
        .map(|line| {
            if line.starts_with("Resolution=") {
                format!("Resolution={}", settings.resolution_text())
            } else if line.starts_with("Fullscreen=") {
                format!(
                    "Fullscreen={}",
                    if settings.fullscreen { "on" } else { "off" }
                )
            } else if line
                .split_once('=')
                .is_some_and(|(key, _)| key.ends_with("Volume"))
            {
                let (key, _) = line.split_once('=').unwrap();
                if settings.audio {
                    original
                        .lines()
                        .find(|s| s.starts_with(&format!("{key}=")))
                        .unwrap_or(line)
                        .to_owned()
                } else {
                    format!("{key}=off")
                }
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\r\n");
    fs::write(ini, text).map_err(|e| e.to_string())?;
    #[cfg(windows)]
    let mut command = Command::new(output.join("FordRacing/fordracing.exe"));
    #[cfg(target_os = "linux")]
    let mut command = {
        let mut c = Command::new("wine");
        c.arg(output.join("FordRacing/fordracing.exe"));
        let prefix=if std::fs::read_to_string("/proc/sys/kernel/osrelease").is_ok_and(|s|s.to_ascii_lowercase().contains("microsoft")){
            std::env::var_os("XDG_CACHE_HOME").map(std::path::PathBuf::from).or_else(||std::env::var_os("HOME").map(|h|std::path::PathBuf::from(h).join(".cache"))).ok_or("No Linux cache directory for the Wine prefix")?.join("arcade-foundry/ford-wine")
        }else{output.join("prefix")};
        fs::create_dir_all(&prefix).map_err(|e|e.to_string())?;
        c.env("WINEPREFIX", prefix)
            .env("WINEARCH", "win32")
            .env("WINEDLLOVERRIDES", "inpout32=n,b;mscoree,mshtml=");
        c
    };
    #[cfg(not(any(windows, target_os = "linux")))]
    return Err(
        "Ford currently needs Windows x86 or Linux/Wine; this host has no tested execution backend"
            .into(),
    );
    #[cfg(any(windows, target_os = "linux"))]
    {
        #[cfg(target_os = "linux")]
        {
            command.envs(settings.guest_env());
            if !settings.audio_sink.is_empty(){command.env("PULSE_SINK",&settings.audio_sink);}
        }
        command
            .current_dir(output.join("FordRacing"))
            .env_remove("ARCADE_FORD_PROBE")
            .env(
                "ARCADE_FORD_KEYS",
                arcade_core::controls::Bindings::load(&project.join("controls.cfg"))?
                    .env_value("ford")?,
            );
        let status = command.status().map_err(|e| e.to_string())?;
        if !status.success() {
            return Err(format!(
                "Ford exited with {status}; inspect runtime/ford-racing/FordRacing/arcade-ford.log"
            ));
        }
        Ok("Ford Racing session closed; saves and logs are under runtime/ford-racing".into())
    }
}
