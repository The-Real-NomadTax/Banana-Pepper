//! Desktop inputs for the fingerprint-verified Jurassic engine snapshot.
use std::sync::atomic::{AtomicU32, Ordering};

// The banner reads a display-only copy; cabinet status and logs remain real.
static DISPLAY_ERRORS: AtomicU32 = AtomicU32::new(0);
const ERROR_DISPLAY_READS: &[(usize, &[u8])] = &[
    (0x082913ab, &[0xa1, 0x68, 0x06, 0xb5, 0x08]),
    (0x0829152b, &[0xa1, 0x68, 0x06, 0xb5, 0x08]),
    (0x082915a0, &[0x8b, 0x15, 0x68, 0x06, 0xb5, 0x08]),
    (0x082915ff, &[0x8b, 0x15, 0x68, 0x06, 0xb5, 0x08]),
    (0x0829161c, &[0xa1, 0x68, 0x06, 0xb5, 0x08]),
    (0x08291684, &[0x8b, 0x15, 0x68, 0x06, 0xb5, 0x08]),
    (0x082916e9, &[0x8b, 0x0d, 0x68, 0x06, 0xb5, 0x08]),
    (0x082917c4, &[0xa1, 0x68, 0x06, 0xb5, 0x08]),
    (0x082917ee, &[0xa1, 0x68, 0x06, 0xb5, 0x08]),
];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(C)]
struct InputMap {
    source: i32,
    target: i32,
}

const fn map(source: i32, target: i32) -> InputMap {
    InputMap { source, target }
}

// Sources are raw IO/SDL enums; targets are CoreInput enums. Keyboard symbols
// match in both enums, while cabinet and gun inputs have different layouts.
// Button bindings come from controls.cfg via ARCADE_JURASSIC_KEYS
// ("target:source,..."); these defaults apply when it is absent.
const DEFAULT_BUTTONS: &[InputMap] = &[
    map(49, 371),
    map(50, 372), // starts: 1, 2
    map(53, 379),
    map(54, 380), // coins: 5, 6
    map(116, 378),
    map(57, 377), // test: T, service: 9
    map(273, 383),
    map(274, 384),
    map(13, 385),
    map(8, 386), // menus
    map(326, 333),
    map(327, 334), // left trigger, right secondary
];
// Mouse position always drives player-one aim and the menu pointer.
const POINTER_MAPS: &[InputMap] = &[
    map(324, 331),
    map(325, 332), // mouse -> player one aim
    map(324, 387),
    map(325, 388),
    map(326, 391),
    map(326, 392), // menu pointer
];
// Two players: the player-two gun sources (raw 353/354, filled from
// controller 2 by `player_two_aim`) drive player two's aim. The cabinet's own
// tables map them the same way; desktop maps are re-added last regardless.
const PLAYER_TWO_MAPS: &[InputMap] = &[map(353, 341), map(354, 342)];
// 343/344 are player two's trigger and secondary (controller 2 only).
const BUTTON_TARGETS: &[i32] = &[
    371, 372, 379, 380, 378, 377, 383, 384, 385, 386, 333, 334, 343, 344,
];
/// Raw input record for a source (`InputGetRaw`), and the game's analog
/// setter, which scales a raw value by the record's configured range.
const RAW_ANALOG: usize = 0x0836674b;
const SET_ANALOG: usize = 0x08366105;
/// Mouse x/y sources and the player-two gun x/y sources.
const MOUSE_AXES: [i32; 2] = [324, 325];
const PLAYER_TWO_AXES: [i32; 2] = [353, 354];
static TWO_PLAYERS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

static MAPS: std::sync::OnceLock<Vec<InputMap>> = std::sync::OnceLock::new();

fn parse_buttons(value: &str) -> Result<Vec<InputMap>, String> {
    let mut maps = Vec::new();
    for pair in value.split(',').filter(|p| !p.trim().is_empty()) {
        let (target, source) = pair
            .split_once(':')
            .ok_or_else(|| format!("bad key binding \"{pair}\""))?;
        let target: i32 = target
            .trim()
            .parse()
            .map_err(|_| format!("bad target \"{pair}\""))?;
        let source: i32 = source
            .trim()
            .parse()
            .map_err(|_| format!("bad key \"{pair}\""))?;
        if !BUTTON_TARGETS.contains(&target) || !(1..=327).contains(&source) || source == 27 {
            return Err(format!("unsupported key binding \"{pair}\""));
        }
        maps.push(map(source, target));
    }
    Ok(maps)
}

fn build_maps(
    buttons: Option<&str>,
    pad: &[(u32, i32)],
    two_players: bool,
) -> Result<Vec<InputMap>, String> {
    let mut maps = match buttons {
        Some(value) => parse_buttons(value)?,
        None => DEFAULT_BUTTONS.to_vec(),
    };
    // Controller buttons arrive as private world keysyms (jurassic_pad.rs).
    for &(target, keysym) in pad {
        if BUTTON_TARGETS.contains(&(target as i32)) {
            maps.push(map(keysym, target as i32));
        }
    }
    // Pointer maps last so they win over any analog destination overlap.
    maps.extend_from_slice(POINTER_MAPS);
    if two_players {
        maps.extend_from_slice(PLAYER_TWO_MAPS);
    }
    maps.dedup();
    Ok(maps)
}

fn maps() -> &'static [InputMap] {
    MAPS.get().map(Vec::as_slice).unwrap_or(&[])
}

// GameInputMaps can add/rebuild cabinet maps after our first SDL poll. Analog
// maps overwrite earlier destinations, so desktop maps must remain last.
fn order_maps(
    table: &mut [InputMap],
    length: usize,
    desktop: &[InputMap],
) -> Result<usize, &'static str> {
    if length > table.len() {
        return Err("invalid input map count");
    }
    let native_count = table[..length]
        .iter()
        .filter(|p| !desktop.contains(p))
        .count();
    if native_count + desktop.len() > table.len() {
        return Err("input map table is full");
    }
    let mut kept = 0;
    for i in 0..length {
        let entry = table[i];
        if !desktop.contains(&entry) {
            table[kept] = entry;
            kept += 1;
        }
    }
    table[kept..kept + desktop.len()].copy_from_slice(desktop);
    Ok(kept + desktop.len())
}

pub fn initialize() -> Result<(), String> {
    let buttons = std::env::var("ARCADE_JURASSIC_KEYS").ok();
    let pad = crate::jurassic_pad::sources(&crate::jurassic_pad::configured_bindings());
    let two_players = crate::jurassic_pad::players() > 1;
    let built = build_maps(buttons.as_deref(), &pad, two_players)?;
    eprintln!(
        "[jurassic] {} desktop input maps ({})",
        built.len(),
        if buttons.is_some() {
            "controls.cfg"
        } else {
            "defaults"
        }
    );
    let _ = MAPS.set(built);
    // The launcher also verifies the complete executable SHA-256. These checks
    // guard the pointer and mapping ABI at the point where the shim uses it.
    for (address, expected) in [
        (0x08262940usize, &[0xb8, 0x80, 0x8e, 0xdb, 0x08, 0xc3][..]), // InputGet
        (0x08262f10, &[0x55, 0x57, 0x56, 0x53, 0x83, 0xec, 0x4c][..]), // InputLoop
        (RAW_ANALOG, &[0x55, 0x89, 0xe5, 0x83, 0xec, 0x10][..]),      // raw analog getter
        (SET_ANALOG, &[0x55, 0x89, 0xe5, 0x83, 0xec, 0x10][..]),      // raw analog setter
    ]
    .into_iter()
    .chain(ERROR_DISPLAY_READS.iter().copied())
    {
        if unsafe { std::slice::from_raw_parts(address as *const u8, expected.len()) } != expected {
            return Err(format!("incompatible desktop input ABI at {address:#x}"));
        }
    }
    // Redirect only reads in the banner coroutine and its creation helper.
    // Validate all sites before changing any operand. This snapshot maps its
    // executable writable; only the loaded image is changed, never the file.
    for &(address, expected) in ERROR_DISPLAY_READS {
        unsafe {
            ((address + expected.len() - 4) as *mut u32)
                .write_unaligned(DISPLAY_ERRORS.as_ptr() as usize as u32);
        }
    }
    eprintln!("[jurassic] desktop mode: physical RIO unavailable; disconnected overlay suppressed");
    TWO_PLAYERS.store(two_players, Ordering::Relaxed);
    Ok(())
}

/// The game's analog input record (0x2c bytes): flags, scaled value, delta,
/// raw value, raw delta, then the min/max/range and output scale words.
type Analog = [u32; 11];

/// Feeds controller 2's aim to the player-two gun sources, scaled exactly like
/// the mouse: copy the mouse record's flags and ranges, then set the raw value
/// in window pixels through the game's own setter (as its mouse handler does).
unsafe fn player_two_aim() {
    let Some((x, y)) = crate::jurassic_pad::player_two_aim() else {
        return;
    };
    let get: unsafe extern "C" fn(i32) -> *mut Analog = unsafe { std::mem::transmute(RAW_ANALOG) };
    let set: unsafe extern "C" fn(*mut Analog, f32) = unsafe { std::mem::transmute(SET_ANALOG) };
    for ((mouse, gun), value) in MOUSE_AXES.into_iter().zip(PLAYER_TWO_AXES).zip([x, y]) {
        let (mouse, gun) = unsafe { (get(mouse), get(gun)) };
        if mouse.is_null() || gun.is_null() {
            continue;
        }
        unsafe {
            let config = *mouse;
            let words = gun.cast::<u32>();
            for i in [0, 5, 6, 7, 8, 9, 10] {
                words.add(i).write(config[i]);
            }
            set(gun, value);
        }
    }
}

/// Called on the engine's SDL polling thread, before maps and callbacks run.
/// No switch state is synthesized: native SDL retains presses/releases/holds.
pub unsafe fn update() {
    DISPLAY_ERRORS.store(
        unsafe { (0x08b50668 as *const u32).read() } & !1,
        Ordering::Relaxed,
    );
    let count = 0x08b505f8 as *mut u32;
    let table = unsafe { std::slice::from_raw_parts_mut(0x08db6e80 as *mut InputMap, 1024) };
    match order_maps(table, unsafe { count.read() } as usize, maps()) {
        Ok(length) => unsafe { count.write(length as u32) },
        Err(error) => {
            eprintln!("[jurassic] desktop controls stopped: {error}");
            std::process::exit(1);
        }
    }
    if TWO_PLAYERS.load(Ordering::Relaxed) {
        unsafe { player_two_aim() };
    }
    // Hardware smoothing blends mapped aim with absent physical gun samples.
    // Absolute desktop mouse input needs no physical-gun smoothing.
    unsafe {
        *(0x08b505fc as *mut u32) &= !1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_aim_follows_late_cabinet_maps_and_survives_rebuilds() {
        let desktop = build_maps(None, &[], false).unwrap();
        let mut table = [InputMap::default(); 1024];
        let first = order_maps(&mut table, 0, &desktop).unwrap();
        let cabinet = map(351, 331);
        table[first] = cabinet;
        let length = order_maps(&mut table, first + 1, &desktop).unwrap();
        assert_eq!(table[0], cabinet);
        assert_eq!(
            table[..length].iter().rfind(|m| m.target == 331),
            Some(&map(324, 331))
        );
        assert_eq!(order_maps(&mut table, length, &desktop).unwrap(), length);
        assert_eq!(order_maps(&mut table, 0, &desktop).unwrap(), desktop.len());
    }

    #[test]
    fn invalid_or_full_tables_are_unchanged() {
        let desktop = build_maps(None, &[], false).unwrap();
        let mut table = [map(1, 2); 20];
        let original = table;
        assert!(order_maps(&mut table, 21, &desktop).is_err());
        assert_eq!(table, original);
        assert!(order_maps(&mut table, 20, &desktop).is_err());
        assert_eq!(table, original);
    }

    #[test]
    fn configured_buttons_replace_defaults_and_keep_pointer_maps() {
        let defaults = build_maps(None, &[], false).unwrap();
        // Same order as banana-input's default controls.cfg output.
        let configured = build_maps(
            Some("371:49,372:50,379:53,380:54,378:116,377:57,383:273,384:274,385:13,386:8,333:326,334:327"),
            &[],
            false,
        )
        .unwrap();
        assert_eq!(configured, defaults);
        let custom = build_maps(Some("378:290,333:32,333:326"), &[], false).unwrap();
        let with_pad = build_maps(Some("378:290"), &[(333, 160), (999, 161)], false).unwrap();
        assert_eq!(&with_pad[..2], &[map(290, 378), map(160, 333)]);
        assert!(with_pad.ends_with(POINTER_MAPS));
        assert_eq!(&custom[..3], &[map(290, 378), map(32, 333), map(326, 333)]);
        assert!(custom.ends_with(POINTER_MAPS));
        assert!(build_maps(Some("999:49"), &[], false).is_err());
        assert!(build_maps(Some("378:27"), &[], false).is_err());
        assert!(build_maps(Some("378"), &[], false).is_err());
    }

    #[test]
    fn two_players_add_controller_two_aim_and_buttons() {
        let two = build_maps(None, &[(343, 168), (372, 169)], true).unwrap();
        assert!(two.ends_with(PLAYER_TWO_MAPS));
        assert!(two.contains(&map(168, 343)) && two.contains(&map(169, 372)));
        let one = build_maps(None, &[], false).unwrap();
        assert!(!one.iter().any(|m| PLAYER_TWO_MAPS.contains(m)));
        assert!(one.ends_with(POINTER_MAPS));
    }
}
