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

/// Input device nodes (`/dev/input/eventN`) to expose to a sandboxed Linux
/// game: only the kinds its players' input methods read
/// ([`banana_input::pad::Inputs::device_player`]): Xbox-style gamepads,
/// other game controllers, or (Raw Input) mice and light guns. Keyboards
/// and touchpads are never exposed, so keystrokes stay unreadable to the
/// game. Devices must be connected before the game starts.
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
            let Some(number) = node.strip_prefix("event").and_then(|n| n.parse::<u32>().ok()) else {
                continue;
            };
            let read = |path: &str| std::fs::read_to_string(entry.path().join(path)).unwrap_or_default();
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
    let has = |words: &[u64], code: usize| words.get(code / 64).is_some_and(|w| w >> (code % 64) & 1 != 0);
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
pub fn pad_env(project: &Path, game: &str) -> Result<Vec<(&'static str, String)>, String> {
    let controls = banana_input::controls::Bindings::load(&project.join("controls.cfg"))?;
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
        pad_devices(&game_settings(project, game)?.input_assignment())
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
fn device_capabilities() {
    use banana_input::pad::Device;
    // Xbox Wireless Controller (Bluetooth): a gamepad.
    assert_eq!(device_kind("7fff000000000000 0 8000000000 0 0", "0", "3003f"), Some(Device::XInput));
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
    assert_eq!(device_kind("1f0000 0 0 0 0", "143", "0"), Some(Device::Mouse));
    assert_eq!(device_kind("70000 0 0 0 0", "0", "3"), Some(Device::Mouse));
    // A generic USB gamepad (BTN_TRIGGER..BTN_BASE4, X/Y/Z/RZ, hat).
    assert_eq!(device_kind("fff00000000 0 0 0 0", "0", "30027"), Some(Device::DirectInput));
    assert_eq!(device_kind("", "", ""), None);
}

