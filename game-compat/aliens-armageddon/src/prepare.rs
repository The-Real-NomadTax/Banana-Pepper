//! Build-specific Aliens snapshot preparation. Original assets stay read-only.
use sha2::{Digest, Sha256};
use std::{fs, io::Write, path::Path};

pub const ORIGINAL_SHA256: &str =
    "297c07db82b531e276d4ea7ceb4a11c9ce3aacb436f63526c179c2fd4de3e4db";

/// Cabinet switch source operand → `mov [esp], source` sites in input setup.
/// Targets: P1/P2 Start (+ menu confirm), Test (+ menu select), Service (+ menu
/// back), Volume up/down (+ menu up/down), Coin 1/2. The third Start/Test
/// sites belong to a second input-map setup that re-registers those switches.
const SWITCH_SITES: &[(u32, &[usize])] = &[
    (0x154, &[0x08081376, 0x0808138a, 0x08193dce]),
    (0x155, &[0x0808139e, 0x080813b2, 0x08193de2]),
    (0x150, &[0x080813c6, 0x080813da, 0x08193dba]),
    (0x151, &[0x080813ee, 0x08081402]),
    (0x152, &[0x08081416, 0x0808142a]),
    (0x153, &[0x0808143e, 0x08081452]),
    (0x14c, &[0x08081466]),
    (0x14d, &[0x0808147a]),
];

/// Player two's desktop-mode aim maps (game aim x/y and menu pointer x/y).
/// They read the mouse (sources 0x144/0x145) like player one's; two-player
/// images read the player-two gun sources 0x183/0x184 instead, which the
/// guest shim fills from controller 2 (`aliens_exit.rs`).
const PLAYER_TWO_AIM_SITES: &[(usize, u32, u32)] = &[
    (0x080818d4, 0x144, 0x183),
    (0x080818e8, 0x145, 0x184),
    (0x08081988, 0x144, 0x183),
    (0x0808199c, 0x145, 0x184),
];

/// Player-two-has-the-mouse images: player one's desktop aim maps read the
/// player-one gun sources 0x181/0x182 (filled from player one's controller
/// by the shim), and player one's mouse-button fire maps trade sources with
/// player two's Q/W/E key maps. (address, original source, new source); the
/// preceding `mov [esp+4], target` operand is checked too.
const MOUSE_PLAYER_TWO_SITES: &[(usize, u32, u32, u32)] = &[
    (0x08081870, 0x14b, 0x144, 0x181), // P1 aim x: mouse x -> P1 gun x
    (0x08081884, 0x14c, 0x145, 0x182), // P1 aim y
    (0x08081898, 0x14d, 0x146, 0x71),  // P1 trigger: left button -> Q
    (0x080818ac, 0x14e, 0x147, 0x77),  // P1 secondary: right button -> W
    (0x080818c0, 0x14f, 0x148, 0x65),  // P1 third: middle button -> E
    (0x080818fc, 0x157, 0x71, 0x146),  // P2 trigger: Q -> left button
    (0x08081910, 0x158, 0x77, 0x147),  // P2 secondary: W -> right button
    (0x08081924, 0x159, 0x65, 0x148),  // P2 third: E -> middle button
];

/// How a two-player image routes aim (settings.cfg `players`, `p1_input`,
/// `p2_input`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TwoPlayer {
    /// One player: the engine's own desktop maps, unchanged.
    Off,
    /// Player two aims with a controller (the player-two gun sources); the
    /// mouse stays player one's.
    ControllerPlayerTwo,
    /// Player two has the mouse and its buttons; player one aims with a
    /// controller (the player-one gun sources) and fires with Q/W/E.
    MousePlayerTwo,
}

impl TwoPlayer {
    /// From settings: `players` and `ARCADE_INPUTS` ([`GameSettings::inputs_env`]).
    ///
    /// [`GameSettings::inputs_env`]: banana_settings::GameSettings::inputs_env
    pub fn from_settings(players: u8, inputs: &str) -> Self {
        match (players > 1, inputs) {
            (false, _) => Self::Off,
            (true, "pad,kbm") => Self::MousePlayerTwo,
            (true, _) => Self::ControllerPlayerTwo,
        }
    }
}

/// Writes a separate runtime image and returns its SHA-256. `keys` holds
/// (cabinet switch source, SDL key symbol) pairs from [`crate::controls`].
/// `two_player` routes each player's aim and fire ([`TwoPlayer`]).
pub fn prepare(
    source: &Path,
    destination: &Path,
    keys: &[(u32, u32)],
    two_player: TwoPlayer,
) -> Result<String, String> {
    let mut bytes = fs::read(source).map_err(|e| e.to_string())?;
    if format!("{:x}", Sha256::digest(&bytes)) != ORIGINAL_SHA256 {
        return Err("Unrecognized Aliens build; no changes applied".into());
    }
    let elf = goblin::elf::Elf::parse(&bytes).map_err(|e| e.to_string())?;
    let dynamic = elf
        .program_headers
        .iter()
        .find(|p| p.p_type == goblin::elf::program_header::PT_DYNAMIC)
        .ok_or("Missing dynamic segment")?;
    let offset = dynamic.p_offset as usize;
    let size = dynamic.p_filesz as usize;
    // ESD has no direct imports; audio tests select SDL's dummy backend. The
    // sole CSV import, csv_write2, is implemented by our preloaded Rust shim.
    let mut entries = Vec::new();
    let mut removed = Vec::new();
    for item in bytes[offset..offset + size].as_chunks::<8>().0 {
        let tag = u32::from_le_bytes(item[..4].try_into().unwrap());
        let value = u32::from_le_bytes(item[4..].try_into().unwrap());
        if tag == 1
            && matches!(
                elf.dynstrtab.get_at(value as usize),
                Some("libesd.so.0" | "/usr/lib/libcsv.so")
            )
        {
            removed.push(elf.dynstrtab.get_at(value as usize).unwrap().to_owned());
        } else {
            entries.extend_from_slice(item);
        }
    }
    if removed != ["libesd.so.0", "/usr/lib/libcsv.so"] {
        return Err("Unexpected Aliens legacy dependency table".into());
    }
    drop(elf);
    bytes[offset..offset + size].fill(0);
    bytes[offset..offset + entries.len()].copy_from_slice(&entries);
    // The cabinet supervisor forks a ptrace monitor and invokes an audit error
    // handler before normal initialization when tracing is unavailable. Desktop
    // sessions use the host's process supervision instead of that cabinet loop.
    const SUPERVISOR: usize = 0x08347966 - 0x08048000;
    if bytes.get(SUPERVISOR..SUPERVISOR + 9)
        != Some(&[0x55, 0x89, 0xe5, 0x81, 0xec, 0x38, 0x04, 0x00, 0x00])
    {
        return Err("Unsupported Aliens supervisor site; no snapshot written".into());
    }
    bytes[SUPERVISOR..SUPERVISOR + 6].copy_from_slice(&[0xb8, 1, 0, 0, 0, 0xc3]);
    // Keep -fWIDTHxHEIGHT's requested size, but create an ordinary desktop
    // window. The original flag requests cabinet fullscreen presentation,
    // which SDL 1.2 stretches across every monitor of the X screen. Desktop
    // fullscreen is requested from the window manager by the guest shim.
    const FULLSCREEN: usize = 0x08083bfa - 0x08048000;
    if bytes.get(FULLSCREEN..FULLSCREEN + 7) != Some(&[0xc6, 0x05, 0x51, 0xb5, 0x90, 0x08, 0x01]) {
        return Err("Unsupported Aliens window-mode site; no snapshot written".into());
    }
    bytes[FULLSCREEN + 6] = 0;
    // Select the engine's existing desktop input mode at both flag-reset sites.
    // Cabinet mode refuses Start while its physical guns are disconnected.
    // Desktop mode also installs the engine's own mouse maps.
    for address in [0x08080c39usize, 0x0808486c] {
        let offset = address - 0x08048000;
        if bytes.get(offset..offset + 7) != Some(&[0xc6, 0x05, 0x52, 0xb5, 0x90, 0x08, 0]) {
            return Err("Unsupported Aliens desktop-mode site; no snapshot written".into());
        }
        bytes[offset + 6] = 1;
    }
    // Cabinet switch sources become desktop keys. Each switch is registered
    // once or twice (game action plus its operator-menu meaning); every site is
    // fingerprint-checked before any operand changes.
    let mut patched = Vec::new();
    for &(source, sites) in SWITCH_SITES {
        let key = keys
            .iter()
            .find(|(s, _)| *s == source)
            .map(|(_, k)| *k)
            .ok_or_else(|| format!("No Aliens key for cabinet switch {source:#x}"))?;
        for &address in sites {
            let offset = address - 0x08048000;
            if bytes.get(offset..offset + 3) != Some(&[0xc7, 0x04, 0x24])
                || bytes.get(offset + 3..offset + 7) != Some(source.to_le_bytes().as_slice())
            {
                return Err("Unsupported Aliens input mapping; no snapshot written".into());
            }
            patched.push((offset + 3, key));
        }
    }
    for &(address, mouse, gun) in PLAYER_TWO_AIM_SITES {
        let offset = address - 0x08048000;
        if bytes.get(offset..offset + 3) != Some(&[0xc7, 0x04, 0x24])
            || bytes.get(offset + 3..offset + 7) != Some(mouse.to_le_bytes().as_slice())
        {
            return Err("Unsupported Aliens player-two aim mapping; no snapshot written".into());
        }
        if two_player == TwoPlayer::ControllerPlayerTwo {
            patched.push((offset + 3, gun));
        }
    }
    for &(address, target, original, replacement) in MOUSE_PLAYER_TWO_SITES {
        let offset = address - 0x08048000;
        if offset < 8
            || bytes.get(offset - 8..offset - 4) != Some(&[0xc7, 0x44, 0x24, 0x04])
            || bytes.get(offset - 4..offset) != Some(target.to_le_bytes().as_slice())
            || bytes.get(offset..offset + 3) != Some(&[0xc7, 0x04, 0x24])
            || bytes.get(offset + 3..offset + 7) != Some(original.to_le_bytes().as_slice())
        {
            return Err("Unsupported Aliens player-one aim mapping; no snapshot written".into());
        }
        if two_player == TwoPlayer::MousePlayerTwo {
            patched.push((offset + 3, replacement));
        }
    }
    for (offset, key) in patched {
        bytes[offset..offset + 4].copy_from_slice(&key.to_le_bytes());
    }
    // Supplied cabinet-response records contain the decoded asset seed tables.
    // Restore those tables in the runtime copy so the game's native filename
    // mixer, AES reader and decompressor can consume its production assets.
    // This does not emulate dongle operations or report a successful licence.
    let directory = source.parent().ok_or("Missing Aliens directory")?;
    for (name, length, hash, offset) in [
        (
            "2a3ccf10ef8dff97539ed4b3cd113701",
            512,
            "369ff9829782a47ad878183bb26fae5c5566923c23a1d58d237ef435487756b8",
            0x0082eea0usize,
        ),
        (
            "91a0dda258cc5df430a5678cc72cd603",
            256,
            "411f22ab8a0bb1ead127f7206e7668ce88ad1155fc61ca39b40e065eb3a561dc",
            0x0082f6a0,
        ),
    ] {
        let seed = fs::read(directory.join("hasp").join(name))
            .map_err(|e| format!("Missing Aliens asset seed {name}: {e}"))?;
        if seed.len() != length || format!("{:x}", Sha256::digest(&seed)) != hash {
            return Err(format!(
                "Unsupported Aliens asset seed {name}; no snapshot written"
            ));
        }
        bytes[offset..offset + length].copy_from_slice(&seed);
    }
    if destination.exists()
        && (source.canonicalize().ok() == destination.canonicalize().ok()
            || fs::symlink_metadata(destination).is_ok_and(|m| m.file_type().is_symlink()))
    {
        return Err("Runtime snapshot must be separate from original".into());
    }
    let temp = destination.with_extension("preparing");
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temp)
        .map_err(|e| e.to_string())?;
    let prepared = format!("{:x}", Sha256::digest(&bytes));
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| e.to_string())?;
    drop(file);
    fs::rename(temp, destination).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(destination, fs::Permissions::from_mode(0o755))
            .map_err(|e| e.to_string())?;
    }
    Ok(prepared)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unknown_build_leaves_source_and_destination_untouched() {
        let dir = std::env::temp_dir().join(format!("arcade-aliens-reject-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let source = dir.join("source");
        let destination = dir.join("output");
        fs::write(&source, b"unknown game").unwrap();
        fs::write(&destination, b"existing snapshot").unwrap();
        banana_input::controls::install(vec![&crate::controls::CONTROLS]);
        let keys = banana_input::controls::Bindings::default()
            .codes("aliens")
            .unwrap();
        assert!(prepare(&source, &destination, &keys, TwoPlayer::MousePlayerTwo).is_err());
        assert_eq!(TwoPlayer::from_settings(1, "pad,kbm"), TwoPlayer::Off);
        assert_eq!(TwoPlayer::from_settings(2, "pad,pad"), TwoPlayer::ControllerPlayerTwo);
        assert_eq!(TwoPlayer::from_settings(2, "kbm,pad"), TwoPlayer::ControllerPlayerTwo);
        assert_eq!(TwoPlayer::from_settings(2, "pad,kbm"), TwoPlayer::MousePlayerTwo);
        assert_eq!(fs::read(&source).unwrap(), b"unknown game");
        assert_eq!(fs::read(&destination).unwrap(), b"existing snapshot");
        assert!(!destination.with_extension("preparing").exists());
        fs::remove_dir_all(dir).unwrap();
    }
}
