use super::*;
use windows_sys::Win32::{
    Foundation::*,
    Graphics::{Gdi::*, OpenGL::*},
    UI::WindowsAndMessaging::*,
};
#[path = "gl_words.rs"]
mod gl_words;

/// Big Buck crosshair (settings.cfg `crosshair`, via ARCADE_CROSSHAIR), drawn
/// over each finished frame at the aim position. All GL state is restored.
mod crosshair {
    use super::*;
    /// Padded to a power of two for old GL 1.x drivers.
    const SIDE: u32 = 128;
    static TEXTURE: OnceLock<Option<(u32, u32, u32)>> = OnceLock::new();

    pub fn enabled() -> bool {
        style().is_some()
    }
    fn style() -> Option<&'static [u8]> {
        std::env::var("ARCADE_CROSSHAIR")
            .ok()
            .and_then(|id| arcade_core::crosshair::png(&id))
    }
    unsafe fn upload() -> Option<(u32, u32, u32)> {
        let image = match image::load_from_memory_with_format(style()?, image::ImageFormat::Png) {
            Ok(image) => image.to_rgba8(),
            Err(e) => {
                eprintln!("[water] crosshair disabled: {e}");
                return None;
            }
        };
        let (w, h) = image.dimensions();
        if w > SIDE || h > SIDE {
            eprintln!("[water] crosshair disabled: {w}x{h} is larger than {SIDE}x{SIDE}");
            return None;
        }
        let mut square = image::RgbaImage::new(SIDE, SIDE);
        image::imageops::replace(&mut square, &image, 0, 0);
        let mut texture = 0;
        glPushAttrib(GL_ALL_ATTRIB_BITS);
        glPushClientAttrib(GL_CLIENT_PIXEL_STORE_BIT);
        glGenTextures(1, &mut texture);
        glBindTexture(GL_TEXTURE_2D, texture);
        glPixelStorei(GL_UNPACK_ALIGNMENT, 1);
        glPixelStorei(GL_UNPACK_ROW_LENGTH, 0);
        glPixelStorei(GL_UNPACK_SKIP_ROWS, 0);
        glPixelStorei(GL_UNPACK_SKIP_PIXELS, 0);
        glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_LINEAR as i32);
        glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_LINEAR as i32);
        glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_S, 0x812f); // CLAMP_TO_EDGE
        glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_T, 0x812f);
        glTexImage2D(
            GL_TEXTURE_2D,
            0,
            GL_RGBA as i32,
            SIDE as i32,
            SIDE as i32,
            0,
            GL_RGBA,
            GL_UNSIGNED_BYTE,
            square.as_raw().as_ptr().cast(),
        );
        glPopClientAttrib();
        glPopAttrib();
        eprintln!("[water] crosshair enabled ({w}x{h})");
        Some((texture, w, h))
    }
    pub unsafe fn draw() {
        let Some((texture, w, h)) = *TEXTURE.get_or_init(|| upload()) else {
            return;
        };
        let (x, y) = jamma::aim_position();
        static SIZE: OnceLock<u32> = OnceLock::new();
        let size = *SIZE.get_or_init(|| {
            arcade_core::crosshair::size_percent(
                std::env::var("ARCADE_CROSSHAIR_SIZE").ok().as_deref(),
            )
        });
        let [left, top, right, bottom] =
            arcade_core::crosshair::quad(x, y, w, h, size, WIDTH, HEIGHT);
        let (s, t) = (w as f32 / SIDE as f32, h as f32 / SIDE as f32);
        glPushAttrib(GL_ALL_ATTRIB_BITS);
        for mode in [GL_PROJECTION, GL_MODELVIEW, GL_TEXTURE] {
            glMatrixMode(mode);
            glPushMatrix();
            glLoadIdentity();
        }
        glMatrixMode(GL_PROJECTION);
        glOrtho(0.0, WIDTH as f64, HEIGHT as f64, 0.0, -1.0, 1.0);
        glViewport(0, 0, WIDTH, HEIGHT);
        for cap in [
            GL_DEPTH_TEST,
            GL_LIGHTING,
            GL_CULL_FACE,
            GL_FOG,
            GL_ALPHA_TEST,
            GL_STENCIL_TEST,
            GL_SCISSOR_TEST,
            GL_COLOR_MATERIAL,
            GL_TEXTURE_GEN_S,
            GL_TEXTURE_GEN_T,
            GL_TEXTURE_1D,
        ] {
            glDisable(cap);
        }
        glEnable(GL_TEXTURE_2D);
        glEnable(GL_BLEND);
        glBlendFunc(GL_SRC_ALPHA, GL_ONE_MINUS_SRC_ALPHA);
        glColorMask(1, 1, 1, 1);
        glPolygonMode(GL_FRONT_AND_BACK, GL_FILL);
        glBindTexture(GL_TEXTURE_2D, texture);
        glTexEnvi(GL_TEXTURE_ENV, GL_TEXTURE_ENV_MODE, GL_MODULATE as i32);
        glColor4f(1.0, 1.0, 1.0, 1.0);
        glBegin(GL_QUADS);
        glTexCoord2f(0.0, 0.0);
        glVertex2f(left, top);
        glTexCoord2f(s, 0.0);
        glVertex2f(right, top);
        glTexCoord2f(s, t);
        glVertex2f(right, bottom);
        glTexCoord2f(0.0, t);
        glVertex2f(left, bottom);
        glEnd();
        for mode in [GL_TEXTURE, GL_MODELVIEW, GL_PROJECTION] {
            glMatrixMode(mode);
            glPopMatrix();
        }
        glPopAttrib();
    }
}

static mut WINDOW: HWND = ptr::null_mut();
static mut DC: HDC = ptr::null_mut();
static mut WIDTH: i32 = 640;
static mut HEIGHT: i32 = 480;
static mut DISPLAY: usize = 0;
static mut RESHAPE: usize = 0;
static mut FRAME: u64 = 0;
static mut STEP: usize = 0;
static mut HUNT_TICK: u128 = u128::MAX;
static mut MOUSE_RELEASE: Option<(u128, i32, i32, i32)> = None;
static START: OnceLock<std::time::Instant> = OnceLock::new();

unsafe extern "system" fn wndproc(window: HWND, message: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    match message {
        WM_CLOSE => {
            jamma::release();
            eprintln!("[water] window closed");
            ExitProcess(0);
        }
        WM_KEYDOWN | WM_KEYUP => {
            if w == 27 && message == WM_KEYDOWN {
                jamma::release();
                ExitProcess(0);
            }
            if message == WM_KEYUP || l & (1 << 30) == 0 {
                jamma::key(w as u8, message == WM_KEYDOWN);
            }
            return 0;
        }
        WM_KILLFOCUS => jamma::release(),
        // The drawn crosshair replaces the pointer inside the window.
        WM_SETCURSOR if (l & 0xffff) as u32 == HTCLIENT && crosshair::enabled() => {
            SetCursor(ptr::null_mut());
            return 1;
        }
        WM_LBUTTONDOWN | WM_LBUTTONUP | WM_RBUTTONDOWN | WM_RBUTTONUP | WM_MOUSEMOVE => {
            let x = (l as i16 as i32) * 640 / WIDTH.max(1);
            let y = ((l >> 16) as i16 as i32) * 480 / HEIGHT.max(1);
            if message == WM_MOUSEMOVE {
                jamma::aim(x, y, false);
            } else {
                jamma::mouse(
                    if message == WM_LBUTTONDOWN || message == WM_LBUTTONUP {
                        0
                    } else {
                        2
                    },
                    message == WM_LBUTTONDOWN || message == WM_RBUTTONDOWN,
                    x,
                    y,
                );
            }
            return 0;
        }
        WM_SIZE => {
            WIDTH = (l as u32 & 65535) as i32;
            HEIGHT = ((l as u32 >> 16) & 65535) as i32;
            if RESHAPE != 0 && WIDTH > 0 && HEIGHT > 0 {
                let f: unsafe extern "C" fn(i32, i32) = std::mem::transmute(RESHAPE);
                f(WIDTH, HEIGHT);
            }
        }
        _ => {}
    }
    DefWindowProcA(window, message, w, l)
}

unsafe extern "C" fn create(_title: *const i8) -> i32 {
    if !WINDOW.is_null() {
        return 1;
    }
    let class = WNDCLASSA {
        style: CS_OWNDC,
        lpfnWndProc: Some(wndproc),
        hInstance: GetModuleHandleA(ptr::null()),
        lpszClassName: c"BananaPepperELF32".as_ptr().cast(),
        hCursor: LoadCursorW(ptr::null_mut(), IDC_CROSS),
        ..std::mem::zeroed()
    };
    RegisterClassA(&class);
    let mut rect = RECT {
        left: 0,
        top: 0,
        right: WIDTH,
        bottom: HEIGHT,
    };
    AdjustWindowRect(&mut rect, WS_OVERLAPPEDWINDOW, 0);
    WINDOW = CreateWindowExA(
        0,
        class.lpszClassName,
        c"Banana pepper - Big Buck Hunter Pro (Water)"
            .as_ptr()
            .cast(),
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
    if WINDOW.is_null() {
        eprintln!(
            "[water] CreateWindow failed: {}",
            std::io::Error::last_os_error()
        );
        ExitProcess(125);
    }
    DC = GetDC(WINDOW);
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
    let mut index = ChoosePixelFormat(DC, &format);
    if std::env::var("ARCADE_RENDERER").is_ok_and(|s| s == "software") {
        let count = DescribePixelFormat(
            DC,
            1,
            std::mem::size_of::<PIXELFORMATDESCRIPTOR>() as u32,
            ptr::null_mut(),
        );
        index = 0;
        for candidate in 1..=count {
            let mut p: PIXELFORMATDESCRIPTOR = std::mem::zeroed();
            DescribePixelFormat(
                DC,
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
    if index == 0 || SetPixelFormat(DC, index, &format) == 0 {
        eprintln!("[water] pixel format failed");
        ExitProcess(125);
    }
    let context = wglCreateContext(DC);
    if context.is_null() || wglMakeCurrent(DC, context) == 0 {
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
    if std::env::var("ARCADE_FULLSCREEN").is_ok_and(|s| s == "1") {
        let monitor = MonitorFromWindow(WINDOW, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..std::mem::zeroed()
        };
        if GetMonitorInfoA(monitor, &mut info) != 0 {
            SetWindowLongA(WINDOW, GWL_STYLE, WS_POPUP as i32);
            let r = info.rcMonitor;
            SetWindowPos(
                WINDOW,
                ptr::null_mut(),
                r.left,
                r.top,
                r.right - r.left,
                r.bottom - r.top,
                SWP_FRAMECHANGED | SWP_SHOWWINDOW,
            );
        }
    }
    eprintln!(
        "[water] OpenGL renderer: {}",
        text(glGetString(GL_RENDERER).cast())
    );
    eprintln!("[water] Windows OpenGL window {}x{}", { WIDTH }, {
        HEIGHT
    });
    1
}
unsafe extern "C" fn init(_argc: usize, _argv: usize) {}
unsafe extern "C" fn size(width: i32, height: i32) {
    let (width, height) = std::env::var("ARCADE_RESOLUTION")
        .ok()
        .and_then(|s| arcade_core::settings::parse_resolution(&s).ok())
        .map(|(w, h)| (w as i32, h as i32))
        .unwrap_or((width, height));
    WIDTH = width.clamp(320, 3840);
    HEIGHT = height.clamp(240, 2160);
}
unsafe extern "C" fn noarg() {}
unsafe extern "C" fn one(_value: usize) {}
unsafe extern "C" fn two(_a: usize, _b: usize) {}
unsafe extern "C" fn modifiers() -> i32 {
    0
}
unsafe extern "C" fn display(callback: usize) {
    DISPLAY = callback;
}
unsafe extern "C" fn reshape(callback: usize) {
    RESHAPE = callback;
    if callback != 0 {
        let f: unsafe extern "C" fn(i32, i32) = std::mem::transmute(callback);
        f(WIDTH, HEIGHT);
    }
}
unsafe extern "C" fn enter_game() -> i32 {
    create(c"Big Buck".as_ptr())
}
unsafe extern "C" fn main_loop() -> ! {
    eprintln!("[water] entering native event loop");
    let mut message: MSG = std::mem::zeroed();
    loop {
        probe();
        jamma::controller_frame(GetForegroundWindow() == WINDOW);
        while PeekMessageA(&mut message, ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
            if message.message == WM_QUIT {
                ExitProcess(0);
            }
            TranslateMessage(&message);
            DispatchMessageA(&message);
        }
        if DISPLAY != 0 && WIDTH > 0 && HEIGHT > 0 && IsIconic(WINDOW) == 0 {
            let f: unsafe extern "C" fn() = std::mem::transmute(DISPLAY);
            f();
        } else {
            Sleep(20);
        }
    }
}
unsafe extern "C" fn swap() {
    FRAME += 1;
    if FRAME == 1 || FRAME % 600 == 0 {
        eprintln!("[water] rendered frame {}", { FRAME });
    }
    if std::env::var_os("ARCADE_NATIVE_PROBE").is_some() && FRAME % 120 == 0 {
        capture("screen.png");
    }
    if std::env::var_os("ARCADE_NATIVE_CAPTURE_ALL").is_some() && FRAME % 60 == 0 {
        capture(&format!(
            "frame-{:03}.png",
            START
                .get_or_init(std::time::Instant::now)
                .elapsed()
                .as_secs()
        ));
    }
    if crosshair::enabled() {
        crosshair::draw();
    }
    SwapBuffers(DC);
    Sleep(1);
}
unsafe fn capture(name: &str) {
    if WIDTH <= 0 || HEIGHT <= 0 {
        return;
    }
    let mut pixels = vec![0u8; WIDTH as usize * HEIGHT as usize * 3];
    glPixelStorei(GL_PACK_ALIGNMENT, 1);
    glReadPixels(
        0,
        0,
        WIDTH,
        HEIGHT,
        GL_RGB,
        GL_UNSIGNED_BYTE,
        pixels.as_mut_ptr().cast(),
    );
    if let Some(buffer) = image::RgbImage::from_raw(WIDTH as u32, HEIGHT as u32, pixels) {
        let buffer = image::imageops::flip_vertical(&buffer);
        if let Err(e) = buffer.save(WORK.get().unwrap().join(name)) {
            eprintln!("[water] capture failed: {e}");
        }
    }
}
unsafe fn probe() {
    if std::env::var_os("ARCADE_NATIVE_PROBE").is_none() {
        return;
    }
    let elapsed = START
        .get_or_init(std::time::Instant::now)
        .elapsed()
        .as_millis();
    if let Some((time, button, x, y)) = MOUSE_RELEASE {
        if elapsed >= time {
            jamma::mouse(button, false, x, y);
            MOUSE_RELEASE = None;
        }
    }
    if std::env::var_os("ARCADE_NATIVE_FIRE_PROBE").is_some() && (48000..50000).contains(&elapsed) {
        let tick = (elapsed - 48000) / 200;
        if tick != HUNT_TICK {
            HUNT_TICK = tick;
            let x = 80 + ((tick / 2 * 120) % 480) as i32;
            let y = 260;
            let button = if tick % 2 == 0 { 2 } else { 0 };
            probe_mouse(elapsed, button, x, y);
        }
    }
    // Repeat the measured Linux progression sequence through the same cabinet
    // adapter. No OS input injection, no automatic inputs in ordinary sessions.
    let steps: &[(u128, u8, i32, i32, &str)] = &[
        (2000, b't', 0, 0, "calibration.png"),
        (2200, 0, 0, 0, ""),
        (4000, b'1', 0, 0, "tracking.png"),
        (4200, 0, 0, 0, ""),
        (6000, b'5', 0, 0, "attract.png"),
        (6300, 0, 0, 0, ""),
        (7000, b'5', 0, 0, ""),
        (7300, 0, 0, 0, ""),
        (8000, b'5', 0, 0, ""),
        (8300, 0, 0, 0, ""),
        (9000, b'5', 0, 0, ""),
        (9300, 0, 0, 0, "coins.png"),
        (11000, b'1', 0, 0, "before-start.png"),
        (11300, 0, 0, 0, ""),
        (14000, 1, 160, 220, "game-select.png"),
        (17000, 1, 160, 220, "mode-select.png"),
        (20000, 1, 160, 140, "adventure-select.png"),
        (23000, 1, 160, 180, "adventure-intro.png"),
        (26000, 1, 160, 180, "records.png"),
        (29000, 1, 160, 180, "instructions.png"),
        (35000, 1, 320, 240, "instructions2.png"),
        (39000, 1, 120, 150, "site-select.png"),
        (45000, 1, 120, 150, "site-select2.png"),
        (50000, 1, 350, 240, "hunt.png"),
        (51000, 2, 350, 240, "after-shot.png"),
        (52000, 1, 300, 200, "after-pump.png"),
        (53000, 2, 300, 200, ""),
        (54000, 1, 200, 300, ""),
        (58000, 0, 0, 0, "final.png"),
        (62000, 0, 0, 0, "shot-result.png"),
        (70000, 0, 0, 0, "hunt-end.png"),
        (72000, 1, 320, 400, "initials.png"),
        (80000, 0, 0, 0, "results.png"),
        (90000, 0, 0, 0, "site-results.png"),
    ];
    if STEP >= steps.len() {
        return;
    }
    let (time, key, x, y, name) = steps[STEP];
    if elapsed < time {
        return;
    }
    if !name.is_empty() {
        capture(name);
        eprintln!("[water] probe checkpoint {name}");
    }
    if std::env::var_os("ARCADE_NATIVE_FIRE_PROBE").is_some() && (50000..60000).contains(&elapsed) {
        // Observe the first hunt's outcome without later shots hitting a doe.
    } else if key == 1 || key == 2 {
        probe_mouse(elapsed, if key == 1 { 0 } else { 2 }, x, y);
    } else {
        jamma::key(b't', false);
        jamma::key(b'1', false);
        jamma::key(b'5', false);
        if key != 0 {
            jamma::key(key, true);
        }
    }
    STEP += 1;
}
unsafe fn probe_mouse(elapsed: u128, button: i32, x: i32, y: i32) {
    let previous = MOUSE_RELEASE;
    MOUSE_RELEASE = None;
    if let Some((_, old, x, y)) = previous {
        jamma::mouse(old, false, x, y);
    }
    jamma::mouse(button, true, x, y);
    MOUSE_RELEASE = Some((elapsed + 100, button, x, y));
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
        "glutInitWindowSize" => size as *const () as usize,
        "glutInitWindowPosition" => two as *const () as usize,
        "glutInitDisplayMode" | "glutGameModeString" | "glutSetWindow" | "glutSetCursor" => {
            one as *const () as usize
        }
        "glutCreateWindow" => create as *const () as usize,
        "glutEnterGameMode" => enter_game as *const () as usize,
        "glutDisplayFunc" => display as *const () as usize,
        "glutReshapeFunc" => reshape as *const () as usize,
        "glutMainLoop" => main_loop as *const () as usize,
        "glutSwapBuffers" => swap as *const () as usize,
        "glutPostRedisplay" => noarg as *const () as usize,
        "glutGetModifiers" => modifiers as *const () as usize,
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
}
