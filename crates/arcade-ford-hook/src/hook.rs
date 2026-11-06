#![allow(non_snake_case, unsafe_op_in_unsafe_fn)]
#[path = "../../arcade-core/src/ford_packet.rs"]
mod packet;
#[allow(dead_code)]
#[path = "../../arcade-core/src/pad.rs"]
mod pad;
#[path = "../../arcade-wine-hook/src/rt.rs"]
mod rt;
#[path = "../../arcade-wine-hook/src/xinput.rs"]
mod xinput;
use core::{mem::transmute, ptr};
#[link(
    name = "kernel32",
    kind = "raw-dylib",
    import_name_type = "undecorated"
)]
unsafe extern "system" {
    fn VirtualProtect(p: *mut u8, n: usize, flags: u32, old: *mut u32) -> i32;
    fn FlushInstructionCache(process: isize, p: *const u8, n: usize) -> i32;
    fn GetCurrentProcess() -> isize;
    fn GetCurrentProcessId() -> u32;
    fn GetEnvironmentVariableA(name: *const u8, value: *mut u8, size: u32) -> u32;
    fn DisableThreadLibraryCalls(module: isize) -> i32;
    fn ExitProcess(code: u32) -> !;
    fn CreateFileA(
        name: *const u8,
        access: u32,
        share: u32,
        security: *const u8,
        disposition: u32,
        flags: u32,
        template: isize,
    ) -> isize;
    fn WriteFile(
        handle: isize,
        data: *const u8,
        size: u32,
        written: *mut u32,
        overlapped: *mut u8,
    ) -> i32;
    fn GetTickCount() -> u32;
    fn AddVectoredExceptionHandler(
        first: u32,
        handler: unsafe extern "system" fn(*mut Exceptions) -> i32,
    ) -> *mut u8;
}
#[link(name = "user32", kind = "raw-dylib", import_name_type = "undecorated")]
unsafe extern "system" {
    fn GetAsyncKeyState(key: i32) -> i16;
    fn GetForegroundWindow() -> isize;
    fn GetWindowThreadProcessId(window: isize, pid: *mut u32) -> u32;
    fn MapVirtualKeyA(code: u32, kind: u32) -> u32;
}
static mut LOG: isize = 0;
static mut STEER: i32 = 128;
static mut LAST_TICK: u32 = 0;
static mut PREVIOUS: u32 = 0;
static mut PROBE: bool = false;
static mut START: u32 = 0;
static mut BINDINGS: [(u32, i32); 64] = [(0, 0); 64];
static mut BIND_COUNT: usize = 0;
static mut CONFIGURED: bool = false;
/// Controller (target, XInput mask) bindings from ARCADE_PAD.
static mut PAD_BINDINGS: [(u32, u32); 64] = [(0, 0); 64];
static mut PAD_COUNT: usize = 0;
static mut PAD: pad::Pad = pad::Pad {
    buttons: 0,
    lx: 0,
    ly: 0,
    rx: 0,
    ry: 0,
    lt: 0,
    rt: 0,
};
static mut PADS_SEEN: u32 = 0;
/// controls.cfg's default controller buttons for Ford.
const DEFAULT_PAD: &[u8] = b"1:4,2:8,3:131072,4:65536,5:16,5:4096,6:32768,7:512,8:256,9:32";
unsafe fn pad_bindings() {
    let mut bytes = [0u8; 1024];
    let n = GetEnvironmentVariableA(b"ARCADE_PAD\0".as_ptr(), bytes.as_mut_ptr(), bytes.len() as u32)
        as usize;
    let set = n > 0 || GetEnvironmentVariableA(b"ARCADE_PAD\0".as_ptr(), bytes.as_mut_ptr(), 0) == 1;
    let text = if set && n < bytes.len() { &bytes[..n] } else { DEFAULT_PAD };
    PAD_COUNT = pad::parse_bindings(text, &mut *ptr::addr_of_mut!(PAD_BINDINGS));
}
unsafe fn pad_bound() -> &'static [(u32, u32)] {
    core::slice::from_raw_parts(ptr::addr_of!(PAD_BINDINGS).cast::<(u32, u32)>(), PAD_COUNT)
}
/// Analog 0..=255 for a pedal: keys are full travel, triggers are analog.
unsafe fn pedal(target: u32, focused: bool) -> u8 {
    if key_action(target, focused) {
        255
    } else if focused {
        pad::target_level(&*ptr::addr_of!(PAD), pad_bound(), target)
    } else {
        0
    }
}
unsafe fn action(target: u32, focused: bool) -> bool {
    key_action(target, focused)
        || (focused && pad::target_held(&*ptr::addr_of!(PAD), pad_bound(), target))
}
unsafe fn bindings() {
    let mut bytes = [0u8; 1024];
    let n = GetEnvironmentVariableA(
        b"ARCADE_FORD_KEYS\0".as_ptr(),
        bytes.as_mut_ptr(),
        bytes.len() as u32,
    ) as usize;
    if n >= bytes.len() {
        return;
    }
    CONFIGURED = n > 0;
    let (mut target, mut code, mut reading_code) = (0u32, 0u32, false);
    for ch in bytes[..n].iter().copied().chain(core::iter::once(b',')) {
        if ch.is_ascii_digit() {
            if reading_code {
                code = code.saturating_mul(10) + (ch - b'0') as u32;
            } else {
                target = target.saturating_mul(10) + (ch - b'0') as u32;
            }
        } else if ch == b':' {
            reading_code = true;
        } else if ch == b',' {
            if reading_code && BIND_COUNT < 64 {
                let scan = if code & 128 != 0 {
                    0xe000 | (code & 127)
                } else {
                    code
                };
                BINDINGS[BIND_COUNT] = (target, MapVirtualKeyA(scan, 3) as i32);
                BIND_COUNT += 1;
            }
            (target, code, reading_code) = (0, 0, false);
        }
    }
}
unsafe fn key_action(target: u32, focused: bool) -> bool {
    if CONFIGURED {
        for i in 0..BIND_COUNT {
            if BINDINGS[i].0 == target && down(BINDINGS[i].1, focused) {
                return true;
            }
        }
        return false;
    }
    match target {
        1 => down(0x25, focused),
        2 => down(0x27, focused),
        3 => down(0x26, focused),
        4 => down(0x28, focused),
        5 => down(13, focused) || down(b'1' as i32, focused),
        6 => down(b'V' as i32, focused),
        7 => down(32, focused),
        8 => down(b'X' as i32, focused),
        9 => down(b'5' as i32, focused),
        _ => false,
    }
}
#[repr(C)]
struct Exceptions {
    record: *const u32,
    context: *const u32,
}
unsafe fn hex(value: u32) {
    let mut bytes = *b"00000000 ";
    for i in 0..8 {
        bytes[i] = b"0123456789abcdef"[((value >> ((7 - i) * 4)) & 15) as usize];
    }
    log(&bytes);
}
unsafe extern "system" fn exception(info: *mut Exceptions) -> i32 {
    if !info.is_null() {
        let e = &*info;
        if !e.record.is_null() && !e.context.is_null() {
            let code = *e.record;
            if code == 0xc0000005 || code == 0xc000001d || code == 0xc0000096 {
                log(b"arcade-ford: fault code, EIP, ESP, EAX, ECX, EDX:");
                hex(code);
                for offset in [46, 49, 44, 43, 42] {
                    hex(*e.context.add(offset));
                }
                ExitProcess(125);
            }
        }
    }
    0
}
unsafe fn log(message: &[u8]) {
    if LOG != 0 && LOG != -1 {
        let mut count = 0;
        WriteFile(
            LOG,
            message.as_ptr(),
            message.len() as u32,
            &mut count,
            ptr::null_mut(),
        );
        WriteFile(LOG, b"\r\n".as_ptr(), 2, &mut count, ptr::null_mut());
    }
}
unsafe fn patch(address: usize, expected: &[u8], target: usize) -> bool {
    if core::slice::from_raw_parts(address as *const u8, expected.len()) != expected {
        return false;
    }
    let mut old = 0;
    if VirtualProtect(address as *mut u8, 8, 0x40, &mut old) == 0 {
        return false;
    }
    let mut bytes = [0xb8, 0, 0, 0, 0, 0xff, 0xe0];
    bytes[1..5].copy_from_slice(&(target as u32).to_le_bytes());
    ptr::copy_nonoverlapping(bytes.as_ptr(), address as *mut u8, bytes.len());
    FlushInstructionCache(GetCurrentProcess(), address as *const u8, bytes.len());
    let mut ignored = 0;
    VirtualProtect(address as *mut u8, 8, old, &mut ignored);
    true
}
unsafe extern "C" fn initialized() -> i32 {
    log(b"arcade-ford: virtual cabinet initialized");
    1
}
unsafe fn down(key: i32, focused: bool) -> bool {
    focused && GetAsyncKeyState(key) < 0
}
unsafe extern "C" fn poll(packet: *mut u8) -> i32 {
    if packet.is_null() {
        return 0;
    }
    let mut pid = 0;
    GetWindowThreadProcessId(GetForegroundWindow(), &mut pid);
    let focused = pid == GetCurrentProcessId();
    let state = xinput::read();
    PAD = state;
    let pads = xinput::connected();
    if pads != PADS_SEEN {
        PADS_SEEN = pads;
        log(b"arcade-ford: XInput controller count changed:");
        hex(pads);
    }
    let pad_exit = focused && state.held(pad::BACK) && state.held(pad::START);
    if down(27, focused) || pad_exit {
        log(b"arcade-ford: Escape (or Back + Start) exit");
        ExitProcess(0);
    }
    let now = GetTickCount();
    let delta = now.wrapping_sub(LAST_TICK).min(50) as i32;
    LAST_TICK = now;
    let left = action(1, focused);
    let right = action(2, focused);
    let stick = if focused { pad::Pad::axis(state.lx) } else { 0 };
    if left == right && stick != 0 {
        // The stick is an absolute wheel position.
        STEER = (128 + stick * 127 / pad::AXIS_MAX).clamp(0, 255);
    } else if left != right {
        STEER = (STEER + if left { -delta } else { delta }).clamp(0, 255);
    } else if STEER < 128 {
        STEER = (STEER + delta * 2).min(128);
    } else {
        STEER = (STEER - delta * 2).max(128);
    }
    let elapsed = now.wrapping_sub(START);
    let start = action(5, focused) || (PROBE && elapsed > 3000 && elapsed % 5000 < 250);
    let gas_level = if PROBE && elapsed > 14000 { 255 } else { pedal(3, focused) };
    let gas = gas_level > 0;
    let brake_level = pedal(4, focused);
    let view = action(6, focused);
    let gear = action(7, focused);
    let start_mask = 4;
    let report = packet::Inputs {
        steering: STEER as u8,
        gas: gas_level,
        brake: brake_level,
        start,
        view,
        shift_up: gear,
        shift_down: action(8, focused),
    }
    .report();
    ptr::copy_nonoverlapping(report.as_ptr(), packet, 8);
    // Shell notifications are counters consumed by the game between frames.
    let test = false;
    let service = false;
    let coin = action(9, focused);
    let state = if test { 1 } else { 0 }
        | if service { 2 } else { 0 }
        | if coin { 4 } else { 0 }
        | if start { 8 } else { 0 }
        | if gas { 16 } else { 0 };
    let edges = state & !PREVIOUS;
    if edges & 1 != 0 {
        let p = 0x559d80 as *mut u32;
        *p = (*p).wrapping_add(1);
        log(b"arcade-ford: test");
    }
    if edges & 2 != 0 {
        let p = 0x559d84 as *mut u32;
        *p = (*p).wrapping_add(1);
    }
    if edges & 4 != 0 {
        let f: unsafe extern "C" fn(i32) = transmute(0x467910usize);
        f(1);
        log(b"arcade-ford: coin");
    }
    if edges & 8 != 0 {
        log(b"arcade-ford: start mask");
        hex(start_mask as u32);
    }
    if edges & 16 != 0 {
        log(b"arcade-ford: gas");
    }
    PREVIOUS = state;
    1
}
#[unsafe(no_mangle)]
pub extern "system" fn Out32(_port: i16, _value: i16) {}
#[unsafe(no_mangle)]
pub unsafe extern "system" fn _DllMainCRTStartup(
    module: isize,
    reason: u32,
    _reserved: *mut u8,
) -> i32 {
    if reason == 1 {
        DisableThreadLibraryCalls(module);
        LOG = CreateFileA(
            b"arcade-ford.log\0".as_ptr(),
            0x40000000,
            3,
            ptr::null(),
            2,
            0,
            0,
        );
        log(b"arcade-ford: own desktop adapter loaded; no physical port I/O");
        AddVectoredExceptionHandler(1, exception);
        let mut value = [0u8; 8];
        PROBE = GetEnvironmentVariableA(b"ARCADE_FORD_PROBE\0".as_ptr(), value.as_mut_ptr(), 8) > 0;
        START = GetTickCount();
        LAST_TICK = START;
        bindings();
        pad_bindings();
        // All sites are checked before applying either patch. The launcher also
        // verifies the complete executable SHA256 before staging this DLL.
        let init = &[0x83, 0xec, 0x18, 0x57, 0x33, 0xff];
        let read = &[0x83, 0x3d, 0xa0, 0x02, 0x6b, 0x00, 0x01];
        if core::slice::from_raw_parts(0x4de1f0 as *const u8, init.len()) != init
            || core::slice::from_raw_parts(0x4de160 as *const u8, read.len()) != read
        {
            log(b"arcade-ford: unsupported executable prologue");
            return 0;
        }
        if !patch(0x4de1f0, init, initialized as *const () as usize)
            || !patch(0x4de160, read, poll as *const () as usize)
        {
            log(b"arcade-ford: patch failed");
            return 0;
        }
        log(b"arcade-ford: cabinet API hooks installed");
    }
    1
}
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    unsafe { ExitProcess(3) }
}
