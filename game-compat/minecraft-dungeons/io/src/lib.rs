//! Build-specific desktop cabinet I/O. No card assets or dongle data live here.

fn parse_scan(line: &[u8]) -> Option<(usize, [u8; 8])> {
    if line.len() != 10 || !(b'1'..=b'4').contains(&line[0]) || line[1] != b' ' {
        return None;
    }
    let code: [u8; 8] = line[2..].try_into().ok()?;
    code.iter()
        .all(u8::is_ascii_alphanumeric)
        .then_some(((line[0] - b'1') as usize, code))
}

#[cfg(test)]
fn parse_key(line: &[u8]) -> Option<(usize, bool)> {
    let text = std::str::from_utf8(line).ok()?;
    let (source, state) = text.split_once(' ')?;
    let source = source.parse::<usize>().ok().filter(|n| *n < 128)?;
    match state {
        "0" => Some((source, false)),
        "1" => Some((source, true)),
        _ => None,
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
#[repr(C)]
struct InputMap {
    source: u32,
    target: u32,
}
const fn map(source: u32, target: u32) -> InputMap {
    InputMap { source, target }
}

fn order_maps(
    table: &mut [InputMap],
    length: usize,
    desktop: &[InputMap],
) -> Result<usize, &'static str> {
    let entries = table.get(..length).ok_or("Invalid native mapping count")?;
    // A desktop key must not also drive the cabinet's original binding (which
    // can join another player). Preserve mappings from actual cabinet devices.
    let owned = |entry: &InputMap| desktop.iter().any(|m| m.source == entry.source);
    let native = entries.iter().filter(|m| !owned(m)).count();
    if native + desktop.len() > table.len() {
        return Err("Native mapping table is full");
    }
    let mut kept = 0;
    for i in 0..length {
        if !owned(&table[i]) {
            table[kept] = table[i];
            kept += 1;
        }
    }
    table[kept..kept + desktop.len()].copy_from_slice(desktop);
    Ok(kept + desktop.len())
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
mod native {
    use std::{collections::VecDeque, ffi::c_void, ptr, time::Duration};

    // All addresses belong to the hash-checked, non-PIE Minecraft 3.07 dump.
    const MUTEXES: usize = 0x1c62e20;
    const RESULTS: usize = 0x1780f00;
    type MutexFn = unsafe extern "C" fn(*mut c_void) -> i32;
    static ACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

    struct Keyboard {
        socket: std::os::unix::net::UnixDatagram,
        sequence: u64,
        held: [bool; 128],
        owned: [bool; 128],
        elapsed: [u32; 128],
        last: std::time::Instant,
        received: std::time::Instant,
    }
    fn bind_socket(path: &str) -> std::io::Result<std::os::unix::net::UnixDatagram> {
        use std::os::unix::fs::FileTypeExt;
        if let Ok(m) = std::fs::symlink_metadata(path) {
            if !m.file_type().is_socket() {
                return Err(std::io::Error::other("Input endpoint is not a socket"));
            }
            std::fs::remove_file(path)?;
        }
        let socket = std::os::unix::net::UnixDatagram::bind(path)?;
        socket.set_nonblocking(true)?;
        Ok(socket)
    }
    impl Keyboard {
        fn open() -> std::io::Result<Self> {
            Ok(Self {
                socket: bind_socket("/work/input.sock")?,
                sequence: 0,
                held: [false; 128],
                owned: [false; 128],
                elapsed: [0; 128],
                last: std::time::Instant::now(),
                received: std::time::Instant::now(),
            })
        }
        unsafe fn merge(&mut self) -> bool {
            let mut presses = [0u32; 128];
            let mut releases = [0u32; 128];
            let mut bytes = [0; 16384];
            for _ in 0..8 {
                let Ok(n) = self.socket.recv(&mut bytes) else {
                    break;
                };
                let Ok(frame) =
                    serde_json::from_slice::<banana_input::transport::InputFrame>(&bytes[..n])
                else {
                    continue;
                };
                if !frame.valid() || frame.sequence <= self.sequence {
                    continue;
                }
                self.sequence = frame.sequence;
                self.received = std::time::Instant::now();
                for (source, down) in frame.edges {
                    let source = source as usize;
                    if source >= 128 {
                        continue;
                    }
                    self.owned[source] = true;
                    let down = down && frame.focused;
                    if self.held[source] != down {
                        if down {
                            presses[source] += 1
                        } else {
                            releases[source] += 1
                        }
                        self.held[source] = down;
                    }
                }
                for source in frame.sources {
                    let source = source as usize;
                    if source >= 128 {
                        continue;
                    }
                    self.owned[source] = true;
                    let down = frame.focused && frame.held.contains(&(source as u16));
                    if self.held[source] != down {
                        if down {
                            presses[source] += 1
                        } else {
                            releases[source] += 1
                        }
                        self.held[source] = down;
                    }
                }
            }
            if self.received.elapsed() > Duration::from_millis(600) {
                for source in 0..128 {
                    if self.held[source] {
                        releases[source] += 1;
                        self.held[source] = false;
                    }
                }
            }
            let now = std::time::Instant::now();
            let dt = now.duration_since(self.last).as_millis().clamp(1, 250) as u32;
            self.last = now;
            for source in 0..128 {
                if !self.owned[source] {
                    continue;
                }
                self.elapsed[source] = if self.held[source] {
                    self.elapsed[source].saturating_add(dt)
                } else {
                    0
                };
                let raw = (0x185ee40 + 0x1248 + source * 48) as *mut u32;
                unsafe {
                    raw.add(1).write(presses[source]);
                    raw.add(2).write(releases[source]);
                    raw.add(3).write(self.elapsed[source]);
                    raw.add(6).write(1);
                }
            }
            self.owned.iter().any(|owned| *owned)
        }
    }

    unsafe fn mutex_call(address: usize, player: usize) -> i32 {
        let call: MutexFn = unsafe { std::mem::transmute(address) };
        unsafe { call((MUTEXES + player * 40) as *mut c_void) }
    }

    unsafe extern "C" fn ready(player: u8) -> i32 {
        i32::from(player < 4 && ACTIVE.load(std::sync::atomic::Ordering::Acquire))
    }
    unsafe extern "C" fn preview() -> *mut c_void {
        ptr::null_mut()
    }

    unsafe extern "C" fn dispenser_status(player: u8) -> u32 {
        // The desktop has no paper mechanism. Report its ready/taken sensor,
        // with empty (0x02, sets P%u_CARDEMPTY), take-card (0x08), card-low
        // (0x20) and jam bits clear. The native dispenser task retains its
        // queue, timing, completion callbacks and audit updates.
        u32::from(player < 4)
    }

    unsafe extern "C" fn worker(_: *mut c_void) -> *mut c_void {
        let mut queues: [VecDeque<[u8; 8]>; 4] = std::array::from_fn(|_| VecDeque::new());
        let socket = match bind_socket("/work/actions.sock") {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Cannot bind session actions: {e}");
                return ptr::null_mut();
            }
        };
        // Physical camera ordering runs before this worker starts. Virtual slots
        // already have fixed player assignments and need no brightness calibration.
        unsafe {
            ptr::write_volatile(0x177e9f0 as *mut u8, 1);
        }
        ACTIVE.store(true, std::sync::atomic::Ordering::Release);
        eprintln!("[minecraft-cards] Four virtual QR scanners ready");
        while unsafe { ptr::read_volatile(0x17ba463 as *const u8) } == 0 {
            let mut bytes = [0; 8192];
            for _ in 0..8 {
                let Ok(n) = socket.recv(&mut bytes) else {
                    break;
                };
                let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes[..n]) else {
                    continue;
                };
                if value["id"] != "scan-card" {
                    continue;
                }
                let Some(player) = value["payload"]["player"]
                    .as_u64()
                    .filter(|p| (1..=4).contains(p))
                else {
                    continue;
                };
                let Some(code) = value["payload"]["code"].as_str() else {
                    continue;
                };
                let line = format!("{player} {code}");
                if let Some((player, code)) = super::parse_scan(line.as_bytes()) {
                    if queues[player].len() < 32 {
                        queues[player].push_back(code);
                    }
                }
            }
            for (player, queue) in queues.iter_mut().enumerate() {
                let Some(code) = queue.front() else {
                    continue;
                };
                if unsafe { mutex_call(0x931e90, player) } != 0 {
                    continue;
                }
                let slot = (RESULTS + player * 12) as *mut u8;
                if unsafe { ptr::read_volatile(slot.add(9)) } == 0 {
                    unsafe {
                        ptr::copy_nonoverlapping(code.as_ptr(), slot, 8);
                        ptr::write_volatile(slot.add(8), 0);
                        ptr::write_volatile(slot.add(10), 1);
                        ptr::write_volatile(slot.add(11), 0);
                        ptr::write_volatile(slot.add(9), 1);
                    }
                    eprintln!(
                        "[minecraft-cards] Delivered P{} {}",
                        player + 1,
                        std::str::from_utf8(code).unwrap()
                    );
                    queue.pop_front();
                }
                unsafe {
                    mutex_call(0x931ed0, player);
                }
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        for player in 0..4 {
            unsafe {
                mutex_call(0x931f00, player);
            }
        }
        ptr::null_mut()
    }

    // SDL2 scancodes -> native cabinet switches (Start and coins are not
    // joystick buttons). Appended after cabinet maps so disconnected RIO data
    // cannot replace a keyboard event. The game still handles credits/debounce.
    const DESKTOP_SWITCHES: &[super::InputMap] = &[
        super::map(40, 0x20c), // Enter: P1 primary action
        super::map(44, 0x20c), // Space: P1 primary action
        super::map(89, 0x20c), // Numpad 1-8
        super::map(90, 0x218),
        super::map(91, 0x224),
        super::map(92, 0x230),
        super::map(93, 0x257),
        super::map(94, 0x25f),
        super::map(95, 0x267),
        super::map(96, 0x26f),
        super::map(30, 0x20c),
        super::map(31, 0x218),
        super::map(32, 0x224),
        super::map(33, 0x230),
        super::map(34, 0x257),
        super::map(35, 0x25f),
        super::map(36, 0x267),
        super::map(37, 0x26f),
        super::map(29, 0x20c),
        super::map(27, 0x20d),
        super::map(6, 0x20e),
        super::map(25, 0x20f),
        super::map(24, 0x218),
        super::map(18, 0x219),
        super::map(19, 0x21a),
        super::map(51, 0x21b),
        super::map(80, 0x2c6),
        super::map(79, 0x2c7),
        super::map(82, 0x2c8),
        super::map(81, 0x2c9),
        super::map(13, 0x2ca),
        super::map(15, 0x2cb),
        super::map(12, 0x2cc),
        super::map(14, 0x2cd),
    ];

    unsafe extern "C" fn input_update() -> i32 {
        let poll: unsafe extern "C" fn() -> i32 = unsafe { std::mem::transmute(0x630f70usize) };
        let status = unsafe { poll() };
        static KEYBOARD: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        if !*KEYBOARD
            .get_or_init(|| std::env::var("ARCADE_MINECRAFT_KEYBOARD").as_deref() != Ok("0"))
        {
            return status;
        }
        static HOST: std::sync::OnceLock<std::sync::Mutex<Option<Keyboard>>> =
            std::sync::OnceLock::new();
        let host = HOST.get_or_init(|| std::sync::Mutex::new(Keyboard::open().ok()));
        if let Ok(mut host) = host.lock()
            && let Some(keyboard) = host.as_mut()
            && unsafe { keyboard.merge() }
        {
            // CoreInputLoop otherwise returns before applying any maps
            // when the old SDL backend reports no new input packet.
            unsafe {
                (0x187185c as *mut u32).write(1);
            }
        }
        static ANNOUNCED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if !ANNOUNCED.swap(true, std::sync::atomic::Ordering::Relaxed) {
            eprintln!("[minecraft-input] Native keyboard cabinet mappings active");
        }
        let count = 0x17c5750 as *mut u32;
        // Called on the core polling thread after SDL events are collected and
        // before the native maps/callbacks run.
        let table =
            unsafe { std::slice::from_raw_parts_mut(0x17c3750 as *mut super::InputMap, 1024) };
        match super::order_maps(table, unsafe { count.read() } as usize, DESKTOP_SWITCHES) {
            Ok(length) => unsafe {
                count.write(length as u32);
            },
            Err(error) => {
                eprintln!("[minecraft-input] {error}");
                unsafe {
                    libc::_exit(1);
                }
            }
        }
        for (source, name) in [(30usize, "P1 Start"), (34, "P1 Coin")] {
            let presses = unsafe { ((0x185ee40 + 0x1248 + source * 48 + 4) as *const u32).read() };
            if presses != 0 {
                eprintln!("[minecraft-input] {name}: {presses} press(es)");
            }
        }
        for player in 0..4 {
            let presses =
                unsafe { ((0x185ee40 + 0x1248 + (58 + player) * 48 + 4) as *const u32).read() };
            if presses != 0 {
                let code =
                    std::env::var("ARCADE_MINECRAFT_CARD").unwrap_or_else(|_| "a0000000".into());
                let value =
                    serde_json::json!({"id":"scan-card","payload":{"player":player+1,"code":code}});
                if let Ok(socket) = std::os::unix::net::UnixDatagram::unbound() {
                    let _ = socket.send_to(value.to_string().as_bytes(), "/work/actions.sock");
                }
            }
        }
        status
    }

    unsafe fn install() -> Result<(), &'static str> {
        let hooks: [(usize, &[u8], usize); 4] = [
            (
                0x41a100,
                &[
                    0x41, 0x56, 0x40, 0x80, 0xff, 0x03, 0x41, 0x55, 0x41, 0x54, 0x55, 0x53, 0xbb,
                    0x03, 0x00, 0x00,
                ],
                dispenser_status as *const () as usize,
            ),
            (
                0x4296e0,
                &[
                    0x41, 0x57, 0x41, 0x56, 0x41, 0x55, 0x41, 0x54, 0x55, 0x53, 0x48, 0x81, 0xec,
                    0x68, 0x01, 0x00,
                ],
                worker as *const () as usize,
            ),
            (
                0x4a0b40,
                &[
                    0x40, 0x0f, 0xb6, 0xff, 0x48, 0x8d, 0x15, 0x39, 0x99, 0x31, 0x01, 0x48, 0x69,
                    0xc7, 0x06, 0x02,
                ],
                ready as *const () as usize,
            ),
            (
                0x4a0cc0,
                &[
                    0x41, 0x57, 0x41, 0x56, 0x41, 0x89, 0xf6, 0x41, 0x55, 0x41, 0x54, 0x4c, 0x8d,
                    0x2d, 0x2e, 0x97,
                ],
                preview as *const () as usize,
            ),
        ];
        let guards: [(usize, &[u8]); 7] = [
            (0x6311b0, &[0x8b, 0x05, 0xa6, 0x06, 0x24, 0x01, 0xc3]),
            (0x4aea6e, &[0xe8, 0xfd, 0x24, 0x18, 0x00]),
            (
                0x630f70,
                &[0x41, 0x54, 0x55, 0x53, 0xe8, 0xb7, 0xda, 0xff, 0xff],
            ),
            (0x419810, &[0x0f, 0xb6, 0x05, 0xd9, 0x51, 0x36, 0x01, 0xc3]),
            (
                0x931e90,
                &[
                    0x48, 0x85, 0xff, 0x74, 0x33, 0x48, 0x83, 0xec, 0x08, 0xe8, 0x62, 0xb8, 0xad,
                    0xff, 0x89, 0xc2,
                ],
            ),
            (
                0x931ed0,
                &[
                    0x48, 0x85, 0xff, 0x74, 0x1b, 0x48, 0x83, 0xec, 0x08, 0xe8, 0x72, 0xa6, 0xad,
                    0xff, 0x85, 0xc0,
                ],
            ),
            (
                0x931f00,
                &[
                    0x48, 0x85, 0xff, 0x74, 0x1b, 0x48, 0x83, 0xec, 0x08, 0xe8, 0xd2, 0xa2, 0xad,
                    0xff, 0x85, 0xc0,
                ],
            ),
        ];
        // Check every ABI before making any changes. The launcher separately hashes the full image.
        for (address, expected) in hooks.iter().map(|(a, b, _)| (*a, *b)).chain(guards) {
            if unsafe { std::slice::from_raw_parts(address as *const u8, expected.len()) }
                != expected
            {
                return Err("Unsupported native scanner ABI");
            }
        }
        // A low-address relay keeps the original five-byte CALL and its return
        // address intact. Only this main-thread polling call installs maps; the
        // shared InputGet getter is also used by worker threads.
        let relay = unsafe {
            libc::mmap(
                ptr::null_mut(),
                4096,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_32BIT,
                -1,
                0,
            )
        };
        if relay == libc::MAP_FAILED {
            return Err("Cannot allocate input call relay");
        }
        let mut jump = [0u8; 14];
        jump[..6].copy_from_slice(&[0xff, 0x25, 0, 0, 0, 0]);
        jump[6..].copy_from_slice(&(input_update as *const () as usize).to_le_bytes());
        unsafe {
            ptr::copy_nonoverlapping(jump.as_ptr(), relay.cast::<u8>(), jump.len());
        }
        if unsafe { libc::mprotect(relay, 4096, libc::PROT_READ | libc::PROT_EXEC) } != 0 {
            return Err("Cannot enable input call relay");
        }
        let displacement = i32::try_from(relay as isize - (0x4aea6eisize + 5))
            .map_err(|_| "Input relay out of range")?;
        // This executable's code LOAD is writable; all bytes were checked above.
        unsafe {
            ptr::copy_nonoverlapping(
                displacement.to_le_bytes().as_ptr(),
                (0x4aea6e + 1) as *mut u8,
                4,
            );
        }
        let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        if page_size <= 0 {
            return Err("Cannot determine page size");
        }
        let page_size = page_size as usize;
        for (address, _, target) in hooks {
            let page = address / page_size * page_size;
            if unsafe {
                libc::mprotect(
                    page as *mut c_void,
                    page_size,
                    libc::PROT_READ | libc::PROT_WRITE | libc::PROT_EXEC,
                )
            } != 0
            {
                return Err("Cannot patch native scanner");
            }
            let mut jump = [0u8; 14];
            jump[..6].copy_from_slice(&[0xff, 0x25, 0, 0, 0, 0]);
            jump[6..].copy_from_slice(&target.to_le_bytes());
            unsafe {
                ptr::copy_nonoverlapping(jump.as_ptr(), address as *mut u8, jump.len());
            }
            // The supplied executable has RWX LOAD segments; preserve that native protection.
        }
        Ok(())
    }

    extern "C" fn init() {
        if std::env::var("ARCADE_MINECRAFT_CARDS").as_deref() != Ok("1") {
            return;
        }
        // Child shell/cabinet helpers inherit LD_PRELOAD, but must never receive game patches.
        if !std::env::current_exe()
            .ok()
            .and_then(|p| p.file_name().map(|n| n == "game.seeded"))
            .unwrap_or(false)
        {
            return;
        }
        if let Err(error) = unsafe { install() } {
            eprintln!("[minecraft-cards] {error}");
            unsafe {
                libc::_exit(1);
            }
        }
        eprintln!("[minecraft-cards] Native QR adapter installed");
    }

    #[used]
    #[unsafe(link_section = ".init_array")]
    static INIT: extern "C" fn() = init;
}

#[cfg(test)]
mod tests {
    use super::{map, order_maps, parse_scan};
    #[test]
    fn keyboard_events_validate_bounds_and_state() {
        assert_eq!(super::parse_key(b"34 1"), Some((34, true)));
        assert_eq!(super::parse_key(b"80 0"), Some((80, false)));
        for event in [
            b"128 1".as_slice(),
            b"-1 0",
            b"30 2",
            b"30 1 extra",
            b"30",
            b"\xff 1",
        ] {
            assert_eq!(super::parse_key(event), None);
        }
    }
    #[test]
    fn desktop_maps_follow_late_cabinet_maps_without_growing() {
        let desktop = [map(30, 0x24e), map(34, 0x257)];
        let mut table = [map(0, 0); 8];
        let count = order_maps(&mut table, 0, &desktop).unwrap();
        table[count] = map(0x24e, 0x24e);
        let count = order_maps(&mut table, count + 1, &desktop).unwrap();
        assert_eq!(
            &table[..count],
            &[map(0x24e, 0x24e), desktop[0], desktop[1]]
        );
        assert_eq!(order_maps(&mut table, count, &desktop), Ok(count));
    }

    #[test]
    fn invalid_or_full_input_tables_remain_unchanged() {
        let mut table = [map(1, 2); 2];
        let original = table;
        assert!(order_maps(&mut table, 3, &[map(30, 0x24e)]).is_err());
        assert_eq!(table, original);
        assert!(order_maps(&mut table, 2, &[map(30, 0x24e)]).is_err());
        assert_eq!(table, original);
    }
    #[test]
    fn desktop_keys_replace_conflicting_native_bindings() {
        let desktop = [map(30, 0x20c), map(29, 0x20c)];
        let mut table = [map(30, 0x224), map(0x239, 0x20c), map(29, 0x257), map(0, 0)];
        let count = order_maps(&mut table, 3, &desktop).unwrap();
        assert_eq!(
            &table[..count],
            &[map(0x239, 0x20c), desktop[0], desktop[1]]
        );
        assert_eq!(order_maps(&mut table, count, &desktop), Ok(count));
    }
    #[test]
    fn preserves_native_codes_and_player_indices() {
        assert_eq!(parse_scan(b"1 a10g0000"), Some((0, *b"a10g0000")));
        assert_eq!(parse_scan(b"4 c01A1000"), Some((3, *b"c01A1000")));
    }
    #[test]
    fn rejects_bad_players_lengths_and_non_ascii_codes() {
        for line in [
            b"0 a0000000".as_slice(),
            b"5 a0000000",
            b"1 a000000",
            b"1 a00000000",
            b"1 a00000/0",
            b"1\ta0000000",
        ] {
            assert_eq!(parse_scan(line), None);
        }
    }
}
