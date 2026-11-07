//! GLUT and OpenGL for Linux programs, on a Win32 window and WGL. GLUT's
//! contract is kept: the program's own keyboard, mouse, timer, idle,
//! reshape and display callbacks run from `glutMainLoop`, with window
//! coordinates and GLUT key codes. OpenGL calls go to `opengl32.dll`.
use super::*;
use windows_sys::Win32::{
    Foundation::*,
    Graphics::{Gdi::*, OpenGL::*},
    UI::{Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
};
#[path = "gl_words.rs"]
mod gl_words;

// GLUT constants used by programs.
const GLUT_LEFT_BUTTON: i32 = 0;
const GLUT_MIDDLE_BUTTON: i32 = 1;
const GLUT_RIGHT_BUTTON: i32 = 2;
const GLUT_DOWN: i32 = 0;
const GLUT_UP: i32 = 1;
const GLUT_ACTIVE_SHIFT: i32 = 1;
const GLUT_ACTIVE_CTRL: i32 = 2;
const GLUT_ACTIVE_ALT: i32 = 4;
const GLUT_CURSOR_INHERIT: i32 = 0x64;
const GLUT_CURSOR_NONE: i32 = 0x65;
const GLUT_CURSOR_CROSSHAIR: i32 = 0x09;
const GLUT_CURSOR_FULL_CROSSHAIR: i32 = 0x66;

/// One window: Water's GLUT has a single top-level window, like GLUT game mode.
struct Glut {
    window: HWND,
    dc: HDC,
    width: i32,
    height: i32,
    /// glutInitWindowSize, used when the window is created.
    initial: (i32, i32),
    display: usize,
    reshape: usize,
    keyboard: usize,
    keyboard_up: usize,
    special: usize,
    special_up: usize,
    mouse: usize,
    motion: usize,
    passive: usize,
    idle: usize,
    visibility: usize,
    /// (due, callback, value), fired from the main loop in due order.
    timers: Vec<(std::time::Instant, usize, i32)>,
    redisplay: bool,
    ignore_repeat: bool,
    cursor: i32,
    /// Keys down, by virtual key, with the GLUT key they were reported as,
    /// so the release reports the same key even if Shift changed meanwhile.
    held: Vec<(u16, Key)>,
    buttons: u32,
    /// Modifier state for glutGetModifiers inside input callbacks.
    modifiers: i32,
    pointer: (i32, i32),
    frame: u64,
    hook: Option<crate::extension::MessageHook>,
}
#[derive(Clone, Copy, PartialEq)]
enum Key {
    Ascii(u8),
    Special(i32),
}
static mut GLUT: Glut = Glut {
    window: ptr::null_mut(),
    dc: ptr::null_mut(),
    width: 0,
    height: 0,
    initial: (300, 300),
    display: 0,
    reshape: 0,
    keyboard: 0,
    keyboard_up: 0,
    special: 0,
    special_up: 0,
    mouse: 0,
    motion: 0,
    passive: 0,
    idle: 0,
    visibility: 0,
    timers: Vec::new(),
    redisplay: true,
    ignore_repeat: false,
    cursor: GLUT_CURSOR_INHERIT,
    held: Vec::new(),
    buttons: 0,
    modifiers: 0,
    pointer: (0, 0),
    frame: 0,
    hook: None,
};
static START: OnceLock<std::time::Instant> = OnceLock::new();

/// The GUI state. GLUT is single-threaded: only the program's main thread
/// (the guest fiber and its window procedure) reaches it.
#[allow(static_mut_refs)]
unsafe fn glut() -> &'static mut Glut {
    &mut *ptr::addr_of_mut!(GLUT)
}
unsafe fn call0(f: usize) {
    if f != 0 {
        let f: unsafe extern "C" fn() = std::mem::transmute(f);
        f();
    }
}
unsafe fn call1(f: usize, a: i32) {
    if f != 0 {
        let f: unsafe extern "C" fn(i32) = std::mem::transmute(f);
        f(a);
    }
}
unsafe fn call2(f: usize, a: i32, b: i32) {
    if f != 0 {
        let f: unsafe extern "C" fn(i32, i32) = std::mem::transmute(f);
        f(a, b);
    }
}
unsafe fn call4(f: usize, a: i32, b: i32, c: i32, d: i32) {
    if f != 0 {
        let f: unsafe extern "C" fn(i32, i32, i32, i32) = std::mem::transmute(f);
        f(a, b, c, d);
    }
}
unsafe fn call_key(f: usize, key: u8, x: i32, y: i32) {
    if f != 0 {
        let f: unsafe extern "C" fn(u8, i32, i32) = std::mem::transmute(f);
        f(key, x, y);
    }
}

fn modifiers() -> i32 {
    let down = |vk: VIRTUAL_KEY| unsafe { GetKeyState(vk as i32) } < 0;
    (if down(VK_SHIFT) { GLUT_ACTIVE_SHIFT } else { 0 })
        | (if down(VK_CONTROL) {
            GLUT_ACTIVE_CTRL
        } else {
            0
        })
        | (if down(VK_MENU) { GLUT_ACTIVE_ALT } else { 0 })
}

/// The GLUT key a virtual key produces now: ASCII (with Shift, Caps Lock
/// and Ctrl applied, Delete as 127) or a GLUT special key.
unsafe fn key_for(vk: u16, scan: u32) -> Option<Key> {
    let special = match vk {
        0x70..=0x7b => Some(i32::from(vk - 0x70 + 1)), // F1..F12
        0x25 => Some(100),                             // left
        0x26 => Some(101),                             // up
        0x27 => Some(102),                             // right
        0x28 => Some(103),                             // down
        0x21 => Some(104),                             // page up
        0x22 => Some(105),                             // page down
        0x24 => Some(106),                             // home
        0x23 => Some(107),                             // end
        0x2d => Some(108),                             // insert
        _ => None,
    };
    if let Some(code) = special {
        return Some(Key::Special(code));
    }
    if vk == 0x2e {
        return Some(Key::Ascii(127));
    }
    let mut state = [0u8; 256];
    GetKeyboardState(state.as_mut_ptr());
    let mut out = 0u16;
    (ToAscii(u32::from(vk), scan, state.as_ptr(), &mut out, 0) == 1)
        .then_some(Key::Ascii(out as u8))
}

unsafe fn key_event(key: Key, down: bool) {
    let g = glut();
    let (x, y) = g.pointer;
    g.modifiers = modifiers();
    match (key, down) {
        (Key::Ascii(k), true) => call_key(g.keyboard, k, x, y),
        (Key::Ascii(k), false) => call_key(g.keyboard_up, k, x, y),
        (Key::Special(k), true) => call4(g.special, k, x, y, 0),
        (Key::Special(k), false) => call4(g.special_up, k, x, y, 0),
    }
}

/// Release every held key and button (focus lost): the program never sees
/// a key stuck down because its release went to another window.
unsafe fn release_all() {
    let g = glut();
    for (_, key) in std::mem::take(&mut g.held) {
        key_event(key, false);
    }
    let (x, y) = g.pointer;
    for button in 0..5 {
        if glut().buttons & (1 << button) != 0 {
            glut().buttons &= !(1 << button);
            call4(glut().mouse, button, GLUT_UP, x, y);
        }
    }
    if glut().buttons == 0 {
        ReleaseCapture();
    }
}

unsafe fn button_event(button: i32, down: bool, l: LPARAM) {
    let (x, y) = (l as i16 as i32, (l >> 16) as i16 as i32);
    let g = glut();
    g.pointer = (x, y);
    g.modifiers = modifiers();
    if down {
        g.buttons |= 1 << button;
        SetCapture(g.window);
    } else {
        g.buttons &= !(1 << button);
        if g.buttons == 0 {
            ReleaseCapture();
        }
    }
    call4(
        g.mouse,
        button,
        if down { GLUT_DOWN } else { GLUT_UP },
        x,
        y,
    );
}

unsafe extern "system" fn wndproc(window: HWND, message: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if let Some(hook) = glut().hook {
        if hook(window as isize, message, w, l) != 0 {
            return 0;
        }
    }
    match message {
        // GLUT exits when its window is closed.
        WM_CLOSE => {
            eprintln!("[water] window closed");
            ExitProcess(0);
        }
        WM_KEYDOWN | WM_SYSKEYDOWN | WM_KEYUP | WM_SYSKEYUP => {
            let vk = w as u16;
            let down = message == WM_KEYDOWN || message == WM_SYSKEYDOWN;
            let repeat = down && l & (1 << 30) != 0;
            let g = glut();
            if down {
                if !(repeat && g.ignore_repeat) {
                    if let Some(key) = key_for(vk, (l as u32 >> 16) & 0xff) {
                        if !repeat {
                            g.held.retain(|(held, _)| *held != vk);
                            g.held.push((vk, key));
                        }
                        key_event(key, true);
                    }
                }
            } else if let Some(i) = g.held.iter().position(|(held, _)| *held == vk) {
                let (_, key) = g.held.remove(i);
                key_event(key, false);
            }
            // Alt+F4 and the system menu keep working.
            if message == WM_SYSKEYDOWN || message == WM_SYSKEYUP {
                return DefWindowProcA(window, message, w, l);
            }
            return 0;
        }
        WM_KILLFOCUS => release_all(),
        WM_SETCURSOR if (l & 0xffff) as u32 == HTCLIENT => {
            match glut().cursor {
                GLUT_CURSOR_NONE => {
                    SetCursor(ptr::null_mut());
                }
                GLUT_CURSOR_CROSSHAIR | GLUT_CURSOR_FULL_CROSSHAIR => {
                    SetCursor(LoadCursorW(ptr::null_mut(), IDC_CROSS));
                }
                _ => {
                    SetCursor(LoadCursorW(ptr::null_mut(), IDC_ARROW));
                }
            }
            return 1;
        }
        WM_LBUTTONDOWN | WM_LBUTTONUP => {
            button_event(GLUT_LEFT_BUTTON, message == WM_LBUTTONDOWN, l)
        }
        WM_MBUTTONDOWN | WM_MBUTTONUP => {
            button_event(GLUT_MIDDLE_BUTTON, message == WM_MBUTTONDOWN, l)
        }
        WM_RBUTTONDOWN | WM_RBUTTONUP => {
            button_event(GLUT_RIGHT_BUTTON, message == WM_RBUTTONDOWN, l)
        }
        // freeglut reports the wheel as buttons 3 (up) and 4 (down).
        WM_MOUSEWHEEL => {
            let mut point = POINT {
                x: l as i16 as i32,
                y: (l >> 16) as i16 as i32,
            };
            ScreenToClient(window, &mut point);
            let button = if (w >> 16) as i16 > 0 { 3 } else { 4 };
            let g = glut();
            g.modifiers = modifiers();
            call4(g.mouse, button, GLUT_DOWN, point.x, point.y);
            call4(g.mouse, button, GLUT_UP, point.x, point.y);
            return 0;
        }
        WM_MOUSEMOVE => {
            let (x, y) = (l as i16 as i32, (l >> 16) as i16 as i32);
            let g = glut();
            g.pointer = (x, y);
            call2(if g.buttons != 0 { g.motion } else { g.passive }, x, y);
            return 0;
        }
        WM_PAINT => {
            glut().redisplay = true;
            ValidateRect(window, ptr::null());
            return 0;
        }
        WM_SIZE => {
            let g = glut();
            g.width = (l as u32 & 65535) as i32;
            g.height = ((l as u32 >> 16) & 65535) as i32;
            if g.width > 0 && g.height > 0 {
                call2(g.reshape, g.width, g.height);
                g.redisplay = true;
            }
        }
        _ => {}
    }
    DefWindowProcA(window, message, w, l)
}

unsafe extern "C" fn create(title: *const i8) -> i32 {
    let g = glut();
    if !g.window.is_null() {
        return 1;
    }
    (g.width, g.height) = g.initial;
    let class = WNDCLASSA {
        style: CS_OWNDC,
        lpfnWndProc: Some(wndproc),
        hInstance: GetModuleHandleA(ptr::null()),
        lpszClassName: c"BananaPepperWater".as_ptr().cast(),
        hCursor: LoadCursorW(ptr::null_mut(), IDC_ARROW),
        ..std::mem::zeroed()
    };
    RegisterClassA(&class);
    let mut rect = RECT {
        left: 0,
        top: 0,
        right: g.width,
        bottom: g.height,
    };
    AdjustWindowRect(&mut rect, WS_OVERLAPPEDWINDOW, 0);
    let title = if title.is_null() {
        c"Water".as_ptr()
    } else {
        title
    };
    g.window = CreateWindowExA(
        0,
        class.lpszClassName,
        title.cast(),
        WS_OVERLAPPEDWINDOW | WS_VISIBLE,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        rect.right - rect.left,
        rect.bottom - rect.top,
        ptr::null_mut(),
        ptr::null_mut(),
        class.hInstance,
        ptr::null(),
    );
    if g.window.is_null() {
        eprintln!(
            "[water] CreateWindow failed: {}",
            std::io::Error::last_os_error()
        );
        ExitProcess(125);
    }
    g.dc = GetDC(g.window);
    let format = PIXELFORMATDESCRIPTOR {
        nSize: std::mem::size_of::<PIXELFORMATDESCRIPTOR>() as u16,
        nVersion: 1,
        dwFlags: PFD_DRAW_TO_WINDOW | PFD_SUPPORT_OPENGL | PFD_DOUBLEBUFFER,
        iPixelType: PFD_TYPE_RGBA,
        cColorBits: 24,
        cDepthBits: 24,
        cStencilBits: 8,
        iLayerType: PFD_MAIN_PLANE as u8,
        ..std::mem::zeroed()
    };
    let mut index = ChoosePixelFormat(g.dc, &format);
    if std::env::var("ARCADE_RENDERER").is_ok_and(|s| s == "software") {
        let count = DescribePixelFormat(
            g.dc,
            1,
            std::mem::size_of::<PIXELFORMATDESCRIPTOR>() as u32,
            ptr::null_mut(),
        );
        index = 0;
        for candidate in 1..=count {
            let mut p: PIXELFORMATDESCRIPTOR = std::mem::zeroed();
            DescribePixelFormat(
                g.dc,
                candidate,
                std::mem::size_of::<PIXELFORMATDESCRIPTOR>() as u32,
                &mut p,
            );
            if p.dwFlags & PFD_GENERIC_FORMAT != 0
                && p.dwFlags & (PFD_SUPPORT_OPENGL | PFD_DRAW_TO_WINDOW | PFD_DOUBLEBUFFER)
                    == PFD_SUPPORT_OPENGL | PFD_DRAW_TO_WINDOW | PFD_DOUBLEBUFFER
                && p.cColorBits >= 24
                && p.cDepthBits >= 24
                && p.cStencilBits >= 8
            {
                index = candidate;
                break;
            }
        }
    }
    if index == 0 || SetPixelFormat(g.dc, index, &format) == 0 {
        eprintln!("[water] pixel format failed");
        ExitProcess(125);
    }
    let context = wglCreateContext(g.dc);
    if context.is_null() || wglMakeCurrent(g.dc, context) == 0 {
        eprintln!("[water] OpenGL context failed");
        ExitProcess(125);
    }
    if let Some(address) = wglGetProcAddress(c"wglSwapIntervalEXT".as_ptr().cast()) {
        let interval: unsafe extern "system" fn(i32) -> i32 = std::mem::transmute(address);
        interval(if std::env::var("ARCADE_VSYNC").is_ok_and(|s| s == "off") {
            0
        } else {
            1
        });
    }
    eprintln!(
        "[water] OpenGL renderer: {}",
        text(glGetString(GL_RENDERER).cast())
    );
    eprintln!("[water] OpenGL window {}x{}", g.width, g.height);
    g.redisplay = true;
    call1(g.visibility, 1); // GLUT_VISIBLE
    1
}

/// Borderless and covering the window's monitor (glutFullScreen).
unsafe extern "C" fn full_screen() {
    let window = glut().window;
    if window.is_null() {
        return;
    }
    let monitor = MonitorFromWindow(window, MONITOR_DEFAULTTONEAREST);
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..std::mem::zeroed()
    };
    if GetMonitorInfoA(monitor, &mut info) != 0 {
        SetWindowLongA(window, GWL_STYLE, (WS_POPUP | WS_VISIBLE) as i32);
        let r = info.rcMonitor;
        SetWindowPos(
            window,
            ptr::null_mut(),
            r.left,
            r.top,
            r.right - r.left,
            r.bottom - r.top,
            SWP_FRAMECHANGED | SWP_SHOWWINDOW,
        );
    }
}
unsafe extern "C" fn destroy(_window: i32) {
    let g = glut();
    if !g.window.is_null() {
        DestroyWindow(g.window);
        g.window = ptr::null_mut();
        g.dc = ptr::null_mut();
    }
}
/// Game mode: a fullscreen window of the requested (or current) size.
unsafe extern "C" fn enter_game() -> i32 {
    create(c"Water".as_ptr());
    full_screen();
    1
}
unsafe extern "C" fn game_mode_string(mode: *const i8) {
    // "WxH[:bpp][@hz]"
    if mode.is_null() {
        return;
    }
    let mode = text(mode);
    let size = mode.split([':', '@']).next().unwrap_or("");
    if let Some((w, h)) = size.split_once('x') {
        if let (Ok(w), Ok(h)) = (w.parse::<i32>(), h.parse::<i32>()) {
            glut().initial = (w.clamp(1, 16384), h.clamp(1, 16384));
        }
    }
}
unsafe extern "C" fn game_mode_get(what: i32) -> i32 {
    let g = glut();
    match what {
        0 => i32::from(!g.window.is_null()), // GLUT_GAME_MODE_ACTIVE
        1 => 1,                              // GLUT_GAME_MODE_POSSIBLE
        2 => g.initial.0,                    // GLUT_GAME_MODE_WIDTH
        3 => g.initial.1,                    // GLUT_GAME_MODE_HEIGHT
        4 => 32,                             // GLUT_GAME_MODE_PIXEL_DEPTH
        5 => 60,                             // GLUT_GAME_MODE_REFRESH_RATE
        _ => 0,
    }
}
unsafe extern "C" fn init(_argc: usize, _argv: usize) {
    START.get_or_init(std::time::Instant::now);
}
unsafe extern "C" fn window_size(width: i32, height: i32) {
    glut().initial = (width.clamp(1, 16384), height.clamp(1, 16384));
}
unsafe extern "C" fn reshape_window(width: i32, height: i32) {
    let window = glut().window;
    if window.is_null() {
        return window_size(width, height);
    }
    let mut rect = RECT {
        left: 0,
        top: 0,
        right: width,
        bottom: height,
    };
    AdjustWindowRect(&mut rect, GetWindowLongA(window, GWL_STYLE) as u32, 0);
    SetWindowPos(
        window,
        ptr::null_mut(),
        0,
        0,
        rect.right - rect.left,
        rect.bottom - rect.top,
        SWP_NOMOVE | SWP_NOZORDER,
    );
}
unsafe extern "C" fn set_title(title: *const i8) {
    if !glut().window.is_null() && !title.is_null() {
        SetWindowTextA(glut().window, title.cast());
    }
}
unsafe extern "C" fn set_cursor(cursor: i32) {
    glut().cursor = cursor;
    let mut point = POINT { x: 0, y: 0 };
    GetCursorPos(&mut point);
    SetCursorPos(point.x, point.y); // re-evaluates WM_SETCURSOR
}
unsafe extern "C" fn warp_pointer(x: i32, y: i32) {
    let mut point = POINT { x, y };
    ClientToScreen(glut().window, &mut point);
    SetCursorPos(point.x, point.y);
}
unsafe extern "C" fn get(what: i32) -> i32 {
    let g = glut();
    let mut rect: RECT = std::mem::zeroed();
    match what {
        100 | 101 => {
            // GLUT_WINDOW_X / _Y: the client area's screen position.
            let mut point = POINT { x: 0, y: 0 };
            ClientToScreen(g.window, &mut point);
            if what == 100 { point.x } else { point.y }
        }
        102 => g.width,                       // GLUT_WINDOW_WIDTH
        103 => g.height,                      // GLUT_WINDOW_HEIGHT
        104 => 8,                             // GLUT_WINDOW_BUFFER_SIZE
        105 => 8,                             // GLUT_WINDOW_STENCIL_SIZE
        106 => 24,                            // GLUT_WINDOW_DEPTH_SIZE
        107..=110 => 8,                       // GLUT_WINDOW_RED/GREEN/BLUE/ALPHA_SIZE
        115 | 116 => 1,                       // GLUT_WINDOW_DOUBLEBUFFER / _RGBA
        200 => GetSystemMetrics(SM_CXSCREEN), // GLUT_SCREEN_WIDTH
        201 => GetSystemMetrics(SM_CYSCREEN), // GLUT_SCREEN_HEIGHT
        400 => 1,                             // GLUT_DISPLAY_MODE_POSSIBLE
        500 | 501 => {
            // GLUT_INIT_WINDOW_X / _Y
            GetWindowRect(g.window, &mut rect);
            if what == 500 { rect.left } else { rect.top }
        }
        502 => g.initial.0, // GLUT_INIT_WINDOW_WIDTH
        503 => g.initial.1, // GLUT_INIT_WINDOW_HEIGHT
        700 => START
            .get_or_init(std::time::Instant::now)
            .elapsed()
            .as_millis()
            .min(i32::MAX as u128) as i32, // GLUT_ELAPSED_TIME
        _ => 0,
    }
}
unsafe extern "C" fn get_modifiers() -> i32 {
    glut().modifiers
}
unsafe extern "C" fn ignore_key_repeat(ignore: i32) {
    glut().ignore_repeat = ignore != 0;
}
unsafe extern "C" fn set_key_repeat(mode: i32) {
    glut().ignore_repeat = mode == 0; // GLUT_KEY_REPEAT_OFF
}
unsafe extern "C" fn post_redisplay() {
    glut().redisplay = true;
}
unsafe extern "C" fn ignored(_value: usize) {}
unsafe extern "C" fn ignored2(_a: usize, _b: usize) {}
unsafe extern "C" fn none() {}

macro_rules! register {
    ($($name:ident => $field:ident),* $(,)?) => {$(
        unsafe extern "C" fn $name(callback: usize) {
            glut().$field = callback;
        }
    )*};
}
register!(
    display_func => display,
    keyboard_func => keyboard,
    keyboard_up_func => keyboard_up,
    special_func => special,
    special_up_func => special_up,
    mouse_func => mouse,
    motion_func => motion,
    passive_func => passive,
    idle_func => idle,
    visibility_func => visibility,
);
unsafe extern "C" fn reshape_func(callback: usize) {
    let g = glut();
    g.reshape = callback;
    if !g.window.is_null() {
        call2(callback, g.width, g.height);
    }
}
unsafe extern "C" fn timer_func(milliseconds: u32, callback: usize, value: i32) {
    let due = std::time::Instant::now() + std::time::Duration::from_millis(u64::from(milliseconds));
    let timers = &mut glut().timers;
    let at = timers.partition_point(|t| t.0 <= due);
    timers.insert(at, (due, callback, value));
}

unsafe extern "C" fn main_loop() -> ! {
    eprintln!("[water] entering GLUT main loop");
    let mut message: MSG = std::mem::zeroed();
    loop {
        while PeekMessageA(&mut message, ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
            if message.message == WM_QUIT {
                ExitProcess(0);
            }
            TranslateMessage(&message);
            DispatchMessageA(&message);
        }
        // Timers registered by a callback run on a later pass.
        let now = std::time::Instant::now();
        let due = glut().timers.partition_point(|t| t.0 <= now);
        for (_, callback, value) in glut().timers.drain(..due).collect::<Vec<_>>() {
            call1(callback, value);
        }
        let g = glut();
        let idle = g.idle;
        call0(idle);
        let g = glut();
        let visible = !g.window.is_null() && IsIconic(g.window) == 0 && g.width > 0;
        if g.redisplay && g.display != 0 && visible {
            g.redisplay = false;
            call0(g.display);
        } else if idle == 0 {
            // Nothing to do until a message or the next timer.
            let wait = glut()
                .timers
                .first()
                .map(|t| t.0.saturating_duration_since(std::time::Instant::now()))
                .unwrap_or(std::time::Duration::from_millis(50))
                .as_millis()
                .min(50) as u32;
            MsgWaitForMultipleObjects(0, ptr::null(), 0, wait, QS_ALLINPUT);
        }
    }
}
unsafe extern "C" fn swap() {
    let g = glut();
    g.frame += 1;
    if g.frame == 1 || g.frame % 600 == 0 {
        eprintln!("[water] rendered frame {}", g.frame);
    }
    if std::env::var_os("ARCADE_NATIVE_PROBE").is_some() && g.frame % 120 == 0 {
        capture("screen.png");
    }
    if std::env::var_os("ARCADE_NATIVE_CAPTURE_ALL").is_some() && g.frame % 60 == 0 {
        capture(&format!(
            "frame-{:03}.png",
            START
                .get_or_init(std::time::Instant::now)
                .elapsed()
                .as_secs()
        ));
    }
    SwapBuffers(glut().dc);
}
unsafe fn capture(name: &str) {
    let (width, height) = (glut().width, glut().height);
    if banana_content::path(name).is_err() || width <= 0 || height <= 0 {
        return;
    }
    let mut pixels = vec![0u8; width as usize * height as usize * 3];
    glPixelStorei(GL_PACK_ALIGNMENT, 1);
    glReadPixels(
        0,
        0,
        width,
        height,
        GL_RGB,
        GL_UNSIGNED_BYTE,
        pixels.as_mut_ptr().cast(),
    );
    if let Some(buffer) = image::RgbImage::from_raw(width as u32, height as u32, pixels) {
        let buffer = image::imageops::flip_vertical(&buffer);
        if let Err(e) = buffer.save(WORK.get().unwrap().join(name)) {
            eprintln!("[water] capture failed: {e}");
        }
    }
}
unsafe extern "C" fn sync(counter: *mut u32) -> i32 {
    if !counter.is_null() {
        *counter = windows_sys::Win32::Media::timeGetTime() / 16;
    }
    0
}

pub unsafe fn resolve(name: &str) -> Result<Option<usize>, String> {
    let address = match name {
        "glutInit" => init as *const () as usize,
        "glutInitWindowSize" => window_size as *const () as usize,
        "glutInitWindowPosition" => ignored2 as *const () as usize,
        "glutInitDisplayMode" | "glutSetWindow" => ignored as *const () as usize,
        "glutGameModeString" => game_mode_string as *const () as usize,
        "glutGameModeGet" => game_mode_get as *const () as usize,
        "glutCreateWindow" => create as *const () as usize,
        "glutDestroyWindow" => destroy as *const () as usize,
        "glutEnterGameMode" => enter_game as *const () as usize,
        "glutLeaveGameMode" => none as *const () as usize,
        "glutFullScreen" => full_screen as *const () as usize,
        "glutReshapeWindow" => reshape_window as *const () as usize,
        "glutSetWindowTitle" => set_title as *const () as usize,
        "glutSetCursor" => set_cursor as *const () as usize,
        "glutWarpPointer" => warp_pointer as *const () as usize,
        "glutGet" => get as *const () as usize,
        "glutGetModifiers" => get_modifiers as *const () as usize,
        "glutIgnoreKeyRepeat" => ignore_key_repeat as *const () as usize,
        "glutSetKeyRepeat" => set_key_repeat as *const () as usize,
        "glutDisplayFunc" => display_func as *const () as usize,
        "glutReshapeFunc" => reshape_func as *const () as usize,
        "glutKeyboardFunc" => keyboard_func as *const () as usize,
        "glutKeyboardUpFunc" => keyboard_up_func as *const () as usize,
        "glutSpecialFunc" => special_func as *const () as usize,
        "glutSpecialUpFunc" => special_up_func as *const () as usize,
        "glutMouseFunc" => mouse_func as *const () as usize,
        "glutMotionFunc" => motion_func as *const () as usize,
        "glutPassiveMotionFunc" => passive_func as *const () as usize,
        "glutIdleFunc" => idle_func as *const () as usize,
        "glutVisibilityFunc" => visibility_func as *const () as usize,
        "glutTimerFunc" => timer_func as *const () as usize,
        "glutMainLoop" => main_loop as *const () as usize,
        "glutSwapBuffers" => swap as *const () as usize,
        "glutPostRedisplay" => post_redisplay as *const () as usize,
        "glXGetVideoSyncSGI" => sync as *const () as usize,
        _ => {
            if let Some(words) = gl_words::words(name) {
                let lib = LoadLibraryA(
                    if name.starts_with("glu") {
                        c"glu32.dll".as_ptr()
                    } else {
                        c"opengl32.dll".as_ptr()
                    }
                    .cast(),
                );
                let address = proc(lib, name);
                if address == 0 {
                    return Err(format!("Windows OpenGL export missing: {name}"));
                }
                stdcall(address, words)?
            } else {
                return Ok(None);
            }
        }
    };
    Ok(Some(address))
}

pub unsafe extern "C" fn capture_service(name: *const i8) {
    if !name.is_null() {
        unsafe {
            capture(text(name));
        }
    }
}
pub unsafe extern "C" fn window_service() -> isize {
    unsafe { glut().window as isize }
}
pub unsafe extern "C" fn message_hook_service(hook: crate::extension::MessageHook) {
    unsafe { glut().hook = Some(hook) };
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn translates_glu_double_values_and_matrix_pointer_arguments() {
        unsafe {
            let address = resolve("gluUnProject").unwrap().unwrap();
            let f: unsafe extern "C" fn(
                f64,
                f64,
                f64,
                *const f64,
                *const f64,
                *const i32,
                *mut f64,
                *mut f64,
                *mut f64,
            ) -> i32 = std::mem::transmute(address);
            let matrix = [
                1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
            ];
            let viewport = [0, 0, 640, 480];
            let (mut x, mut y, mut z) = (0., 0., 0.);
            assert_eq!(
                f(
                    320.,
                    240.,
                    0.5,
                    matrix.as_ptr(),
                    matrix.as_ptr(),
                    viewport.as_ptr(),
                    &mut x,
                    &mut y,
                    &mut z
                ),
                1
            );
            assert_eq!((x, y, z), (0., 0., 0.));
            assert_eq!(
                f(
                    640.,
                    480.,
                    1.,
                    matrix.as_ptr(),
                    matrix.as_ptr(),
                    viewport.as_ptr(),
                    &mut x,
                    &mut y,
                    &mut z
                ),
                1
            );
            assert_eq!((x, y, z), (1., 1., 1.));
        }
    }

    #[test]
    fn timers_fire_in_due_order() {
        unsafe {
            glut().timers.clear();
            timer_func(30, 3, 0);
            timer_func(10, 1, 0);
            timer_func(20, 2, 0);
            let order: Vec<usize> = glut().timers.iter().map(|t| t.1).collect();
            assert_eq!(order, [1, 2, 3]);
            glut().timers.clear();
        }
    }
}
