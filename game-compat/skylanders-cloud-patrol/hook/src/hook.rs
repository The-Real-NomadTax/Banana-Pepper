//! Replacement for the PQ Labs MultiTouch client (`PQMTClient.dll`) that
//! Skylanders Cloud Patrol uses for all pointer input.
//!
//! The original DLL connects to a PQ Labs touch-screen server on
//! 127.0.0.1:21303. Without one the game never learns the screen size and
//! every frame throws `DivideByZeroException` in `PqmtScreenGestures`, so
//! nothing can be touched. This adapter implements the same cdecl API with
//! no networking: the game window is the touch screen, and
//!
//! - the left mouse button touches at the cursor,
//! - the bound "touch" keys and controller buttons touch at the cursor,
//! - the controller's movement axes move the cursor.
//!
//! Callbacks run on the adapter's own thread, as they did on the original
//! client's receive thread. Escape (or Back + Start) exits like the other
//! Banana Pepper adapters.
//!
//! The same DLL is also staged as `KaboomComDll.dll`, the coin and ticket
//! board client (see `board.rs`). Each name loads as its own module, so the
//! two roles never share state.
#![allow(non_snake_case, unsafe_op_in_unsafe_fn, static_mut_refs)]
#[path = "board.rs"]
mod board;
#[allow(dead_code)]
#[path = "../../../../guest/win32/controllers.rs"]
mod controllers;
#[allow(dead_code)]
#[path = "../../../../guest/win32/dinput.rs"]
mod dinput;
#[allow(dead_code)]
#[path = "../../../../crates/banana-input/src/pad.rs"]
mod pad;
#[path = "../../../../guest/win32/rt.rs"]
mod rt;
#[allow(dead_code)]
#[path = "../../../../guest/win32/xinput.rs"]
mod xinput;
use core::{arch::asm, ptr};

#[link(
    name = "kernel32",
    kind = "raw-dylib",
    import_name_type = "undecorated"
)]
unsafe extern "system" {
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
    fn Sleep(ms: u32);
    fn GetModuleFileNameA(module: isize, name: *mut u8, size: u32) -> u32;
    fn CreateThread(
        security: *const u8,
        stack: usize,
        start: unsafe extern "system" fn(*mut u8) -> u32,
        parameter: *mut u8,
        flags: u32,
        id: *mut u32,
    ) -> isize;
}
#[link(name = "user32", kind = "raw-dylib", import_name_type = "undecorated")]
unsafe extern "system" {
    fn GetAsyncKeyState(key: i32) -> i16;
    fn GetForegroundWindow() -> isize;
    fn GetWindowThreadProcessId(window: isize, pid: *mut u32) -> u32;
    fn MapVirtualKeyA(code: u32, kind: u32) -> u32;
    fn GetCursorPos(point: *mut Point) -> i32;
    fn SetCursorPos(x: i32, y: i32) -> i32;
    fn ScreenToClient(window: isize, point: *mut Point) -> i32;
    fn ClientToScreen(window: isize, point: *mut Point) -> i32;
    fn GetClientRect(window: isize, rect: *mut Rect) -> i32;
    fn IsIconic(window: isize) -> i32;
    fn IsWindowVisible(window: isize) -> i32;
    fn EnumWindows(
        callback: unsafe extern "system" fn(isize, isize) -> i32,
        parameter: isize,
    ) -> i32;
    fn GetSystemMetrics(index: i32) -> i32;
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct Point {
    x: i32,
    y: i32,
}
#[repr(C)]
#[derive(Default, Clone, Copy)]
struct Rect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

/// `PQ_SDK_MultiTouch::TouchPoint` (16 bytes, the stride the original
/// client passes to the point-frame callback).
#[repr(C)]
#[derive(Default, Clone, Copy)]
struct TouchPoint {
    point_event: u16,
    id: u16,
    x: i32,
    y: i32,
    dx: u16,
    dy: u16,
}
const TP_DOWN: u16 = 0;
const TP_MOVE: u16 = 1;
const TP_UP: u16 = 2;
/// Contact size reported for every touch, in screen pixels.
const CONTACT: u16 = 12;
/// The single contact's id. The game drops ids at or above its finger
/// count (6), so the mouse is always contact 0, like one finger on the
/// cabinet's screen.
const CONTACT_ID: u16 = 0;

/// Action target of "Touch / shoot" in `controls.rs`.
const TOUCH: u32 = 1;

/// (callback, call_back_object) pairs set by the game.
static mut ON_FRAME: (usize, usize) = (0, 0);
static mut ON_GESTURE: (usize, usize) = (0, 0);
static mut ON_BREAK: (usize, usize) = (0, 0);
static mut ON_ERROR: (usize, usize) = (0, 0);
static mut ON_DEVICE: (usize, usize) = (0, 0);
static mut ON_RESOLUTION: (usize, usize) = (0, 0);
static mut RESOLUTION_PENDING: bool = false;
static mut CONNECTED: bool = false;
static mut THREAD: bool = false;

static mut LOG: isize = 0;
static mut KEYS: [(u32, i32); 64] = [(0, 0); 64];
static mut KEY_COUNT: usize = 0;
static mut KEYS_CONFIGURED: bool = false;
static mut PAD_BINDINGS: [(u32, u32); 64] = [(0, 0); 64];
static mut PAD_COUNT: usize = 0;
static mut MOVES: pad::Movement = pad::Movement::STICKS;
/// controls.cfg's default controller buttons: touch = A or RT, coin = Back.
const DEFAULT_PAD: &[u8] = b"1:4096,1:131072,2:32";

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
unsafe fn log_number(prefix: &[u8], value: i32) {
    let mut bytes = [0u8; 96];
    let n = prefix.len().min(80);
    bytes[..n].copy_from_slice(&prefix[..n]);
    let mut digits = [0u8; 12];
    let mut d = 0;
    let mut v = value.unsigned_abs();
    loop {
        digits[d] = b'0' + (v % 10) as u8;
        d += 1;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    let mut at = n;
    if value < 0 {
        bytes[at] = b'-';
        at += 1;
    }
    for i in (0..d).rev() {
        bytes[at] = digits[i];
        at += 1;
    }
    log(&bytes[..at]);
}

/// Call a game callback with cdecl arguments. ESP is restored afterwards,
/// so a callee that pops its own arguments (stdcall) is also safe.
unsafe fn call(function: usize, args: &[u32]) {
    if function == 0 {
        return;
    }
    asm!(
        "mov edi, esp",
        "2:",
        "test edx, edx",
        "jz 3f",
        "dec edx",
        "push dword ptr [eax + edx*4]",
        "jmp 2b",
        "3:",
        "call ecx",
        "mov esp, edi",
        inout("eax") args.as_ptr() => _,
        inout("ecx") function => _,
        inout("edx") args.len() => _,
        out("edi") _,
        clobber_abi("C"),
    );
}

fn env<'a>(name: &[u8], buffer: &'a mut [u8]) -> Option<&'a [u8]> {
    let n =
        unsafe { GetEnvironmentVariableA(name.as_ptr(), buffer.as_mut_ptr(), buffer.len() as u32) }
            as usize;
    (n > 0 && n < buffer.len()).then(|| &buffer[..n])
}

/// `ARCADE_SKYLANDERS_KEYS`: `target:DIK scan code,...` from controls.cfg.
unsafe fn key_bindings() {
    let mut bytes = [0u8; 1024];
    let Some(text) = env(b"ARCADE_SKYLANDERS_KEYS\0", &mut bytes) else {
        return;
    };
    KEYS_CONFIGURED = true;
    let mut pairs = [(0u32, 0u32); 64];
    let count = pad::parse_bindings(text, &mut pairs);
    for &(target, code) in &pairs[..count] {
        let scan = if code & 128 != 0 {
            0xe000 | (code & 127)
        } else {
            code
        };
        KEYS[KEY_COUNT] = (target, MapVirtualKeyA(scan, 3) as i32);
        KEY_COUNT += 1;
    }
}

unsafe fn pad_bindings() {
    let mut bytes = [0u8; 1024];
    // Set but empty means "no controller buttons"; unset uses the defaults.
    let set = GetEnvironmentVariableA(b"ARCADE_PAD\0".as_ptr(), ptr::null_mut(), 0) > 0;
    let text = if set {
        env(b"ARCADE_PAD\0", &mut bytes).unwrap_or(&[])
    } else {
        DEFAULT_PAD
    };
    PAD_COUNT = pad::parse_bindings(text, &mut PAD_BINDINGS);
    let mut moves = [0u8; 256];
    MOVES = pad::Movement::parse(env(b"ARCADE_MOVE\0", &mut moves), pad::Movement::STICKS);
}

unsafe fn key_down(vk: i32) -> bool {
    GetAsyncKeyState(vk) < 0
}

unsafe fn action(target: u32, state: &pad::Pad) -> bool {
    let keys = if KEYS_CONFIGURED {
        KEYS[..KEY_COUNT]
            .iter()
            .any(|&(t, vk)| t == target && key_down(vk))
    } else {
        // controls.cfg defaults: Space touches, 5 inserts a coin.
        (target == TOUCH && key_down(0x20)) || (target == 2 && key_down(0x35))
    };
    keys || pad::target_held(state, &PAD_BINDINGS[..PAD_COUNT], target)
}

/// The game's visible top-level window with the largest client area.
static mut FOUND: (isize, i32) = (0, 0);
unsafe extern "system" fn each_window(window: isize, _parameter: isize) -> i32 {
    let mut pid = 0;
    GetWindowThreadProcessId(window, &mut pid);
    if pid == GetCurrentProcessId() && IsWindowVisible(window) != 0 {
        let mut rect = Rect::default();
        GetClientRect(window, &mut rect);
        let area = (rect.right - rect.left) * (rect.bottom - rect.top);
        if area > FOUND.1 {
            FOUND = (window, area);
        }
    }
    1
}
unsafe fn game_window() -> isize {
    let foreground = GetForegroundWindow();
    let mut pid = 0;
    GetWindowThreadProcessId(foreground, &mut pid);
    if foreground != 0 && pid == GetCurrentProcessId() {
        return foreground;
    }
    FOUND = (0, 0);
    EnumWindows(each_window, 0);
    FOUND.0
}
/// Whether the game window has the keyboard focus (input is ignored
/// otherwise, like the other adapters).
unsafe fn focused() -> bool {
    let foreground = GetForegroundWindow();
    let mut pid = 0;
    GetWindowThreadProcessId(foreground, &mut pid);
    foreground != 0 && pid == GetCurrentProcessId() && IsIconic(foreground) == 0
}
unsafe fn client_size(window: isize) -> (i32, i32) {
    let mut rect = Rect::default();
    if window == 0 || GetClientRect(window, &mut rect) == 0 {
        return (0, 0);
    }
    (rect.right - rect.left, rect.bottom - rect.top)
}

struct Touch {
    down: bool,
    x: i32,
    y: i32,
}
static mut TOUCH_STATE: Touch = Touch {
    down: false,
    x: 0,
    y: 0,
};
static mut FRAME: u32 = 0;
/// The touch-screen resolution reported to the game.
static mut REPORTED: (i32, i32) = (0, 0);
/// Sub-pixel cursor travel from the controller.
static mut CARRY: (i32, i32) = (0, 0);

unsafe fn send(event: u16, touch: &Touch) {
    let point = TouchPoint {
        point_event: event,
        id: CONTACT_ID,
        x: touch.x,
        y: touch.y,
        dx: CONTACT,
        dy: CONTACT,
    };
    FRAME = FRAME.wrapping_add(1);
    let (function, object) = ON_FRAME;
    call(
        function,
        &[
            FRAME,
            GetTickCount(),
            1,
            ptr::addr_of!(point) as u32,
            object as u32,
        ],
    );
}

/// Move the cursor by the controller's movement axes: full deflection
/// crosses the window's width in about 0.8 s.
unsafe fn steer_cursor(window: isize, state: &pad::Pad, size: (i32, i32), elapsed: i32) {
    let (mx, my) = state.movement(&MOVES);
    if mx == 0 && my == 0 {
        CARRY = (0, 0);
        return;
    }
    let speed = size.0.max(size.1) * 5 / 4;
    let step = |v: i32, carry: &mut i32| {
        let total = v * speed * elapsed + *carry;
        let scale = pad::AXIS_MAX * 1000;
        *carry = total % scale;
        total / scale
    };
    let dx = step(mx, &mut CARRY.0);
    let dy = step(my, &mut CARRY.1);
    let mut cursor = Point::default();
    GetCursorPos(&mut cursor);
    ScreenToClient(window, &mut cursor);
    let mut target = Point {
        x: (cursor.x + dx).clamp(0, size.0 - 1),
        y: (cursor.y + dy).clamp(0, size.1 - 1),
    };
    ClientToScreen(window, &mut target);
    SetCursorPos(target.x, target.y);
}

/// The game maps a touch to Unity screen space as
/// `(x, height - y) * (Screen.currentResolution / reported)`, with integer
/// division: reporting the desktop mode as the touch screen keeps the scale
/// at exactly 1, so touches are sent in window pixels (Y offset so that the
/// game's flip against the desktop height lands in the window).
unsafe fn desktop() -> (i32, i32) {
    (GetSystemMetrics(0), GetSystemMetrics(1))
}

unsafe fn poll(elapsed: i32) {
    let window = game_window();
    let size = client_size(window);
    let screen = desktop();
    // Re-sent when the display mode changes (e.g. a fullscreen mode switch).
    if ON_RESOLUTION.0 != 0
        && screen.0 > 0
        && screen.1 > 0
        && (RESOLUTION_PENDING || screen != REPORTED)
    {
        RESOLUTION_PENDING = false;
        REPORTED = screen;
        log_number(b"arcade-skylanders: touch screen width ", screen.0);
        log_number(b"arcade-skylanders: touch screen height ", screen.1);
        let (function, object) = ON_RESOLUTION;
        call(function, &[screen.0 as u32, screen.1 as u32, object as u32]);
    }
    let focused = window != 0 && focused();
    let state = if focused {
        controllers::read()
    } else {
        pad::Pad::default()
    };
    if focused && (key_down(0x1b) || (state.held(pad::BACK) && state.held(pad::START))) {
        log(b"arcade-skylanders: Escape (or Back + Start) exit");
        ExitProcess(0);
    }
    if focused && size.0 > 0 && size.1 > 0 {
        steer_cursor(window, &state, size, elapsed);
    }
    let mut cursor = Point::default();
    GetCursorPos(&mut cursor);
    ScreenToClient(window, &mut cursor);
    let inside = cursor.x >= 0 && cursor.y >= 0 && cursor.x < size.0 && cursor.y < size.1;
    let pressed = focused && (key_down(0x01) || action(TOUCH, &state));
    let touch = &mut *ptr::addr_of_mut!(TOUCH_STATE);
    // A touch starts inside the window and follows the cursor until release.
    let holding = pressed && (touch.down || inside);
    if REPORTED.0 <= 0 || REPORTED.1 <= 0 || size.0 <= 0 || size.1 <= 0 {
        return;
    }
    let x = cursor.x.clamp(0, size.0 - 1);
    let y = cursor.y.clamp(0, size.1 - 1) + (REPORTED.1 - size.1);
    if holding && !touch.down {
        touch.down = true;
        (touch.x, touch.y) = (x, y);
        send(TP_DOWN, touch);
    } else if holding && (touch.x, touch.y) != (x, y) {
        (touch.x, touch.y) = (x, y);
        send(TP_MOVE, touch);
    } else if !holding && touch.down {
        touch.down = false;
        send(TP_UP, touch);
    }
}

unsafe extern "system" fn worker(_parameter: *mut u8) -> u32 {
    log(b"arcade-skylanders: touch thread running");
    let mut last = GetTickCount();
    while CONNECTED {
        Sleep(8);
        let now = GetTickCount();
        let elapsed = now.wrapping_sub(last).min(50) as i32;
        last = now;
        poll(elapsed);
    }
    THREAD = false;
    0
}

unsafe fn start_thread() {
    CONNECTED = true;
    if !THREAD {
        THREAD = true;
        if CreateThread(ptr::null(), 0, worker, ptr::null_mut(), 0, ptr::null_mut()) == 0 {
            THREAD = false;
            log(b"arcade-skylanders: could not start the touch thread");
        }
    }
}

const PQMTE_SUCCESS: i32 = 0;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ConnectServer(_ip: *const u8, _port: i32) -> i32 {
    log(b"arcade-skylanders: ConnectServer (built-in touch screen, no network)");
    start_thread();
    PQMTE_SUCCESS
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn DisconnectServer() -> i32 {
    log(b"arcade-skylanders: DisconnectServer");
    CONNECTED = false;
    PQMTE_SUCCESS
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn GetServerResolution(function: usize, object: usize) -> i32 {
    ON_RESOLUTION = (function, object);
    RESOLUTION_PENDING = true;
    start_thread();
    PQMTE_SUCCESS
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn SendRequest(_request: *const u8) -> i32 {
    PQMTE_SUCCESS
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn SendThreshold(_move: i32) -> i32 {
    PQMTE_SUCCESS
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn SetRawDataResolution(_width: i32, _height: i32) -> i32 {
    PQMTE_SUCCESS
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn GetGestureName(_gesture: *const u8) -> *const u8 {
    b"PQMT gesture\0".as_ptr()
}
unsafe fn replace(slot: *mut (usize, usize), function: usize, object: usize) -> usize {
    let previous = (*slot).0;
    *slot = (function, object);
    previous
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn SetOnReceivePointFrame(function: usize, object: usize) -> usize {
    replace(ptr::addr_of_mut!(ON_FRAME), function, object)
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn SetOnReceiveGesture(function: usize, object: usize) -> usize {
    replace(ptr::addr_of_mut!(ON_GESTURE), function, object)
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn SetOnServerBreak(function: usize, object: usize) -> usize {
    replace(ptr::addr_of_mut!(ON_BREAK), function, object)
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn SetOnReceiveError(function: usize, object: usize) -> usize {
    replace(ptr::addr_of_mut!(ON_ERROR), function, object)
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn SetOnGetDeviceInfo(function: usize, object: usize) -> usize {
    replace(ptr::addr_of_mut!(ON_DEVICE), function, object)
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn _DllMainCRTStartup(
    module: isize,
    reason: u32,
    _reserved: *mut u8,
) -> i32 {
    if reason == 1 {
        DisableThreadLibraryCalls(module);
        // Loaded once as the touch client and once as the coin board.
        let mut name = [0u8; 260];
        let n = GetModuleFileNameA(module, name.as_mut_ptr(), name.len() as u32) as usize;
        let board = name[..n.min(name.len())]
            .windows(6)
            .any(|w| w.eq_ignore_ascii_case(b"kaboom"));
        let file: &[u8] = if board {
            b"arcade-skylanders-board.log\0"
        } else {
            b"arcade-skylanders.log\0"
        };
        LOG = CreateFileA(file.as_ptr(), 0x40000000, 3, ptr::null(), 2, 0, 0);
        log(if board {
            b"arcade-skylanders: Banana Pepper adapter loaded as the coin/ticket board (KaboomComDll.dll)"
        } else {
            b"arcade-skylanders: Banana Pepper touch adapter loaded (replaces PQMTClient.dll)"
        });
        key_bindings();
        pad_bindings();
    }
    1
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    unsafe { ExitProcess(3) }
}
