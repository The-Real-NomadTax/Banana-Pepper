//! Shared game preparation and process orchestration: settings and
//! controller hand-off, content resolution and launch dispatch. Individual
//! games supply their own launchers through `banana-api`.
pub mod content;

use banana_settings::{GameSettings, Settings};
use std::path::{Path, PathBuf};

/// The repository root, which holds `settings.cfg`, `controls.cfg`,
/// `scripts/`, `runtime/` and the guest build outputs in `target/`.
pub fn project_root() -> Result<PathBuf, String> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .map_err(|e| e.to_string())
}

/// Run `action` for the registered game `id` against content below `root`.
pub fn run(id: &str, root: &Path, action: &str) -> Result<String, String> {
    let game = banana_api::game(id).ok_or_else(|| format!("Unknown game {id}"))?;
    game.run(root, action)
}

/// Per-game settings from settings.cfg in the project root.
pub fn game_settings(project: &Path, game: &str) -> Result<GameSettings, String> {
    Ok(Settings::load(&project.join("settings.cfg"))?
        .game(game)
        .clone())
}

/// Hand settings to a Python launch harness: audio choices for arcade_audio.py
/// and guest environment variables for arcade_settings.py.
pub fn settings_env(command: &mut std::process::Command, settings: &GameSettings) {
    command
        .env("ARCADE_AUDIO", if settings.audio { "1" } else { "0" })
        .env("ARCADE_RESOLUTION", settings.resolution_text())
        .env("ARCADE_GUEST_ENV", settings.guest_env_value());
    if settings.audio_sink.is_empty() {
        command.env_remove("ARCADE_AUDIO_SINK");
    } else {
        command.env("ARCADE_AUDIO_SINK", &settings.audio_sink);
    }
}

/// Controller device nodes (`/dev/input/eventN`) to expose to a sandboxed
/// Linux game: devices reporting gamepad buttons (BTN_SOUTH) that are not
/// keyboards (no KEY_A), so keystrokes are never readable by the game.
/// Controllers must be connected before the game starts.
#[cfg(target_os = "linux")]
pub fn pad_devices() -> Vec<std::path::PathBuf> {
    let mut devices = Vec::new();
    let Ok(entries) = std::fs::read_dir("/sys/class/input") else {
        return devices;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with("event") {
            continue;
        }
        let caps = std::fs::read_to_string(entry.path().join("device/capabilities/key"))
            .unwrap_or_default();
        let node = Path::new("/dev/input").join(&name);
        if is_gamepad(&caps) && node.exists() {
            devices.push(node);
        }
    }
    devices.sort();
    devices
}

/// sysfs key capability bitmap (hex words, most significant first).
#[cfg(any(target_os = "linux", test))]
fn is_gamepad(caps: &str) -> bool {
    let words: Vec<u64> = caps
        .split_whitespace()
        .rev()
        .map(|w| u64::from_str_radix(w, 16).unwrap_or(0))
        .collect();
    // 64-bit words on x86_64/aarch64 kernels.
    let has = |code: usize| words.get(code / 64).is_some_and(|w| w >> (code % 64) & 1 != 0);
    has(0x130) && !has(30)
}

/// Controller settings for a game: `ARCADE_PAD` (`target:mask,...`),
/// `ARCADE_PAD_P2` for controller 2 in two-player games and
/// `ARCADE_PAD_DEVICES` (colon-separated device nodes) for the Linux
/// sandboxes. Games whose adapters press keys for controller buttons add
/// [`pad_key_env`] themselves.
pub fn pad_env(project: &Path, game: &str) -> Result<Vec<(&'static str, String)>, String> {
    let controls = banana_input::controls::Bindings::load(&project.join("controls.cfg"))?;
    let mut env = vec![("ARCADE_PAD", controls.pad_env_value(game)?)];
    // Controller 2 in two-player games (used when settings.cfg players = 2).
    let p2 = controls.pad_env_value_p2(game)?;
    if !p2.is_empty() {
        env.push(("ARCADE_PAD_P2", p2));
    }
    #[cfg(target_os = "linux")]
    env.push((
        "ARCADE_PAD_DEVICES",
        pad_devices()
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(":"),
    ));
    Ok(env)
}

/// `ARCADE_PAD_KEYS` and `ARCADE_PAD_KEYS_P2`: controller buttons mapped to
/// the key codes bound to each action, for adapters that inject keystrokes.
pub fn pad_key_env(project: &Path, game: &str) -> Result<Vec<(&'static str, String)>, String> {
    let controls = banana_input::controls::Bindings::load(&project.join("controls.cfg"))?;
    Ok(vec![
        ("ARCADE_PAD_KEYS", controls.pad_key_env_value(game)?),
        ("ARCADE_PAD_KEYS_P2", controls.pad_key_env_value_p2(game)?),
    ])
}

#[cfg(test)]
#[test]
fn gamepad_capabilities() {
    // Xbox Wireless Controller (Bluetooth) and a full keyboard that also
    // reports BTN_SOUTH (a remote-desktop virtual keyboard).
    assert!(is_gamepad("7fff000000000000 0 8000000000 0 0"));
    assert!(!is_gamepad(
        "ffffffffff 0 7ffffff000301ff 1000f7fffffff 7fe001fffff000f 7ffffffffffffff \
         ffffffff0003fdff 7fff8fff00ff03ff 1ffffffffffff07 ffffffffffffffff ffffffffffefffff fffffffffffffffe"
    ));
    assert!(!is_gamepad("30000 0 0 0 0")); // mouse
    assert!(!is_gamepad(""));
}

