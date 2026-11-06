//! Native imports recovered from the hash-verified Jurassic snapshot.
use std::ffi::{CStr, c_char, c_void};
use std::sync::atomic::{AtomicBool, Ordering};

#[path = "jurassic_controls.rs"]
mod controls;
static DESKTOP_CONTROLS: AtomicBool = AtomicBool::new(false);
#[link(name = "dl")]
unsafe extern "C" {
    fn dlsym(handle: *mut c_void, name: *const c_char) -> *mut c_void;
}
#[link(name = "SDL")]
unsafe extern "C" {
    fn SDL_GL_SetAttribute(attribute: i32, value: i32) -> i32;
    fn SDL_SetVideoMode(width: i32, height: i32, bits: i32, flags: u32) -> *mut c_void;
    fn SDL_PollEvent(event: *mut c_void) -> i32;
    fn SDL_GetMouseState(x: *mut i32, y: *mut i32) -> u8;
}
/// The engine drained its events last call: the next call starts a frame.
static DRAINED: AtomicBool = AtomicBool::new(true);
#[link(name = "GL")]
unsafe extern "C" {
    fn glGetString(query: u32) -> *const c_char;
}

extern "C" fn poll_event(event: *mut c_void) -> i32 {
    if DESKTOP_CONTROLS.load(Ordering::Relaxed) {
        unsafe { controls::update() };
        if DRAINED.swap(false, Ordering::Relaxed) {
            crate::jurassic_pad::frame();
        }
        if crate::jurassic_pad::next(event) {
            return 1;
        }
    }
    let result = unsafe { SDL_PollEvent(event) };
    if result == 0 {
        DRAINED.store(true, Ordering::Relaxed);
    }
    if result > 0 && !event.is_null() {
        let event = event as *const u8;
        let kind = unsafe { *event };
        if kind == 12
            || (kind == 2 && unsafe { (event.add(8) as *const i32).read_unaligned() } == 27)
        {
            std::process::exit(0);
        }
    }
    result
}

/// SDL_FULLSCREEN. Stripped for a desktop window unless settings.cfg asks
/// for fullscreen (ARCADE_FULLSCREEN=1).
const SDL_FULLSCREEN: u32 = 0x8000_0000;

extern "C" fn set_video_mode(width: i32, height: i32, bits: i32, flags: u32) -> *mut c_void {
    let fullscreen = std::env::var_os("ARCADE_FULLSCREEN").is_some_and(|v| v == "1");
    let flags = if fullscreen {
        flags | SDL_FULLSCREEN
    } else {
        flags & !SDL_FULLSCREEN
    };
    let surface = unsafe { SDL_SetVideoMode(width, height, bits, flags) };
    if !surface.is_null() {
        crate::jurassic_pad::WIDTH.store(width, Ordering::Relaxed);
        crate::jurassic_pad::HEIGHT.store(height, Ordering::Relaxed);
        let renderer = unsafe { glGetString(0x1f01) };
        if !renderer.is_null() {
            eprintln!(
                "[jurassic] {} SDL video {width}x{height}: {:?}",
                if fullscreen { "fullscreen" } else { "windowed" },
                unsafe { CStr::from_ptr(renderer) }
            );
        }
    }
    surface
}

/// Mouse position for aiming: the controller pointer while a stick is in use.
extern "C" fn get_mouse_state(x: *mut i32, y: *mut i32) -> u8 {
    let (mut real_x, mut real_y) = (0, 0);
    let buttons = unsafe { SDL_GetMouseState(&mut real_x, &mut real_y) };
    let (px, py) = if DESKTOP_CONTROLS.load(Ordering::Relaxed) {
        crate::jurassic_pad::pointer(real_x, real_y)
    } else {
        (real_x, real_y)
    };
    unsafe {
        if !x.is_null() {
            *x = px;
        }
        if !y.is_null() {
            *y = py;
        }
    }
    buttons
}

// SM_SetVSync/VidReset can run before a GL context exists. Let SDL keep the
// requested swap policy rather than sending an unsupported vendor GLX request.
// ARCADE_VSYNC (0/1, from settings.cfg) overrides the game's own choice.
extern "C" fn set_vsync(interval: i32) -> i32 {
    let interval = std::env::var("ARCADE_VSYNC")
        .ok()
        .and_then(|v| v.parse::<i32>().ok())
        .filter(|v| (0..=1).contains(v))
        .unwrap_or(interval);
    unsafe { SDL_GL_SetAttribute(16, interval) } // SDL_GL_SWAP_CONTROL
}
struct Binding {
    plt: usize,
    got: usize,
    saved: u32,
    symbol: &'static CStr,
}
const LIBC_BINDINGS: &[Binding] = include!("jurassic_libc_bindings.rs");
const PTHREAD_BINDINGS: &[Binding] = include!("jurassic_pthread_bindings.rs");
const LIBM_BINDINGS: &[Binding] = include!("jurassic_libm_bindings.rs");
const SDL_BINDINGS: &[Binding] = include!("jurassic_sdl_bindings.rs");
const GL_BINDINGS: &[Binding] = include!("jurassic_gl_bindings.rs");

// Identified from C++/graphics callers and the matching PhysX SDK export.
const SUPPLEMENTAL_BINDINGS: &[Binding] = &[
    Binding {
        plt: 0x0813bb50,
        got: 0x08add910,
        saved: 0,
        symbol: c"NxGetCookingLib",
    },
    Binding {
        plt: 0x0813a690,
        got: 0x08add3e0,
        saved: 0,
        symbol: c"_Znwj",
    },
    Binding {
        plt: 0x0813a510,
        got: 0x08add380,
        saved: 0,
        symbol: c"_ZdlPv",
    },
    Binding {
        plt: 0x0813b520,
        got: 0x08add784,
        saved: 0,
        symbol: c"_ZNSt8ios_base4InitC1Ev",
    },
    Binding {
        plt: 0x0813a930,
        got: 0x08add488,
        saved: 0,
        symbol: c"_ZNSt8ios_base4InitD1Ev",
    },
    Binding {
        plt: 0x0813b340,
        got: 0x08add70c,
        saved: 0,
        symbol: c"SDL_GL_SetAttribute",
    },
    Binding {
        plt: 0x0813b330,
        got: 0x08add708,
        saved: 0,
        symbol: c"gluErrorString",
    },
    Binding {
        plt: 0x0813abf0,
        got: 0x08add538,
        saved: 0,
        symbol: c"glGetObjectParameterivARB",
    },
    Binding {
        plt: 0x0813a320,
        got: 0x08add304,
        saved: 0,
        symbol: c"glGetInfoLogARB",
    },
    Binding {
        plt: 0x0813b5a0,
        got: 0x08add7a4,
        saved: 0,
        symbol: c"glGenerateMipmapEXT",
    },
    Binding {
        plt: 0x0813ad30,
        got: 0x08add588,
        saved: 0,
        symbol: c"glXGetProcAddressARB",
    },
    Binding {
        plt: 0x0813a0a0,
        got: 0x08add264,
        saved: 0,
        symbol: c"glXQueryVersion",
    },
    Binding {
        plt: 0x0813bb20,
        got: 0x08add904,
        saved: 0,
        symbol: c"glXGetClientString",
    },
];

pub fn initialize() {
    if !std::env::var_os("ARCADE_JURASSIC").is_some_and(|v| v == "1") {
        return;
    }
    if let Err(error) = rebind() {
        eprintln!("[jurassic] import repair aborted: {error}");
        std::process::exit(1);
    }
    if std::env::var_os("ARCADE_JURASSIC_DESKTOP").is_some_and(|v| v == "1") {
        if let Err(error) = controls::initialize() {
            eprintln!("[jurassic] desktop controls aborted: {error}");
            std::process::exit(1);
        }
        DESKTOP_CONTROLS.store(true, Ordering::Relaxed);
        eprintln!("[jurassic] desktop controls active (see controls.cfg); mouse aims, Esc exits");
    }
}

fn rebind() -> Result<(), String> {
    // Validate every instruction/slot identity and resolve every name before mutation.
    let mut resolved = Vec::new();
    for binding in LIBC_BINDINGS
        .iter()
        .chain(PTHREAD_BINDINGS)
        .chain(LIBM_BINDINGS)
        .chain(SDL_BINDINGS)
        .chain(GL_BINDINGS)
        .chain(SUPPLEMENTAL_BINDINGS)
    {
        let bytes = unsafe { std::slice::from_raw_parts(binding.plt as *const u8, 6) };
        let got = u32::from_le_bytes(bytes[2..6].try_into().unwrap()) as usize;
        if bytes[..2] != [0xff, 0x25] || got != binding.got {
            return Err(format!("incompatible PLT at {:#x}", binding.plt));
        }
        let function = if binding.plt == 0x0813b770 {
            poll_event as *const () as *mut c_void
        } else if binding.plt == 0x0813a450 {
            set_video_mode as *const () as *mut c_void
        } else if binding.plt == 0x0813b340 {
            set_vsync as *const () as *mut c_void
        } else if binding.plt == 0x0813b390 {
            get_mouse_state as *const () as *mut c_void
        } else {
            unsafe { dlsym((-1isize) as *mut c_void, binding.symbol.as_ptr()) }
        };
        if function.is_null() {
            return Err(format!(
                "cannot resolve {:?} (saved {:#x})",
                binding.symbol, binding.saved
            ));
        }
        resolved.push((got, function));
    }
    for (got, function) in resolved {
        unsafe {
            (got as *mut *mut c_void).write(function);
        }
    }
    eprintln!(
        "[jurassic] rebound {} fingerprint-verified imports and {} supplemental imports",
        LIBC_BINDINGS.len()
            + PTHREAD_BINDINGS.len()
            + LIBM_BINDINGS.len()
            + SDL_BINDINGS.len()
            + GL_BINDINGS.len(),
        SUPPLEMENTAL_BINDINGS.len()
    );
    Ok(())
}
