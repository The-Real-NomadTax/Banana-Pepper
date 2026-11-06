//! Fast and Furious Super Cars desktop adapter, loaded by Wine as `dinput8.dll`.
//!
//! The game reads its cabinet controls through a Raw Thrills I/O board that a
//! desktop does not have. After the game's own per-frame input poll this
//! library turns DirectInput keyboard state into the same input events the
//! board driver would post (start, coins, test, views, gears, keypad) and
//! writes the steering/gas/brake analog channels. `DirectInput8Create` is
//! forwarded to Wine's real dinput8.
//!
//! Everything is patched in memory only, after checking the exact bytes of the
//! one supported `sdaemon.exe` build; the original files are not modified.
//! Bindings come from `ARCADE_SUPERCARS_KEYS` (`target:dik,...`, written by
//! the Rust launcher from controls.cfg). Controllers are read through XInput
//! (`xinput.rs`) with bindings from `ARCADE_PAD` (`target:mask,...`): the left
//! stick steers, triggers bound to gas/brake are analog, and Back + Start
//! together exit like Escape.
#![allow(non_snake_case)]

#[allow(dead_code)]
#[path = "../../arcade-core/src/pad.rs"]
mod pad;
#[path = "rt.rs"]
mod rt;
#[path = "xinput.rs"]
mod xinput;

use core::{mem::transmute, ptr};

// ---------------------------------------------------------------------------
// Win32 imports (raw-dylib: no import libraries or CRT are needed).

#[link(
    name = "kernel32",
    kind = "raw-dylib",
    import_name_type = "undecorated"
)]
unsafe extern "system" {
    fn VirtualProtect(addr: *mut u8, size: usize, protect: u32, old: *mut u32) -> i32;
    fn VirtualAlloc(addr: *mut u8, size: usize, kind: u32, protect: u32) -> *mut u8;
    fn FlushInstructionCache(process: isize, addr: *const u8, size: usize) -> i32;
    fn GetCurrentProcess() -> isize;
    fn LoadLibraryA(name: *const u8) -> isize;
    fn GetProcAddress(module: isize, name: *const u8) -> *const u8;
    fn GetSystemDirectoryA(buf: *mut u8, size: u32) -> u32;
    fn GetEnvironmentVariableA(name: *const u8, buf: *mut u8, size: u32) -> u32;
    fn CreateFileA(
        name: *const u8,
        access: u32,
        share: u32,
        security: *mut u8,
        disposition: u32,
        flags: u32,
        template: isize,
    ) -> isize;
    fn WriteFile(
        file: isize,
        buf: *const u8,
        len: u32,
        written: *mut u32,
        overlapped: *mut u8,
    ) -> i32;
    fn GetTickCount() -> u32;
    fn DisableThreadLibraryCalls(module: isize) -> i32;
    fn ExitProcess(code: u32) -> !;
}

// ---------------------------------------------------------------------------
// sdaemon.exe (SHA-256 0cddbdbf…95ba) addresses. Every code address is
// checked against its expected bytes before anything is patched.

/// Per-frame input poll (no arguments, cdecl). Starts `push ecx; call 0x401ab0`.
const POLL: usize = 0x004a_8e90;
const POLL_BYTES: [u8; 8] = [0x51, 0xe8, 0x1a, 0x8c, 0xf5, 0xff, 0x85, 0xc0];
/// Called by the poll's first instruction (RTIO "is active").
const RTIO_ACTIVE: usize = 0x0040_1ab0;
/// Input event sink used by the board driver: `sink(code, state)`, cdecl.
const SINK: usize = 0x004a_9510;
const SINK_BYTES: [u8; 9] = [0x8b, 0x44, 0x24, 0x04, 0xa9, 0x00, 0x00, 0x01, 0x00];
/// The `0x10040` (cabinet controls available) case in the input query at
/// 0x4a9a40. Menus only read adc1 when this query or a joystick is active;
/// otherwise they fall back to the developer keyboard that we suppress.
/// Patch this query case only: RTIO_ACTIVE itself must still report the real
/// board status so the original poll does not call the absent board driver.
const CABINET_QUERY: usize = 0x004a_9a6d;
const CABINET_QUERY_BYTES: [u8; 5] = [0xe9, 0x3e, 0x80, 0xf5, 0xff];
// xor eax,eax; inc eax; ret; nop (return true in the five-byte tail jump).
const CABINET_QUERY_PATCH: [u8; 5] = [0x33, 0xc0, 0x40, 0xc3, 0x90];
/// Raw `GetDeviceState` keyboard buffer, 256 bytes by DIK code; 0x80 = down.
const RAW_KEYS: usize = 0x0478_0108;
/// The game's per-key edge state derived from it (bit 0 down, 1 pressed, 2 released).
const KEYS: usize = 0x0477_f6f8;
/// Last key pressed this frame (read by developer shortcuts).
const LAST_KEY: usize = 0x0477_f6f0;
/// Analog channel table (u32 each); the game scales by analogmax (0xfff).
const ANALOG: usize = 0x0477_f808;
const ADC_STEER: usize = 0x20; // bipolar: 0 = full left, 0xfff = full right
const ADC_GAS: usize = 0x21; // 0..0xfff
const ADC_BRAKE: usize = 0x22; // 0..0xfff
const ANALOG_MAX: i32 = 0xfff;

/// Pseudo targets for the analog controls (real targets are RTIO codes 0x10000+).
const T_STEER_LEFT: u32 = 1;
const T_STEER_RIGHT: u32 = 2;
const T_GAS: u32 = 3;
const T_BRAKE: u32 = 4;

const DIK_ESCAPE: usize = 0x01;

/// Steering travel, in 1/4096ths of half the range per millisecond.
const STEER_RATE: i32 = 12; // full lock in ~0.33 s
const CENTER_RATE: i32 = 20;

const MAX_BINDINGS: usize = 96;
const MAX_BUTTONS: usize = 48;

struct State {
    bindings: [(u32, u8); MAX_BINDINGS],
    binding_count: usize,
    /// Controller (target, XInput mask) bindings.
    pad_bindings: [(u32, u32); MAX_BINDINGS],
    pad_count: usize,
    pads_seen: u32,
    /// Distinct button targets and whether they are currently reported held.
    buttons: [(u32, bool); MAX_BUTTONS],
    button_count: usize,
    trampoline: usize,
    steer: i32, // -4096..=4096
    last_tick: u32,
    log: isize,
    real_create: usize,
    /// Escape must be seen released before it can exit (stale state at boot).
    escape_armed: bool,
    /// Previous raw key state, for the diagnostic transition log.
    prev_raw: [bool; 256],
    /// Key transitions still to log (ARCADE_SUPERCARS_DEBUG=1 only).
    logged: u32,
}

static mut STATE: State = State {
    bindings: [(0, 0); MAX_BINDINGS],
    binding_count: 0,
    pad_bindings: [(0, 0); MAX_BINDINGS],
    pad_count: 0,
    pads_seen: 0,
    buttons: [(0, false); MAX_BUTTONS],
    button_count: 0,
    trampoline: 0,
    steer: 0,
    last_tick: 0,
    log: -1,
    real_create: 0,
    escape_armed: false,
    prev_raw: [false; 256],
    logged: 0,
};

fn state() -> &'static mut State {
    // Single game thread: DllMain runs before the game, the poll hook on its main thread.
    unsafe { &mut *ptr::addr_of_mut!(STATE) }
}

// ---------------------------------------------------------------------------
// Small freestanding helpers.

fn log(text: &[u8]) {
    let s = state();
    unsafe {
        if s.log == -1 {
            // GENERIC_WRITE, share read, OPEN_ALWAYS -> appended below via FILE_APPEND_DATA.
            s.log = CreateFileA(
                b"arcade-hook.log\0".as_ptr(),
                0x0004,
                1,
                ptr::null_mut(),
                4,
                0x80,
                0,
            );
        }
        if s.log != -1 {
            let mut written = 0;
            WriteFile(
                s.log,
                text.as_ptr(),
                text.len() as u32,
                &mut written,
                ptr::null_mut(),
            );
            WriteFile(s.log, b"\r\n".as_ptr(), 2, &mut written, ptr::null_mut());
        }
    }
}

fn log_number(prefix: &[u8], value: u32) {
    let mut buf = [0u8; 96];
    let n = prefix.len().min(80);
    buf[..n].copy_from_slice(&prefix[..n]);
    let mut digits = [0u8; 10];
    let mut d = 0;
    let mut v = value;
    loop {
        digits[d] = b'0' + (v % 10) as u8;
        d += 1;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    let mut len = n;
    while d > 0 {
        d -= 1;
        buf[len] = digits[d];
        len += 1;
    }
    log(&buf[..len]);
}

fn bytes_match(addr: usize, expected: &[u8]) -> bool {
    expected
        .iter()
        .enumerate()
        .all(|(i, b)| unsafe { ptr::read_volatile((addr + i) as *const u8) } == *b)
}

/// Parses `target:dik,target:dik` (decimal) into the binding table.
fn parse_bindings(text: &[u8]) {
    let s = state();
    let mut target: u32 = 0;
    let mut value: u32 = 0;
    let mut have_target = false;
    for &c in text.iter().chain(core::iter::once(&b',')) {
        match c {
            b'0'..=b'9' => value = value.saturating_mul(10).saturating_add(u32::from(c - b'0')),
            b':' => {
                target = value;
                value = 0;
                have_target = true;
            }
            b',' => {
                if have_target && value > 0 && value < 256 && s.binding_count < MAX_BINDINGS {
                    s.bindings[s.binding_count] = (target, value as u8);
                    s.binding_count += 1;
                }
                value = 0;
                have_target = false;
            }
            _ => {}
        }
    }
}

/// Distinct button targets from both keyboard and controller bindings.
fn collect_buttons() {
    let s = state();
    let targets = s.bindings[..s.binding_count]
        .iter()
        .map(|b| b.0)
        .chain(s.pad_bindings[..s.pad_count].iter().map(|b| b.0));
    for target in targets {
        if target >= 0x10000
            && !s.buttons[..s.button_count].iter().any(|b| b.0 == target)
            && s.button_count < MAX_BUTTONS
        {
            s.buttons[s.button_count] = (target, false);
            s.button_count += 1;
        }
    }
}

/// controls.cfg's default controller buttons for Super Cars.
const DEFAULT_PAD: &[u8] = b"1:4,2:8,3:131072,4:65536,65536:16,65536:4096,65537:16384,65538:32768,\
65539:8192,65540:128,65541:256,65542:512,65543:1,65544:2,65545:32";

const DEFAULT_BINDINGS: &[u8] =
    // start1 Enter/1, view1-3 C/V/B, tunes M, gears Q/W/E/R, coin1 5, coin2 6,
    // test T, service 9, volume PageUp/PageDown, steering Left/Right, gas Up, brake Down.
    b"65536:28,65536:2,65537:46,65538:47,65539:48,65540:50,65541:16,65542:17,65543:18,65544:19,\
65545:6,65546:7,65547:20,65548:10,65550:201,65551:209,1:203,2:205,3:200,4:208";

// ---------------------------------------------------------------------------
// The per-frame hook.

extern "C" fn poll_hook() {
    let s = state();
    unsafe {
        let original: extern "C" fn() = transmute(s.trampoline);
        original();
    }
    after_poll(s);
}

fn after_poll(s: &mut State) {
    let keys = KEYS as *mut u8;
    let raw = RAW_KEYS as *mut u8;
    let down = |dik: u8| unsafe { ptr::read_volatile(raw.add(usize::from(dik))) } & 0x80 != 0;
    for dik in 0..=255u8 {
        let now = down(dik);
        if now != s.prev_raw[usize::from(dik)] {
            s.prev_raw[usize::from(dik)] = now;
            if s.logged > 0 {
                s.logged -= 1;
                log_number(
                    if now {
                        b"key down dik="
                    } else {
                        b"key up dik="
                    },
                    u32::from(dik),
                );
            }
        }
    }
    let pad = xinput::read();
    let pads = xinput::connected();
    if pads != s.pads_seen {
        s.pads_seen = pads;
        log_number(b"XInput controllers connected: ", pads);
    }
    let pad_exit = pad.held(pad::BACK) && pad.held(pad::START);
    if !down(DIK_ESCAPE as u8) && !pad_exit {
        s.escape_armed = true;
    } else if s.escape_armed {
        log(b"Escape (or Back + Start) pressed: exiting");
        unsafe { ExitProcess(0) }
    }
    let pad_bindings = &s.pad_bindings[..s.pad_count];
    let held = |target: u32| {
        s.bindings[..s.binding_count]
            .iter()
            .any(|&(t, k)| t == target && down(k))
            || pad::target_held(&pad, pad_bindings, target)
    };

    // Buttons: post press/release edges exactly like the board driver does.
    let sink: extern "C" fn(u32, u32) = unsafe { transmute(SINK) };
    for i in 0..s.button_count {
        let (target, was) = s.buttons[i];
        let now = held(target);
        if now != was {
            sink(target, u32::from(now));
            s.buttons[i].1 = now;
        }
    }

    // Steering ramps like a wheel and recentres; pedals are on/off.
    let tick = unsafe { GetTickCount() };
    let elapsed = if s.last_tick == 0 {
        16
    } else {
        tick.wrapping_sub(s.last_tick).min(100)
    } as i32;
    s.last_tick = tick;
    let left = held(T_STEER_LEFT);
    let right = held(T_STEER_RIGHT);
    let goal = match (left, right) {
        (true, false) => -4096,
        (false, true) => 4096,
        _ => 0,
    };
    let rate = if goal == 0 { CENTER_RATE } else { STEER_RATE } * elapsed;
    let stick = pad::Pad::axis(pad.lx);
    if goal == 0 && stick != 0 {
        // The stick is an absolute wheel position.
        s.steer = stick;
    } else if s.steer < goal {
        s.steer = (s.steer + rate).min(goal);
    } else if s.steer > goal {
        s.steer = (s.steer - rate).max(goal);
    }
    let pedal = |target: u32| {
        let keys = s.bindings[..s.binding_count]
            .iter()
            .any(|&(t, k)| t == target && down(k));
        if keys {
            ANALOG_MAX
        } else {
            i32::from(pad::target_level(&pad, pad_bindings, target)) * ANALOG_MAX / 255
        }
    };
    let steer = (ANALOG_MAX + 1) / 2 + s.steer * (ANALOG_MAX / 2) / 4096;
    let analog = ANALOG as *mut i32;
    unsafe {
        ptr::write_volatile(analog.add(ADC_STEER), steer);
        ptr::write_volatile(analog.add(ADC_GAS), pedal(T_GAS));
        ptr::write_volatile(analog.add(ADC_BRAKE), pedal(T_BRAKE));
    }

    // Hide the keyboard from the game's developer shortcuts (camera/debug keys).
    // Clearing the raw buffer too means a failed GetDeviceState (window
    // unfocused) releases everything instead of repeating stale keys.
    for i in 0..256 {
        unsafe {
            ptr::write_volatile(keys.add(i), 0);
            ptr::write_volatile(raw.add(i), 0);
        }
    }
    unsafe { ptr::write_volatile(LAST_KEY as *mut u8, 0) };
}

// ---------------------------------------------------------------------------
// Installation.

unsafe fn install() {
    let s = state();
    if !bytes_match(POLL, &POLL_BYTES)
        || !bytes_match(SINK, &SINK_BYTES)
        || !bytes_match(CABINET_QUERY, &CABINET_QUERY_BYTES)
    {
        log(b"arcade-wine-hook: unsupported sdaemon.exe build; controls not installed");
        return;
    }
    let mut env = [0u8; 2048];
    let n = unsafe {
        GetEnvironmentVariableA(
            b"ARCADE_SUPERCARS_KEYS\0".as_ptr(),
            env.as_mut_ptr(),
            env.len() as u32,
        )
    } as usize;
    let mut flag = [0u8; 4];
    if unsafe {
        GetEnvironmentVariableA(b"ARCADE_SUPERCARS_DEBUG\0".as_ptr(), flag.as_mut_ptr(), 4)
    } == 1
        && flag[0] == b'1'
    {
        s.logged = 200;
    }
    if n > 0 && n < env.len() {
        parse_bindings(&env[..n]);
    } else {
        log(b"ARCADE_SUPERCARS_KEYS not set; using default bindings");
        parse_bindings(DEFAULT_BINDINGS);
    }
    let mut pad_env = [0u8; 1024];
    let n = unsafe {
        GetEnvironmentVariableA(b"ARCADE_PAD\0".as_ptr(), pad_env.as_mut_ptr(), pad_env.len() as u32)
    } as usize;
    // Set but empty means every controller button was unbound.
    let configured = n > 0 || unsafe { GetEnvironmentVariableA(b"ARCADE_PAD\0".as_ptr(), pad_env.as_mut_ptr(), 0) } == 1;
    let text = if n < pad_env.len() && configured { &pad_env[..n] } else { DEFAULT_PAD };
    s.pad_count = pad::parse_bindings(text, &mut s.pad_bindings);
    collect_buttons();

    // Trampoline: the two displaced instructions, then back into the poll.
    let t = unsafe { VirtualAlloc(ptr::null_mut(), 16, 0x3000, 0x40) };
    if t.is_null() {
        log(b"arcade-wine-hook: VirtualAlloc failed");
        return;
    }
    let tb = t as usize;
    let code: [u8; 11] = {
        let call = (RTIO_ACTIVE as i32)
            .wrapping_sub((tb + 6) as i32)
            .to_le_bytes();
        let back = ((POLL + 6) as i32)
            .wrapping_sub((tb + 11) as i32)
            .to_le_bytes();
        [
            0x51, 0xe8, call[0], call[1], call[2], call[3], 0xe9, back[0], back[1], back[2],
            back[3],
        ]
    };
    for (i, b) in code.iter().enumerate() {
        unsafe { ptr::write_volatile(t.add(i), *b) };
    }
    s.trampoline = tb;

    let jump = (poll_hook as extern "C" fn() as usize as i32)
        .wrapping_sub((POLL + 5) as i32)
        .to_le_bytes();
    let patch = [0xe9, jump[0], jump[1], jump[2], jump[3], 0x90];
    let mut old = 0;
    let mut query_old = 0;
    unsafe {
        if VirtualProtect(CABINET_QUERY as *mut u8, CABINET_QUERY_PATCH.len(), 0x40, &mut query_old) == 0 {
            log(b"arcade-wine-hook: cabinet query VirtualProtect failed");
            return;
        }
        if VirtualProtect(POLL as *mut u8, patch.len(), 0x40, &mut old) == 0 {
            VirtualProtect(CABINET_QUERY as *mut u8, CABINET_QUERY_PATCH.len(), query_old, &mut query_old);
            log(b"arcade-wine-hook: VirtualProtect failed");
            return;
        }
        for (i, b) in CABINET_QUERY_PATCH.iter().enumerate() {
            ptr::write_volatile((CABINET_QUERY + i) as *mut u8, *b);
        }
        for (i, b) in patch.iter().enumerate() {
            ptr::write_volatile((POLL + i) as *mut u8, *b);
        }
        VirtualProtect(POLL as *mut u8, patch.len(), old, &mut old);
        VirtualProtect(CABINET_QUERY as *mut u8, CABINET_QUERY_PATCH.len(), query_old, &mut query_old);
        FlushInstructionCache(GetCurrentProcess(), POLL as *const u8, patch.len());
        FlushInstructionCache(GetCurrentProcess(), CABINET_QUERY as *const u8, CABINET_QUERY_PATCH.len());
    }
    log_number(
        b"arcade-wine-hook: controls installed, bindings=",
        s.binding_count as u32,
    );
}

#[unsafe(no_mangle)]
pub extern "system" fn _DllMainCRTStartup(module: isize, reason: u32, _reserved: *mut u8) -> i32 {
    if reason == 1 {
        unsafe {
            DisableThreadLibraryCalls(module);
            install();
        }
    }
    1
}

// ---------------------------------------------------------------------------
// DirectInput forwarding.

type CreateFn = extern "system" fn(isize, u32, *const u8, *mut *mut u8, *mut u8) -> i32;

#[unsafe(no_mangle)]
pub extern "system" fn DirectInput8Create(
    instance: isize,
    version: u32,
    riid: *const u8,
    out: *mut *mut u8,
    outer: *mut u8,
) -> i32 {
    let s = state();
    if s.real_create == 0 {
        let mut path = [0u8; 300];
        let n = unsafe { GetSystemDirectoryA(path.as_mut_ptr(), 260) } as usize;
        let name = b"\\dinput8.dll\0";
        if n == 0 || n + name.len() > path.len() {
            return 0x8000_4005u32 as i32; // E_FAIL
        }
        path[n..n + name.len()].copy_from_slice(name);
        unsafe {
            let module = LoadLibraryA(path.as_ptr());
            if module != 0 {
                s.real_create = GetProcAddress(module, b"DirectInput8Create\0".as_ptr()) as usize;
            }
        }
        if s.real_create == 0 {
            log(b"arcade-wine-hook: system dinput8 unavailable");
            return 0x8000_4005u32 as i32;
        }
    }
    let real: CreateFn = unsafe { transmute(s.real_create) };
    real(instance, version, riid, out, outer)
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    log(b"arcade-wine-hook: internal error");
    unsafe { ExitProcess(3) }
}
