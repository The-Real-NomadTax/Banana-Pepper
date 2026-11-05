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
const MAPS: &[InputMap] = &[
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
    map(324, 331),
    map(325, 332), // mouse -> player one aim
    map(326, 333),
    map(327, 334), // left trigger, right secondary
    map(324, 387),
    map(325, 388),
    map(326, 391),
    map(326, 392), // menu pointer
];

// GameInputMaps can add/rebuild cabinet maps after our first SDL poll. Analog
// maps overwrite earlier destinations, so desktop maps must remain last.
fn order_maps(table: &mut [InputMap], length: usize) -> Result<usize, &'static str> {
    if length > table.len() {
        return Err("invalid input map count");
    }
    let native_count = table[..length].iter().filter(|p| !MAPS.contains(p)).count();
    if native_count + MAPS.len() > table.len() {
        return Err("input map table is full");
    }
    let mut kept = 0;
    for i in 0..length {
        let entry = table[i];
        if !MAPS.contains(&entry) {
            table[kept] = entry;
            kept += 1;
        }
    }
    table[kept..kept + MAPS.len()].copy_from_slice(MAPS);
    Ok(kept + MAPS.len())
}

pub fn initialize() -> Result<(), String> {
    // The launcher also verifies the complete executable SHA-256. These checks
    // guard the pointer and mapping ABI at the point where the shim uses it.
    for (address, expected) in [
        (0x08262940usize, &[0xb8, 0x80, 0x8e, 0xdb, 0x08, 0xc3][..]), // InputGet
        (0x08262f10, &[0x55, 0x57, 0x56, 0x53, 0x83, 0xec, 0x4c][..]), // InputLoop
        (0x0836674b, &[0x55, 0x89, 0xe5, 0x83, 0xec, 0x10][..]),      // raw analog getter
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
    Ok(())
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
    match order_maps(table, unsafe { count.read() } as usize) {
        Ok(length) => unsafe { count.write(length as u32) },
        Err(error) => {
            eprintln!("[jurassic] desktop controls stopped: {error}");
            std::process::exit(1);
        }
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
        let mut table = [InputMap::default(); 1024];
        let first = order_maps(&mut table, 0).unwrap();
        let cabinet = map(351, 331);
        table[first] = cabinet;
        let length = order_maps(&mut table, first + 1).unwrap();
        assert_eq!(table[0], cabinet);
        assert_eq!(
            table[..length].iter().rfind(|m| m.target == 331),
            Some(&map(324, 331))
        );
        assert_eq!(order_maps(&mut table, length).unwrap(), length);
        assert_eq!(order_maps(&mut table, 0).unwrap(), MAPS.len());
    }

    #[test]
    fn invalid_or_full_tables_are_unchanged() {
        let mut table = [map(1, 2); 20];
        let original = table;
        assert!(order_maps(&mut table, 21).is_err());
        assert_eq!(table, original);
        assert!(order_maps(&mut table, 20).is_err());
        assert_eq!(table, original);
    }
}
