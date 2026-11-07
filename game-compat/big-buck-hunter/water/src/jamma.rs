use super::*;

static mut COUNTERS: [u32; 20] = [0; 20];
static mut HELD: u32 = 0;
static mut BITS: u32 = 0;
static mut DIRTY: u32 = 0;
static mut CALLBACK: usize = 0;
static mut SEEN: u64 = 0;
static KEYS: OnceLock<Vec<(u8, usize)>> = OnceLock::new();

unsafe extern "C" fn success() -> i32 {
    0
}
#[unsafe(naked)]
unsafe extern "C" fn op() -> i32 {
    core::arch::naked_asm!("lea eax,[esp+4]", "push eax", "call {}", "add esp,4", "ret", sym dispatch);
}
unsafe extern "C" fn dispatch(args: *const u32) -> i32 {
    let op = *args;
    let code = op & 255;
    if code < 64 && SEEN & (1 << code) == 0 {
        SEEN |= 1 << code;
        eprintln!("[water] JAMMA op {op:#x}");
    }
    let arg = |n| *args.add(n);
    match code {
        1 => 0x30001,
        2 => {
            if arg(1) != 0 {
                *(arg(1) as *mut u32) = (&raw const BITS) as u32;
            }
            0
        }
        3 => 0,
        6 => {
            CALLBACK = arg(1) as usize;
            0
        }
        0x12 => {
            if arg(1) != 0 {
                *(arg(1) as *mut u32) = DIRTY;
            }
            if arg(2) != 0 {
                *(arg(2) as *mut u32) = (&raw const COUNTERS) as u32;
            }
            if op & 0x100 != 0 {
                DIRTY = 0;
            }
            0
        }
        4 | 5 | 0xb | 0xd | 0x11 | 0x14 | 0x15 | 0x17 | 0x18 | 0x1f | 0x20 => 0,
        _ => -9000,
    }
}
pub unsafe fn install() -> Result<(), String> {
    let hooks = [
        (0x080d9ce5usize, success as *const () as usize),
        (0x080d9e81, success as *const () as usize),
        (0x080dbf32, success as *const () as usize),
        (0x080da943, op as *const () as usize),
    ];
    for &(address, _) in &hooks {
        if std::slice::from_raw_parts(address as *const u8, 3) != [0x55, 0x89, 0xe5] {
            return Err(format!("JAMMA prologue mismatch at {address:#x}"));
        }
    }
    for (address, target) in hooks {
        let mut patch = [0xb8, 0, 0, 0, 0, 0xff, 0xe0];
        patch[1..5].copy_from_slice(&(target as u32).to_le_bytes());
        ptr::copy_nonoverlapping(patch.as_ptr(), address as *mut u8, patch.len());
    }
    // The launcher passes the game's bindings; Water reads no game config.
    let value = std::env::var("ARCADE_BBH_KEYS")
        .map_err(|_| "ARCADE_BBH_KEYS must carry the switch bindings")?;
    let keys = value
        .split(',')
        .filter(|s| !s.is_empty())
        .map(|pair| {
            let (switch, key) = pair.split_once(':').ok_or("Bad Big Buck binding")?;
            Ok((
                key.parse::<u8>()
                    .map_err(|_| "Bad ASCII key")?
                    .to_ascii_lowercase(),
                switch.parse::<usize>().map_err(|_| "Bad switch")?,
            ))
        })
        .collect::<Result<Vec<_>, String>>()?;
    KEYS.set(keys).map_err(|_| "Keys already set")?;
    eprintln!("[water] virtual JAMMA installed; keyboard and mouse use Windows messages");
    Ok(())
}
unsafe fn switch(index: usize, down: bool) {
    if index >= 20 {
        return;
    }
    let bit = 1 << index;
    if (HELD & bit != 0) == down {
        return;
    }
    if down {
        HELD |= bit;
    } else {
        HELD &= !bit;
    }
    COUNTERS[index] = COUNTERS[index].wrapping_add(1);
    BITS = HELD >> 1;
    DIRTY = 1;
    eprintln!(
        "[water] switch {index} {}",
        if down { "down" } else { "up" }
    );
}
pub unsafe fn key(key: u8, down: bool) {
    if let Some(keys) = KEYS.get() {
        for &(bound, index) in keys {
            if bound == key.to_ascii_lowercase() {
                switch(follow_mouse(index), down);
            }
        }
    }
}
pub unsafe fn release() {
    for index in 0..20 {
        switch(index, false);
    }
}
/// Last aimed position per gun (mouse or controller), in 640x480 game
/// coordinates. The mouse always aims gun one.
static mut AIM: [(i32, i32); pad::MAX_PLAYERS] = [(320, 240); pad::MAX_PLAYERS];
/// controls.cfg's default controller buttons for Big Buck.
const DEFAULT_PAD: &str = "5:16,7:32,256:131072,256:4096,257:65536,257:8192";
/// Controller 2 with two players: the same buttons on player two's targets.
const DEFAULT_PAD_P2: &str = "6:16,8:32,258:131072,258:4096,259:65536,259:8192";
const PAD_FIRE: u32 = 0x100;
const PAD_PUMP: u32 = 0x101;
const PAD_FIRE_P2: u32 = 0x102;
const PAD_PUMP_P2: u32 = 0x103;
/// Switch-table indices for gun two (`g21` trigger, `g22` pump).
const GUN2_TRIGGER: usize = 2;
const GUN2_PUMP: usize = 4;
/// Crosshair speed at full stick deflection, in game pixels per second.
const CROSSHAIR_SPEED: i32 = 700;
struct Controller {
    inputs: pad::Inputs,
    /// Each player's controller buttons (controls.cfg; `[big_buck.p2]`).
    bindings: [Vec<(u32, u32)>; pad::MAX_PLAYERS],
    /// Each player's aim axes (controls.cfg `move_x` / `move_y`).
    moves: [pad::Movement; pad::MAX_PLAYERS],
    /// Pad-held state per bound target, for press/release edges.
    held: Vec<(u32, bool)>,
    last: std::time::Instant,
    connected: u32,
}
static mut CONTROLLER: Option<Controller> = None;

fn parse_pad(name: &str, defaults: &str) -> Vec<(u32, u32)> {
    let text = std::env::var(name).unwrap_or_else(|_| defaults.into());
    let mut bindings = vec![(0, 0); 128];
    let n = pad::parse_bindings(text.as_bytes(), &mut bindings);
    bindings.truncate(n);
    bindings
}

/// Who plays with which device (settings.cfg `players`, `p1_input`,
/// `p2_input`, via ARCADE_PLAYERS and ARCADE_INPUTS).
pub fn inputs() -> pad::Inputs {
    static INPUTS: OnceLock<pad::Inputs> = OnceLock::new();
    *INPUTS.get_or_init(|| {
        let var = |name| std::env::var(name).ok();
        let (players, inputs) = (var("ARCADE_PLAYERS"), var("ARCADE_INPUTS"));
        pad::Inputs::parse(
            players.as_deref().map(str::as_bytes),
            inputs.as_deref().map(str::as_bytes),
        )
    })
}

/// Stick-aim mode for this player's controller. Pointer devices stay direct.
pub fn aim_mode(player: usize) -> pad::AimMode {
    static MODES: OnceLock<[pad::AimMode; 2]> = OnceLock::new();
    let modes = MODES.get_or_init(|| {
        let text = std::env::var("ARCADE_AIM_MODES").ok();
        [0, 1].map(|p| pad::AimMode::for_player(text.as_deref().map(str::as_bytes), p))
    });
    match inputs().methods[player.min(1)] {
        pad::Method::XInput | pad::Method::DirectInput => modes[player.min(1)],
        _ => pad::AimMode::Cursor,
    }
}

/// Players in this session.
pub fn players() -> usize {
    inputs().players
}

/// Gun one's trigger and pump switches belong to whoever has the mouse:
/// with player two on keyboard and mouse they become gun two's.
fn follow_mouse(index: usize) -> usize {
    match (index, inputs().mouse()) {
        (1, Some(1)) => GUN2_TRIGGER,
        (3, Some(1)) => GUN2_PUMP,
        _ => index,
    }
}

/// One message-loop pass: XInput crosshairs, shots, pumps and switches.
pub unsafe fn controller_frame(focused: bool) {
    let c = (*ptr::addr_of_mut!(CONTROLLER)).get_or_insert_with(|| {
        if inputs().uses(pad::Method::XInput) && !xinput::available() {
            eprintln!("[water] XInput unavailable; controllers disabled");
        }
        if players() > 1 {
            eprintln!(
                "[water] two players ({}, {}); the system mouse aims {}",
                inputs().methods[0].label(),
                inputs().methods[1].label(),
                inputs()
                    .mouse()
                    .map_or("no gun".to_owned(), |g| format!("gun {}", g + 1))
            );
        }
        Controller {
            inputs: inputs(),
            bindings: [
                parse_pad("ARCADE_PAD", DEFAULT_PAD),
                parse_pad("ARCADE_PAD_P2", DEFAULT_PAD_P2),
            ],
            moves: ["ARCADE_MOVE", "ARCADE_MOVE_P2"].map(|name| {
                let text = std::env::var(name).ok();
                pad::Movement::parse(text.as_deref().map(str::as_bytes), pad::Movement::STICKS)
            }),
            held: Vec::new(),
            last: std::time::Instant::now(),
            connected: 0,
        }
    });
    let states = if focused {
        let [c1, c2] = controllers::read_players(&c.inputs);
        let [r1, r2] = rawinput::read_players(&c.inputs);
        [c1.merge(r1), c2.merge(r2)]
    } else {
        [pad::Pad::default(); pad::MAX_PLAYERS]
    };
    if focused && controllers::connected() != c.connected {
        c.connected = controllers::connected();
        eprintln!(
            "[water] {} controller(s) connected (XInput + DirectInput)",
            c.connected
        );
    }
    if states
        .iter()
        .any(|s| s.held(pad::BACK) && s.held(pad::START))
    {
        release();
        eprintln!("[water] Back + Start: exiting");
        std::process::exit(0);
    }
    let elapsed = c.last.elapsed().as_millis() as u32;
    c.last = std::time::Instant::now();
    let mut cursors = [(0, 0); pad::MAX_PLAYERS];
    for gun in 0..c.inputs.players {
        let (x, y) = aim_position(gun);
        let mut cursor = pad::Cursor::new(x, y);
        // A mouse or light gun of the player's own points directly.
        if let Some((px, py)) = states[gun].pointer_moved(&c.moves[gun], 640, 480) {
            if (px, py) != (x, y) {
                cursor = pad::Cursor::new(px, py);
                aim_gun(gun, px, py, false);
            }
        } else if cursor.step_mode(
            aim_mode(gun),
            states[gun].movement(&c.moves[gun]),
            elapsed,
            CROSSHAIR_SPEED,
            640,
            480,
        ) {
            aim_gun(gun, cursor.x, cursor.y, false);
        }
        cursors[gun] = (cursor.x, cursor.y);
    }
    for bindings in &c.bindings[..c.inputs.players] {
        for &(target, _) in bindings {
            if !c.held.iter().any(|h| h.0 == target) {
                c.held.push((target, false));
            }
        }
    }
    let mut changes = Vec::new();
    for (target, was) in &mut c.held {
        // A target shared by both players is held while either holds it.
        let now =
            (0..c.inputs.players).any(|p| pad::target_held(&states[p], &c.bindings[p], *target));
        if now != *was {
            *was = now;
            changes.push((*target, now));
        }
    }
    for (target, down) in changes {
        match target {
            PAD_FIRE | PAD_FIRE_P2 => {
                let gun = usize::from(target == PAD_FIRE_P2);
                if down {
                    aim_gun(gun, cursors[gun].0, cursors[gun].1, true);
                }
                switch(if gun == 0 { 1 } else { GUN2_TRIGGER }, down);
            }
            PAD_PUMP => switch(3, down),
            PAD_PUMP_P2 => switch(GUN2_PUMP, down),
            1 | 2 | 5 | 6 | 7 | 8 | 10 | 11 => switch(target as usize, down),
            _ => {}
        }
    }
}

/// Where a gun's crosshair is drawn, in 640x480 game coordinates.
pub unsafe fn aim_position(gun: usize) -> (i32, i32) {
    (*ptr::addr_of!(AIM))[gun.min(pad::MAX_PLAYERS - 1)]
}

/// The mouse's aim: gun one, or gun two when player two has the mouse.
pub unsafe fn aim(x: i32, y: i32, shoot: bool) {
    if let Some(gun) = inputs().mouse() {
        aim_gun(gun, x, y, shoot);
    }
}

/// Shot (1) or move (9), with the gun number (0 = gun one) in bits 8-11 as
/// the game's gun callback decodes it.
unsafe fn aim_gun(gun: usize, x: i32, y: i32, shoot: bool) {
    let gun = gun.min(pad::MAX_PLAYERS - 1);
    let (x, y) = (x.clamp(0, 639), y.clamp(0, 479));
    (*ptr::addr_of_mut!(AIM))[gun] = (x, y);
    if CALLBACK == 0 {
        return;
    }
    let f: unsafe extern "C" fn(u32, i32, i32, i32, i32, u32, u32) -> i32 =
        std::mem::transmute(CALLBACK);
    let empty = [0u32];
    f(
        (if shoot { 1 } else { 9 }) | ((gun as u32) << 8),
        x,
        y,
        255,
        255,
        empty.as_ptr() as u32,
        empty.as_ptr() as u32,
    );
    if shoot {
        eprintln!("[water] gun {} shot {x},{y}", gun + 1);
    }
}
pub unsafe fn mouse(button: i32, down: bool, x: i32, y: i32) {
    if inputs().mouse().is_none() {
        return; // every mouse and light gun is read through Raw Input
    }
    aim(x, y, button == 0 && down);
    switch(follow_mouse(if button == 0 { 1 } else { 3 }), down);
}
