//! Desktop exit for Aliens. Its statically linked SDL 1.2 loads X11 itself, so
//! its events cannot be interposed; the engine also ignores SDL_QUIT. A watcher
//! thread on a separate XCB connection therefore provides:
//! * Escape while an Aliens window has keyboard focus: exit the game.
//! * The window close button: WM_DELETE_WINDOW is removed from the game
//!   window's WM_PROTOCOLS, so the window manager disconnects the client
//!   instead of sending an ignored close request.
//! * Fullscreen (ARCADE_FULLSCREEN=1 from settings.cfg): the window manager is
//!   asked for EWMH fullscreen, which fills the monitor the window is on. The
//!   game's own SDL 1.2 fullscreen covers the whole X screen instead, i.e.
//!   every monitor at once, so the launcher always builds a windowed image.
//!
//! * Controllers (evdev): while an Aliens window has focus, bound buttons
//!   press the action's own key, RT/LT click the left/right mouse button and
//!   either stick moves the pointer, all through XTest so the game sees
//!   ordinary X input. Back + Start exits.
//! * Two players (settings.cfg `players = 2`): each player's controller
//!   presses that player's keys (Start 2 and Coin 2 for player two). The
//!   player with the mouse fires with its buttons (RT/LT click them); the
//!   other player fires with Q/W and aims with its stick through its gun
//!   sources, which the prepared image maps to its aim and which are written
//!   here through the game's own analog setter. With player two on keyboard
//!   and mouse, player one is the controller-aimed player.
//!
//! Game windows are identified by the unique SDL_VIDEO_X11_WMCLASS the
//! launcher sets. XCB is loaded with dlopen; it reports errors per request, so
//! nothing here touches the game's own Xlib error handlers.
use std::ffi::{c_char, c_int, c_void};
use std::time::{Duration, Instant};

#[repr(C)]
#[derive(Clone, Copy)]
struct Cookie {
    sequence: u32,
}
#[repr(C)]
struct FocusReply {
    response_type: u8,
    revert_to: u8,
    sequence: u16,
    length: u32,
    focus: u32,
}
#[repr(C)]
struct TreeReply {
    response_type: u8,
    pad0: u8,
    sequence: u16,
    length: u32,
    root: u32,
    parent: u32,
    children_len: u16,
    pad1: [u8; 14],
}
#[repr(C)]
struct KeymapReply {
    response_type: u8,
    pad0: u8,
    sequence: u16,
    length: u32,
    keys: [u8; 32],
}
#[repr(C)]
struct AtomReply {
    response_type: u8,
    pad0: u8,
    sequence: u16,
    length: u32,
    atom: u32,
}
#[repr(C)]
struct PropertyReply {
    response_type: u8,
    format: u8,
    sequence: u16,
    length: u32,
    kind: u32,
    bytes_after: u32,
    value_len: u32,
    pad0: [u8; 12],
}
#[repr(C)]
struct KeyboardMappingReply {
    response_type: u8,
    keysyms_per_keycode: u8,
    sequence: u16,
    length: u32,
    pad0: [u8; 24],
}
#[repr(C)]
struct ScreenIterator {
    data: *const u32, // xcb_screen_t begins with its root window
    rem: c_int,
    index: c_int,
}

type Conn = *mut c_void;
type Err = *mut *mut c_void;

struct Xcb {
    connection_has_error: unsafe extern "C" fn(Conn) -> c_int,
    get_setup: unsafe extern "C" fn(Conn) -> *const c_void,
    setup_roots_iterator: unsafe extern "C" fn(*const c_void) -> ScreenIterator,
    flush: unsafe extern "C" fn(Conn) -> c_int,
    poll_for_event: unsafe extern "C" fn(Conn) -> *mut c_void,
    get_input_focus: unsafe extern "C" fn(Conn) -> Cookie,
    get_input_focus_reply: unsafe extern "C" fn(Conn, Cookie, Err) -> *mut FocusReply,
    query_tree: unsafe extern "C" fn(Conn, u32) -> Cookie,
    query_tree_reply: unsafe extern "C" fn(Conn, Cookie, Err) -> *mut TreeReply,
    query_tree_children: unsafe extern "C" fn(*const TreeReply) -> *const u32,
    query_tree_children_length: unsafe extern "C" fn(*const TreeReply) -> c_int,
    query_keymap: unsafe extern "C" fn(Conn) -> Cookie,
    query_keymap_reply: unsafe extern "C" fn(Conn, Cookie, Err) -> *mut KeymapReply,
    intern_atom: unsafe extern "C" fn(Conn, u8, u16, *const c_char) -> Cookie,
    intern_atom_reply: unsafe extern "C" fn(Conn, Cookie, Err) -> *mut AtomReply,
    get_property: unsafe extern "C" fn(Conn, u8, u32, u32, u32, u32, u32) -> Cookie,
    get_property_reply: unsafe extern "C" fn(Conn, Cookie, Err) -> *mut PropertyReply,
    get_property_value: unsafe extern "C" fn(*const PropertyReply) -> *const c_void,
    get_property_value_length: unsafe extern "C" fn(*const PropertyReply) -> c_int,
    delete_property: unsafe extern "C" fn(Conn, u32, u32) -> Cookie,
    send_event: unsafe extern "C" fn(Conn, u8, u32, u32, *const u8) -> Cookie,
    get_keyboard_mapping: unsafe extern "C" fn(Conn, u8, u8) -> Cookie,
    get_keyboard_mapping_reply: unsafe extern "C" fn(Conn, Cookie, Err) -> *mut KeyboardMappingReply,
    get_keyboard_mapping_keysyms: unsafe extern "C" fn(*const KeyboardMappingReply) -> *const u32,
    get_keyboard_mapping_keysyms_length: unsafe extern "C" fn(*const KeyboardMappingReply) -> c_int,
}

impl Xcb {
    // Each symbol's type is fixed by the struct field it initializes.
    #[allow(clippy::missing_transmute_annotations)]
    fn load() -> Result<
        (
            Self,
            unsafe extern "C" fn(*const c_char, *mut c_int) -> Conn,
        ),
        String,
    > {
        let handle =
            unsafe { libc::dlopen(c"libxcb.so.1".as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
        if handle.is_null() {
            return Err("libxcb.so.1 is unavailable".into());
        }
        macro_rules! sym {
            ($name:literal) => {{
                let pointer = unsafe { libc::dlsym(handle, $name.as_ptr()) };
                if pointer.is_null() {
                    return Err(format!("libxcb lacks {:?}", $name));
                }
                unsafe { std::mem::transmute::<*mut c_void, _>(pointer) }
            }};
        }
        let connect = sym!(c"xcb_connect");
        Ok((
            Self {
                connection_has_error: sym!(c"xcb_connection_has_error"),
                get_setup: sym!(c"xcb_get_setup"),
                setup_roots_iterator: sym!(c"xcb_setup_roots_iterator"),
                flush: sym!(c"xcb_flush"),
                poll_for_event: sym!(c"xcb_poll_for_event"),
                get_input_focus: sym!(c"xcb_get_input_focus"),
                get_input_focus_reply: sym!(c"xcb_get_input_focus_reply"),
                query_tree: sym!(c"xcb_query_tree"),
                query_tree_reply: sym!(c"xcb_query_tree_reply"),
                query_tree_children: sym!(c"xcb_query_tree_children"),
                query_tree_children_length: sym!(c"xcb_query_tree_children_length"),
                query_keymap: sym!(c"xcb_query_keymap"),
                query_keymap_reply: sym!(c"xcb_query_keymap_reply"),
                intern_atom: sym!(c"xcb_intern_atom"),
                intern_atom_reply: sym!(c"xcb_intern_atom_reply"),
                get_property: sym!(c"xcb_get_property"),
                get_property_reply: sym!(c"xcb_get_property_reply"),
                get_property_value: sym!(c"xcb_get_property_value"),
                get_property_value_length: sym!(c"xcb_get_property_value_length"),
                delete_property: sym!(c"xcb_delete_property"),
                send_event: sym!(c"xcb_send_event"),
                get_keyboard_mapping: sym!(c"xcb_get_keyboard_mapping"),
                get_keyboard_mapping_reply: sym!(c"xcb_get_keyboard_mapping_reply"),
                get_keyboard_mapping_keysyms: sym!(c"xcb_get_keyboard_mapping_keysyms"),
                get_keyboard_mapping_keysyms_length: sym!(
                    c"xcb_get_keyboard_mapping_keysyms_length"
                ),
            },
            connect,
        ))
    }
}

/// Owned XCB reply/error memory (malloc'd by libxcb).
fn take<T>(reply: *mut T, error: *mut c_void) -> Option<*mut T> {
    if !error.is_null() {
        unsafe { libc::free(error) };
    }
    (!reply.is_null()).then_some(reply)
}
fn release<T>(reply: *mut T) {
    unsafe { libc::free(reply.cast()) };
}

const ATOM_WM_CLASS: u32 = 67;
const ATOM_STRING: u32 = 31;
const ANY_PROPERTY_TYPE: u32 = 0;
/// Evdev keycode for Escape (Xorg and XWayland).
const ESCAPE_KEYCODE: usize = 9;

pub struct Watcher {
    x: Xcb,
    conn: Conn,
    root: u32,
    wm_protocols: u32,
    net_wm_state: u32,
    net_wm_state_fullscreen: u32,
    /// Windows already asked to go fullscreen.
    fullscreen_requested: Vec<u32>,
    class: Vec<u8>,
    /// Top-level windows carrying the launcher's WM_CLASS.
    pub windows: Vec<u32>,
}

impl Watcher {
    pub fn connect(class: &str) -> Result<Self, String> {
        let (x, connect) = Xcb::load()?;
        let conn = unsafe { connect(std::ptr::null(), std::ptr::null_mut()) };
        if conn.is_null() || unsafe { (x.connection_has_error)(conn) } != 0 {
            return Err("cannot connect to the X display".into());
        }
        let root = unsafe { *(x.setup_roots_iterator)((x.get_setup)(conn)).data };
        let atom = |name: &[u8]| -> Result<u32, String> {
            let mut error = std::ptr::null_mut();
            let cookie =
                unsafe { (x.intern_atom)(conn, 0, name.len() as u16, name.as_ptr().cast()) };
            let reply = take(
                unsafe { (x.intern_atom_reply)(conn, cookie, &mut error) },
                error,
            )
            .ok_or_else(|| format!("cannot intern {}", String::from_utf8_lossy(name)))?;
            let atom = unsafe { (*reply).atom };
            release(reply);
            Ok(atom)
        };
        let wm_protocols = atom(b"WM_PROTOCOLS")?;
        let net_wm_state = atom(b"_NET_WM_STATE")?;
        let net_wm_state_fullscreen = atom(b"_NET_WM_STATE_FULLSCREEN")?;
        Ok(Self {
            x,
            conn,
            root,
            wm_protocols,
            net_wm_state,
            net_wm_state_fullscreen,
            fullscreen_requested: Vec::new(),
            class: class.as_bytes().to_vec(),
            windows: Vec::new(),
        })
    }

    fn drain_events(&self) {
        loop {
            let event = unsafe { (self.x.poll_for_event)(self.conn) };
            if event.is_null() {
                break;
            }
            release(event);
        }
    }

    fn children(&self, window: u32) -> Option<(u32, Vec<u32>)> {
        let mut error = std::ptr::null_mut();
        let cookie = unsafe { (self.x.query_tree)(self.conn, window) };
        let reply = take(
            unsafe { (self.x.query_tree_reply)(self.conn, cookie, &mut error) },
            error,
        )?;
        let count = unsafe { (self.x.query_tree_children_length)(reply) }.max(0) as usize;
        let children = unsafe { (self.x.query_tree_children)(reply) };
        let list = if children.is_null() {
            Vec::new()
        } else {
            unsafe { std::slice::from_raw_parts(children, count) }.to_vec()
        };
        let parent = unsafe { (*reply).parent };
        release(reply);
        Some((parent, list))
    }

    /// Property bytes (8/32-bit formats both returned as raw bytes).
    fn property(&self, window: u32, atom: u32, kind: u32) -> Option<Vec<u8>> {
        let mut error = std::ptr::null_mut();
        let cookie = unsafe { (self.x.get_property)(self.conn, 0, window, atom, kind, 0, 64) };
        let reply = take(
            unsafe { (self.x.get_property_reply)(self.conn, cookie, &mut error) },
            error,
        )?;
        let length = unsafe { (self.x.get_property_value_length)(reply) }.max(0) as usize;
        let value = unsafe { (self.x.get_property_value)(reply) };
        let bytes = if value.is_null() || length == 0 {
            Vec::new()
        } else {
            unsafe { std::slice::from_raw_parts(value.cast::<u8>(), length) }.to_vec()
        };
        release(reply);
        Some(bytes)
    }

    fn is_game(&self, window: u32) -> bool {
        // WM_CLASS is "instance\0class\0"; match the class component.
        self.property(window, ATOM_WM_CLASS, ATOM_STRING)
            .is_some_and(|value| value.split(|b| *b == 0).nth(1) == Some(self.class.as_slice()))
    }

    /// Search root → frame → wrapper → client for windows with our class.
    pub fn find_windows(&mut self) {
        let mut found = Vec::new();
        let mut level = vec![self.root];
        for _ in 0..4 {
            let mut next = Vec::new();
            for window in level {
                if let Some((_, children)) = self.children(window) {
                    for child in children {
                        if self.is_game(child) {
                            found.push(child);
                        } else {
                            next.push(child);
                        }
                    }
                }
            }
            level = next;
        }
        self.windows = found;
    }

    /// Re-validate known windows; rescan if any disappeared or none are known.
    pub fn refresh(&mut self) {
        if self.windows.is_empty() || !self.windows.iter().all(|w| self.is_game(*w)) {
            self.find_windows();
        }
    }

    /// Remove WM_PROTOCOLS so the window manager closes the client directly.
    /// Returns how many windows changed. SDL may set it again; call regularly.
    pub fn enable_close_button(&self) -> usize {
        let mut changed = 0;
        for &window in &self.windows {
            if self
                .property(window, self.wm_protocols, ANY_PROPERTY_TYPE)
                .is_some_and(|v| !v.is_empty())
            {
                unsafe { (self.x.delete_property)(self.conn, window, self.wm_protocols) };
                changed += 1;
            }
        }
        unsafe { (self.x.flush)(self.conn) };
        self.drain_events();
        changed
    }

    /// Ask the window manager (EWMH) to make each new game window fullscreen
    /// on its current monitor. Returns how many requests were sent.
    pub fn request_fullscreen(&mut self) -> usize {
        let mut sent = 0;
        for &window in &self.windows {
            if self.fullscreen_requested.contains(&window) {
                continue;
            }
            // xcb_client_message_event_t: type 33, format 32, sequence,
            // window, message type, then five 32-bit data words.
            let mut event = [0u8; 32];
            event[0] = 33;
            event[1] = 32;
            event[4..8].copy_from_slice(&window.to_ne_bytes());
            event[8..12].copy_from_slice(&self.net_wm_state.to_ne_bytes());
            for (i, word) in [1u32, self.net_wm_state_fullscreen, 0, 1, 0]
                .into_iter()
                .enumerate()
            {
                // _NET_WM_STATE_ADD, property, none, source = application.
                event[12 + i * 4..16 + i * 4].copy_from_slice(&word.to_ne_bytes());
            }
            const SUBSTRUCTURE_NOTIFY_AND_REDIRECT: u32 = (1 << 19) | (1 << 20);
            unsafe {
                (self.x.send_event)(
                    self.conn,
                    0,
                    self.root,
                    SUBSTRUCTURE_NOTIFY_AND_REDIRECT,
                    event.as_ptr(),
                )
            };
            self.fullscreen_requested.push(window);
            sent += 1;
        }
        if sent > 0 {
            unsafe { (self.x.flush)(self.conn) };
        }
        sent
    }

    fn focused_game(&self) -> bool {
        let mut error = std::ptr::null_mut();
        let cookie = unsafe { (self.x.get_input_focus)(self.conn) };
        let Some(reply) = take(
            unsafe { (self.x.get_input_focus_reply)(self.conn, cookie, &mut error) },
            error,
        ) else {
            return false;
        };
        let mut window = unsafe { (*reply).focus };
        release(reply);
        // 0 = None, 1 = PointerRoot. Walk up from SDL's child window.
        for _ in 0..6 {
            if window <= 1 || window == self.root {
                return false;
            }
            if self.windows.contains(&window) {
                return true;
            }
            match self.children(window) {
                Some((parent, _)) => window = parent,
                None => return false,
            }
        }
        false
    }

    fn escape_down(&self) -> bool {
        let mut error = std::ptr::null_mut();
        let cookie = unsafe { (self.x.query_keymap)(self.conn) };
        let Some(reply) = take(
            unsafe { (self.x.query_keymap_reply)(self.conn, cookie, &mut error) },
            error,
        ) else {
            return false;
        };
        let down = unsafe { (*reply).keys[ESCAPE_KEYCODE / 8] } & (1 << (ESCAPE_KEYCODE % 8)) != 0;
        release(reply);
        down
    }

    pub fn escape_in_game(&self) -> bool {
        !self.windows.is_empty() && self.escape_down() && self.focused_game()
    }

    pub fn alive(&self) -> bool {
        unsafe { (self.x.connection_has_error)(self.conn) == 0 }
    }

    /// The X keycode that produces `keysym` (unshifted or shifted column).
    fn keycode(&self, keysym: u32) -> Option<u8> {
        let setup = unsafe { (self.x.get_setup)(self.conn) }.cast::<u8>();
        // xcb_setup_t: min_keycode at byte 32, max_keycode at 33.
        let (min, max) = unsafe { (*setup.add(32), *setup.add(33)) };
        if max < min {
            return None;
        }
        let mut error = std::ptr::null_mut();
        let cookie = unsafe { (self.x.get_keyboard_mapping)(self.conn, min, max - min + 1) };
        let reply = take(
            unsafe { (self.x.get_keyboard_mapping_reply)(self.conn, cookie, &mut error) },
            error,
        )?;
        let per = usize::from(unsafe { (*reply).keysyms_per_keycode }).max(1);
        let length = unsafe { (self.x.get_keyboard_mapping_keysyms_length)(reply) }.max(0) as usize;
        let keysyms = unsafe {
            std::slice::from_raw_parts((self.x.get_keyboard_mapping_keysyms)(reply), length)
        };
        let found = keysyms
            .chunks(per)
            .position(|syms| syms.iter().take(2).any(|s| *s == keysym))
            .map(|i| min + i as u8);
        release(reply);
        found
    }
}

/// SDL 1.2 key symbol (as patched into the Aliens image) -> X keysym.
fn x_keysym(sdl: u32) -> Option<u32> {
    Some(match sdl {
        32..=126 => sdl,
        8 => 0xff08,
        9 => 0xff09,
        13 => 0xff0d,
        19 => 0xff13,
        127 => 0xffff,
        256..=265 => 0xffb0 + (sdl - 256),
        266 => 0xffae,
        267 => 0xffaf,
        268 => 0xffaa,
        269 => 0xffad,
        270 => 0xffab,
        271 => 0xff8d,
        273 => 0xff52,
        274 => 0xff54,
        275 => 0xff53,
        276 => 0xff51,
        277 => 0xff63,
        278 => 0xff50,
        279 => 0xff57,
        280 => 0xff55,
        281 => 0xff56,
        282..=296 => 0xffbe + (sdl - 282), // F1-F15
        303 => 0xffe2,
        304 => 0xffe1,
        305 => 0xffe4,
        306 => 0xffe3,
        307 => 0xffea,
        308 => 0xffe9,
        _ => return None,
    })
}

type FakeInput = unsafe extern "C" fn(Conn, u8, u8, u32, u32, i16, i16, u8) -> Cookie;

/// controls.cfg's default controller buttons for Aliens, as `sdlkey:mask`.
const DEFAULT_PAD_KEYS: &str = "49:16,49:4096,53:32,57:8192,273:1,274:2";
/// Controller 2 with two players: Start 2 and Coin 2 keys instead.
const DEFAULT_PAD_KEYS_P2: &str = "50:16,50:4096,54:32,57:8192,273:1,274:2";
/// Fire and secondary keys (q, w) for the player without the mouse: the
/// engine's desktop-mode player-two keys, or player one's in images where
/// player two has the mouse.
const Q_FIRE_KEY: u32 = 113;
const W_SECONDARY_KEY: u32 = 119;
/// Player one's fire and secondary keys while nobody has the system mouse
/// (SDL F13/F14; the preparer's `NO_MOUSE_FIRE_KEYS`).
const F13_FIRE_KEYS: [u32; 2] = [294, 295];
/// Pointer speed at full stick deflection, in screen pixels per second.
const POINTER_SPEED: i32 = 1200;
const KEY_PRESS: u8 = 2;
const BUTTON_PRESS: u8 = 4;
const MOTION_NOTIFY: u8 = 6;

/// Raw input record for a source, and the analog setter that scales a raw
/// value by the record's configured range (same engine as Jurassic Park).
const RAW_ANALOG: usize = 0x081b18d8;
const SET_ANALOG: usize = 0x081b1234;
const ANALOG_PROLOGUE: [u8; 6] = [0x55, 0x89, 0xe5, 0x83, 0xec, 0x10];
/// Mouse x/y sources, and each player's gun x/y sources. A two-player image
/// maps the gun sources to the aim of a player who aims with a controller
/// only (see the preparer's `TwoPlayer`).
const MOUSE_AXES: [i32; 2] = [0x144, 0x145];
const GUN_AXES: [[i32; 2]; 2] = [[0x181, 0x182], [0x183, 0x184]];
/// The game's analog input record (0x2c bytes): flags, scaled value, delta,
/// raw value, raw delta, then the min/max/range and output scale words.
type Analog = [u32; 11];

/// True if `len` bytes at `address` are readable in this process.
fn mapped(address: usize, len: usize) -> bool {
    let Ok(maps) = std::fs::read_to_string("/proc/self/maps") else {
        return false;
    };
    maps.lines().any(|line| {
        let mut fields = line.split_whitespace();
        let (Some(range), Some(perms)) = (fields.next(), fields.next()) else {
            return false;
        };
        let Some((start, end)) = range.split_once('-') else {
            return false;
        };
        let (Ok(start), Ok(end)) = (
            usize::from_str_radix(start, 16),
            usize::from_str_radix(end, 16),
        ) else {
            return false;
        };
        perms.starts_with('r') && start <= address && address + len <= end
    })
}

/// A player's controller aim, written into that player's gun records.
struct GunAim {
    axes: [i32; 2],
    get: unsafe extern "C" fn(i32) -> *mut Analog,
    set: unsafe extern "C" fn(*mut Analog, f32),
    cursor: crate::pad::Cursor,
    width: i32,
    height: i32,
}

impl GunAim {
    fn new(player: usize) -> Result<Self, String> {
        for address in [RAW_ANALOG, SET_ANALOG] {
            if !mapped(address, ANALOG_PROLOGUE.len())
                || unsafe { std::slice::from_raw_parts(address as *const u8, 6) } != ANALOG_PROLOGUE
            {
                return Err(format!("unexpected analog input code at {address:#x}"));
            }
        }
        let (width, height) = std::env::var("ARCADE_RESOLUTION")
            .ok()
            .and_then(|r| {
                let (w, h) = r.split_once('x')?;
                Some((w.parse::<i32>().ok()?, h.parse::<i32>().ok()?))
            })
            .filter(|&(w, h)| w > 0 && h > 0)
            .unwrap_or((1360, 768));
        Ok(Self {
            axes: GUN_AXES[player.min(1)],
            // Absolute addresses in the game image, not symbols of this
            // position-independent library: keep the calls indirect, or i686
            // folds them into relative calls that miss the image (SIGSEGV).
            get: unsafe {
                std::mem::transmute::<usize, unsafe extern "C" fn(i32) -> *mut Analog>(
                    std::hint::black_box(RAW_ANALOG),
                )
            },
            set: unsafe {
                std::mem::transmute::<usize, unsafe extern "C" fn(*mut Analog, f32)>(
                    std::hint::black_box(SET_ANALOG),
                )
            },
            cursor: crate::pad::Cursor::new(width / 2, height / 2),
            width,
            height,
        })
    }

    /// Moves the player's aim and stores it in window pixels, scaled exactly
    /// like the mouse: copy the mouse record's flags and ranges, then set the
    /// raw value through the game's own setter.
    fn update(&mut self, state: &crate::pad::Pad, moves: &crate::pad::Movement, elapsed: u32) {
        // A mouse or light gun of the player's own points directly.
        match state.pointer_moved(moves, self.width, self.height) {
            Some((x, y)) => self.cursor = crate::pad::Cursor::new(x, y),
            None => {
                self.cursor
                    .step(state.movement(moves), elapsed, POINTER_SPEED, self.width, self.height);
            }
        }
        let values = [self.cursor.x as f32, self.cursor.y as f32];
        for ((mouse, gun), value) in MOUSE_AXES.into_iter().zip(self.axes).zip(values) {
            let (mouse, gun) = unsafe { ((self.get)(mouse), (self.get)(gun)) };
            if mouse.is_null() || gun.is_null() {
                continue;
            }
            unsafe {
                let config = *mouse;
                let words = gun.cast::<u32>();
                for i in [0, 5, 6, 7, 8, 9, 10] {
                    words.add(i).write(config[i]);
                }
                (self.set)(gun, value);
            }
        }
    }
}

/// Controller input for Aliens, sent as XTest events to the focused game.
struct Controller {
    fake: FakeInput,
    reader: crate::evdev_pad::Reader,
    inputs: crate::pad::Inputs,
    /// (event type, keycode/button, pad mask per player, currently pressed)
    outputs: Vec<(u8, u8, [u32; crate::pad::MAX_PLAYERS], bool)>,
    cursor: crate::pad::Cursor,
    /// Controller-only aim per player (players without the mouse).
    guns: [Option<GunAim>; crate::pad::MAX_PLAYERS],
    /// Each player's aim axes (controls.cfg `move_x` / `move_y`).
    moves: [crate::pad::Movement; crate::pad::MAX_PLAYERS],
    last: Instant,
}

impl Controller {
    fn new(watcher: &Watcher) -> Result<Self, String> {
        let handle = unsafe {
            libc::dlopen(c"libxcb-xtest.so.0".as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL)
        };
        if handle.is_null() {
            return Err("libxcb-xtest.so.0 is unavailable".into());
        }
        let pointer = unsafe { libc::dlsym(handle, c"xcb_test_fake_input".as_ptr()) };
        if pointer.is_null() {
            return Err("libxcb-xtest lacks xcb_test_fake_input".into());
        }
        let fake: FakeInput = unsafe { std::mem::transmute::<*mut c_void, FakeInput>(pointer) };
        let inputs = crate::evdev_pad::inputs();
        let players = inputs.players;
        let mut outputs: Vec<(u8, u8, [u32; crate::pad::MAX_PLAYERS], bool)> = Vec::new();
        let mut add = |kind: u8, detail: u8, player: usize, mask: u32| {
            match outputs.iter_mut().find(|o| o.0 == kind && o.1 == detail) {
                Some(o) => o.2[player] |= mask,
                None => {
                    let mut masks = [0; crate::pad::MAX_PLAYERS];
                    masks[player] = mask;
                    outputs.push((kind, detail, masks, false));
                }
            }
        };
        let key_sets = [
            ("ARCADE_PAD_KEYS", DEFAULT_PAD_KEYS),
            ("ARCADE_PAD_KEYS_P2", DEFAULT_PAD_KEYS_P2),
        ];
        for (player, (name, defaults)) in key_sets.into_iter().take(players).enumerate() {
            let text = std::env::var(name).unwrap_or_else(|_| defaults.into());
            let mut pairs = [(0, 0); 64];
            let n = crate::pad::parse_bindings(text.as_bytes(), &mut pairs);
            for &(sdl, mask) in &pairs[..n] {
                match x_keysym(sdl).and_then(|k| watcher.keycode(k)) {
                    Some(keycode) => add(KEY_PRESS, keycode, player, mask),
                    None => eprintln!("[aliens] controller: no X key for SDL key {sdl}"),
                }
            }
        }
        // The system mouse's player fires with the mouse buttons (RT/LT click
        // them). The prepared image gives every other player fire keys of its
        // own: player two Q/W, player one Q/W or (nobody on the system mouse)
        // F13/F14; see the preparer's `sources`.
        if let Some(mouse) = inputs.mouse() {
            add(BUTTON_PRESS, 1, mouse, crate::pad::RT);
            add(BUTTON_PRESS, 3, mouse, crate::pad::LT);
        }
        let mut guns = [None, None];
        for (player, gun) in guns.iter_mut().enumerate() {
            if !inputs.own_aim(player) {
                continue;
            }
            let fire = if player == 0 && inputs.mouse().is_none() {
                F13_FIRE_KEYS
            } else {
                [Q_FIRE_KEY, W_SECONDARY_KEY]
            };
            for (sdl, mask) in fire.into_iter().zip([crate::pad::RT, crate::pad::LT]) {
                match x_keysym(sdl).and_then(|k| watcher.keycode(k)) {
                    Some(keycode) => add(KEY_PRESS, keycode, player, mask),
                    None => eprintln!("[aliens] player {}: no X key for SDL key {sdl}", player + 1),
                }
            }
            match GunAim::new(player) {
                Ok(aim) => {
                    eprintln!(
                        "[aliens] player {} aims with its {}",
                        player + 1,
                        inputs.methods[player].label()
                    );
                    *gun = Some(aim);
                }
                Err(error) => eprintln!("[aliens] player {} aim disabled: {error}", player + 1),
            }
        }
        Ok(Self {
            fake,
            reader: crate::evdev_pad::Reader::new(),
            inputs,
            outputs,
            cursor: crate::pad::Cursor::new(1 << 20, 1 << 20),
            guns,
            moves: [0, 1].map(|p| crate::evdev_pad::movement(p, crate::pad::Movement::STICKS)),
            last: Instant::now(),
        })
    }

    fn frame(&mut self, watcher: &Watcher) {
        let states = self.reader.read_players(&self.inputs);
        // A controller moves the system pointer only for the system mouse's player.
        let (pointer, pointer_moves) = self
            .inputs
            .mouse()
            .map_or((crate::pad::Pad::default(), crate::pad::Movement::STICKS), |m| (states[m], self.moves[m]));
        let elapsed = self.last.elapsed().as_millis() as u32;
        self.last = Instant::now();
        let any = states
            .iter()
            .zip(self.moves)
            .any(|(s, m)| s.buttons != 0 || s.movement(&m) != (0, 0) || s.pointer.is_some());
        let pressed = self.outputs.iter().any(|o| o.3);
        if !any && !pressed {
            return;
        }
        let focused = !watcher.windows.is_empty() && watcher.focused_game();
        if focused
            && states
                .iter()
                .any(|s| s.held(crate::pad::BACK) && s.held(crate::pad::START))
        {
            eprintln!("[aliens] Back + Start pressed; exiting");
            unsafe {
                libc::fflush(std::ptr::null_mut());
                libc::_exit(0);
            }
        }
        let mut sent = false;
        for ((gun, state), moves) in self.guns.iter_mut().zip(states).zip(self.moves) {
            if focused && let Some(gun) = gun {
                gun.update(&state, &moves, elapsed);
            }
        }
        for output in &mut self.outputs {
            let now = focused && states.iter().zip(output.2).any(|(s, mask)| s.held(mask));
            if now != output.3 {
                output.3 = now;
                let kind = if now { output.0 } else { output.0 + 1 };
                unsafe { (self.fake)(watcher.conn, kind, output.1, 0, 0, 0, 0, 0) };
                sent = true;
            }
        }
        let (old_x, old_y) = (self.cursor.x, self.cursor.y);
        if focused && self.cursor.step(pointer.movement(&pointer_moves), elapsed, POINTER_SPEED, 1 << 21, 1 << 21) {
            let (dx, dy) = (self.cursor.x - old_x, self.cursor.y - old_y);
            // Relative motion (detail 1). The virtual canvas keeps sub-pixel
            // remainders; recentre it only when it nears an edge.
            unsafe { (self.fake)(watcher.conn, MOTION_NOTIFY, 1, 0, 0, dx as i16, dy as i16, 0) };
            if (self.cursor.x - (1 << 20)).abs() > 1 << 19 || (self.cursor.y - (1 << 20)).abs() > 1 << 19 {
                self.cursor = crate::pad::Cursor::new(1 << 20, 1 << 20);
            }
            sent = true;
        }
        if sent {
            unsafe { (watcher.x.flush)(watcher.conn) };
        }
    }
}

/// Start the watcher thread for launcher-run Aliens sessions.
pub fn start() {
    if !std::env::var_os("ARCADE_ALIENS").is_some_and(|v| v == "1") {
        return;
    }
    let Some(class) = std::env::var("SDL_VIDEO_X11_WMCLASS")
        .ok()
        .filter(|c| !c.is_empty())
    else {
        eprintln!("[aliens] desktop exit disabled: SDL_VIDEO_X11_WMCLASS not set by the launcher");
        return;
    };
    let _ = std::thread::Builder::new()
        .name("arcade-exit".into())
        .spawn(move || {
            let mut watcher = match Watcher::connect(&class) {
                Ok(watcher) => watcher,
                Err(error) => {
                    eprintln!("[aliens] desktop exit disabled: {error}");
                    return;
                }
            };
            eprintln!("[aliens] desktop exit: Esc or the window close button");
            let mut controller = match Controller::new(&watcher) {
                Ok(controller) => Some(controller),
                Err(error) => {
                    eprintln!("[aliens] controllers disabled: {error}");
                    None
                }
            };
            let fullscreen = std::env::var_os("ARCADE_FULLSCREEN").is_some_and(|v| v == "1");
            let mut last_scan = Instant::now() - Duration::from_secs(10);
            let mut announced = false;
            while watcher.alive() {
                if last_scan.elapsed() >= Duration::from_secs(1) {
                    last_scan = Instant::now();
                    watcher.refresh();
                    if watcher.enable_close_button() > 0 && !announced {
                        announced = true;
                        eprintln!("[aliens] window close button enabled");
                    }
                    if fullscreen && watcher.request_fullscreen() > 0 {
                        eprintln!("[aliens] requested fullscreen on the window's monitor");
                    }
                }
                if watcher.escape_in_game() {
                    eprintln!("[aliens] Esc pressed; exiting");
                    unsafe {
                        libc::fflush(std::ptr::null_mut());
                        libc::_exit(0);
                    }
                }
                if let Some(controller) = controller.as_mut() {
                    controller.frame(&watcher);
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        });
}

// xcb reply structs are fixed-size X11 wire layouts.
const _: () = assert!(std::mem::size_of::<FocusReply>() == 12);
const _: () = assert!(std::mem::size_of::<TreeReply>() == 32);
const _: () = assert!(std::mem::size_of::<KeymapReply>() == 40);
const _: () = assert!(std::mem::size_of::<AtomReply>() == 12);
const _: () = assert!(std::mem::size_of::<PropertyReply>() == 32);
const _: () = assert!(std::mem::size_of::<KeyboardMappingReply>() == 32);
