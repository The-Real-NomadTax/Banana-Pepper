//! Shared game preparation and process orchestration: settings and
//! controller hand-off, content resolution and launch dispatch. Individual
//! games supply declarative compatibility plans through `banana-api`.
pub mod content;
pub mod dependencies;
pub mod process;

mod configuration;
mod migration;
mod orchestrator;
pub mod storage;

use banana_settings::{GameSettings, Settings};
use std::path::{Path, PathBuf};

/// Distribution root beside the executable, or the checkout for development.
/// Resolve from the executable rather than the working directory so shortcuts
/// and relocated bundles resolve their own immutable defaults and adapters.
pub fn project_root() -> Result<PathBuf, String> {
    root_for(
        &std::env::current_exe().map_err(|e| e.to_string())?,
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."),
    )
}

pub const DIST_MARKER: &str = "banana-pepper.dist";

fn root_for(executable: &Path, development: &Path) -> Result<PathBuf, String> {
    let directory = executable
        .parent()
        .ok_or("Executable has no parent directory")?;
    for root in directory.ancestors().take(3) {
        if is_distribution(root) {
            return root.canonicalize().map_err(|e| e.to_string());
        }
    }
    development.canonicalize().map_err(|e| e.to_string())
}

pub fn is_distribution(root: &Path) -> bool {
    root.join(DIST_MARKER).is_file()
}

/// Bundles use their organized resources exclusively; never fall back to a
/// developer's checkout when an adapter is missing from a shipped package.
pub fn resource_path(root: &Path, bundled: &str, development: &str) -> PathBuf {
    root.join(if is_distribution(root) {
        bundled
    } else {
        development
    })
}

#[cfg(test)]
mod path_tests {
    use super::*;

    #[test]
    fn relocated_bundle_and_nested_helpers_ignore_a_missing_checkout() {
        let temp = tempfile::tempdir().unwrap();
        let bundle = temp.path().join("Moved Banana Pepper [portable]");
        std::fs::create_dir_all(bundle.join("helpers/linux")).unwrap();
        std::fs::write(bundle.join(DIST_MARKER), "v1").unwrap();
        let missing = temp.path().join("deleted checkout");
        let expected = bundle.canonicalize().unwrap();
        assert_eq!(
            root_for(&bundle.join("arcade-desktop.exe"), &missing).unwrap(),
            expected
        );
        assert_eq!(
            root_for(&bundle.join("helpers/linux/arcade-desktop"), &missing).unwrap(),
            expected
        );
        // A missing shipped adapter must not resolve to a development artifact.
        assert_eq!(
            resource_path(&bundle, "adapters/shim.so", "target/debug/shim.so"),
            bundle.join("adapters/shim.so")
        );
    }

    #[test]
    fn development_executables_keep_the_checkout_layout() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        assert_eq!(
            root_for(&root.join("target/debug/arcade-desktop"), &root).unwrap(),
            root
        );
        assert_eq!(
            resource_path(&root, "adapters/shim.so", "target/debug/shim.so"),
            root.join("target/debug/shim.so")
        );
    }
}

pub fn run(id: &str, root: &Path, action: &str) -> Result<String, String> {
    orchestrator::run(id, root, banana_api::ActionRequest::new(action))
}
pub use orchestrator::{SessionReport, run_report, start};
/// Per-game settings from settings.cfg in the project root.
pub fn game_settings(_project: &Path, game: &str) -> Result<GameSettings, String> {
    Ok(
        Settings::load(&storage::Paths::discover()?.config.join("settings.cfg"))?
            .game(game)
            .clone(),
    )
}

#[cfg(target_os = "linux")]
pub fn pad_devices(inputs: &banana_input::pad::Inputs) -> Vec<std::path::PathBuf> {
    let mut devices = Vec::new();
    let Ok(entries) = std::fs::read_dir("/sys/class/input") else {
        return devices;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with("event") {
            continue;
        }
        let caps = |kind: &str| {
            std::fs::read_to_string(entry.path().join("device/capabilities").join(kind))
                .unwrap_or_default()
        };
        let node = Path::new("/dev/input").join(&name);
        if device_kind(&caps("key"), &caps("rel"), &caps("abs"))
            .is_some_and(|kind| inputs.device_player(kind, 0).is_some())
            && node.exists()
        {
            devices.push(node);
        }
    }
    devices.sort();
    devices
}

/// Names of the connected devices of `kind`, in the order the game
/// adapters number them (by device node). Empty on other systems, where
/// devices are found when the game starts.
pub fn device_names(kind: banana_input::pad::Device) -> Vec<String> {
    #[cfg(target_os = "linux")]
    {
        let mut found: Vec<(u32, String)> = Vec::new();
        let Ok(entries) = std::fs::read_dir("/sys/class/input") else {
            return Vec::new();
        };
        for entry in entries.flatten() {
            let node = entry.file_name().to_string_lossy().into_owned();
            let Some(number) = node
                .strip_prefix("event")
                .and_then(|n| n.parse::<u32>().ok())
            else {
                continue;
            };
            let read =
                |path: &str| std::fs::read_to_string(entry.path().join(path)).unwrap_or_default();
            let caps = |kind: &str| read(&format!("device/capabilities/{kind}"));
            if device_kind(&caps("key"), &caps("rel"), &caps("abs")) == Some(kind) {
                found.push((number, read("device/name").trim().to_owned()));
            }
        }
        // The adapters sort device nodes by path ("event10" before "event2").
        found.sort_by(|a, b| format!("event{}", a.0).cmp(&format!("event{}", b.0)));
        found.into_iter().map(|(_, name)| name).collect()
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = kind;
        Vec::new()
    }
}

/// A device's kind from its sysfs capability bitmaps (hex words, most
/// significant first; 64-bit words on x86_64/aarch64 kernels).
#[cfg(any(target_os = "linux", test))]
fn device_kind(key: &str, rel: &str, abs: &str) -> Option<banana_input::pad::Device> {
    fn bits(caps: &str) -> Vec<u64> {
        caps.split_whitespace()
            .rev()
            .map(|w| u64::from_str_radix(w, 16).unwrap_or(0))
            .collect()
    }
    let has = |words: &[u64], code: usize| {
        words
            .get(code / 64)
            .is_some_and(|w| w >> (code % 64) & 1 != 0)
    };
    let (key, rel, abs) = (bits(key), bits(rel), bits(abs));
    banana_input::pad::classify(
        |code| has(&key, usize::from(code)),
        has(&rel, 0) && has(&rel, 1),
        has(&abs, 0) && has(&abs, 1),
    )
}

/// Controller settings for a game: `ARCADE_PAD` (`target:mask,...`),
/// `ARCADE_PAD_P2` for controller 2 in two-player games and
/// `ARCADE_PAD_DEVICES` (colon-separated device nodes) for the Linux
/// sandboxes. Games whose adapters press keys for controller buttons add
/// [`pad_key_env`] themselves.
pub fn pad_env(_project: &Path, game: &str) -> Result<Vec<(&'static str, String)>, String> {
    let controls = banana_input::controls::Bindings::load(
        &storage::Paths::discover()?.config.join("controls.cfg"),
    )?;
    let mut env = vec![("ARCADE_PAD", controls.pad_env_value(game)?)];
    // Controller 2 in two-player games (used when settings.cfg players = 2).
    let p2 = controls.pad_env_value_p2(game)?;
    if !p2.is_empty() {
        env.push(("ARCADE_PAD_P2", p2));
    }
    // Each player's movement axes (aim or steering).
    let moves = controls.move_env_value(game, 0);
    if !moves.is_empty() {
        env.push(("ARCADE_MOVE", moves));
        env.push(("ARCADE_MOVE_P2", controls.move_env_value(game, 1)));
    }
    #[cfg(target_os = "linux")]
    env.push((
        "ARCADE_PAD_DEVICES",
        pad_devices(&game_settings(_project, game)?.input_assignment())
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(":"),
    ));
    Ok(env)
}

/// `ARCADE_PAD_KEYS` and `ARCADE_PAD_KEYS_P2`: controller buttons mapped to
/// the key codes bound to each action, for adapters that inject keystrokes.
pub fn pad_key_env(_project: &Path, game: &str) -> Result<Vec<(&'static str, String)>, String> {
    let controls = banana_input::controls::Bindings::load(
        &storage::Paths::discover()?.config.join("controls.cfg"),
    )?;
    Ok(vec![
        ("ARCADE_PAD_KEYS", controls.pad_key_env_value(game)?),
        ("ARCADE_PAD_KEYS_P2", controls.pad_key_env_value_p2(game)?),
    ])
}

#[cfg(test)]
#[test]
fn device_capabilities() {
    use banana_input::pad::Device;
    // Xbox Wireless Controller (Bluetooth): a gamepad.
    assert_eq!(
        device_kind("7fff000000000000 0 8000000000 0 0", "0", "3003f"),
        Some(Device::XInput)
    );
    // A full keyboard that also reports BTN_SOUTH (remote-desktop virtual keyboard).
    assert_eq!(
        device_kind(
            "ffffffffff 0 7ffffff000301ff 1000f7fffffff 7fe001fffff000f 7ffffffffffffff \
             ffffffff0003fdff 7fff8fff00ff03ff 1ffffffffffff07 ffffffffffffffff ffffffffffefffff fffffffffffffffe",
            "3",
            "3"
        ),
        None
    );
    // A USB mouse (BTN_LEFT..BTN_EXTRA, REL_X/Y/WHEEL) and an absolute light gun.
    assert_eq!(
        device_kind("1f0000 0 0 0 0", "143", "0"),
        Some(Device::Mouse)
    );
    assert_eq!(device_kind("70000 0 0 0 0", "0", "3"), Some(Device::Mouse));
    // A generic USB gamepad (BTN_TRIGGER..BTN_BASE4, X/Y/Z/RZ, hat).
    assert_eq!(
        device_kind("fff00000000 0 0 0 0", "0", "30027"),
        Some(Device::DirectInput)
    );
    assert_eq!(device_kind("", "", ""), None);
}

/// Dispatch a structured command to an existing session; no game utility executable.
pub fn send_action(entry: &str, action: banana_api::ActionRequest) -> Result<(), String> {
    let paths = storage::Paths::discover()?;
    let record = storage::entries(&paths)?
        .into_iter()
        .find(|r| r.id == entry)
        .ok_or("Unknown library entry")?;
    let game = banana_api::game(&record.game).ok_or("Unknown game")?;
    if !storage::valid_identity(&record.id) {
        return Err("Invalid entry identity".into());
    }
    game.validate_session_action(&action)?;
    #[cfg(unix)]
    {
        let bytes = serde_json::to_vec(&action).map_err(|e| e.to_string())?;
        if bytes.len() > 8192 {
            return Err("Action payload exceeds 8 KiB".into());
        }
        let socket = std::os::unix::net::UnixDatagram::unbound().map_err(|e| e.to_string())?;
        socket.set_nonblocking(true).map_err(|e| e.to_string())?;
        socket
            .send_to(
                &bytes,
                paths
                    .state
                    .join("entries")
                    .join(&record.id)
                    .join("actions.sock"),
            )
            .map_err(|e| format!("Session is unavailable: {e}"))?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let _ = action;
        Err("This backend has no active session command transport".into())
    }
}

/// Explicit user reset, protected by the same single-writer lock as a game session.
pub fn reset_overlay(entry: &str, path: Option<&str>) -> Result<(), String> {
    let paths = storage::Paths::discover()?;
    let record = storage::entries(&paths)?
        .into_iter()
        .find(|r| r.id == entry)
        .ok_or("Unknown library entry")?;
    if !storage::valid_identity(&record.id) {
        return Err("Invalid entry identity".into());
    }
    banana_vfs::reset_overlay(&paths.data.join("overlays").join(record.id), path)
        .map_err(|e| e.to_string())
}
