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
    let project = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let bindings = arcade_core::controls::Bindings::load(&project.join("controls.cfg"))?;
    let value = std::env::var("ARCADE_BBH_KEYS").unwrap_or(bindings.env_value("big_buck")?);
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
                switch(index, down);
            }
        }
    }
}
pub unsafe fn release() {
    for index in 0..20 {
        switch(index, false);
    }
}
/// Last aimed position (mouse or controller), in 640x480 game coordinates.
static mut AIM_X: i32 = 320;
static mut AIM_Y: i32 = 240;
/// controls.cfg's default controller buttons for Big Buck.
const DEFAULT_PAD: &str = "5:16,7:32,256:131072,256:4096,257:65536,257:8192";
const PAD_FIRE: u32 = 0x100;
const PAD_PUMP: u32 = 0x101;
/// Crosshair speed at full stick deflection, in game pixels per second.
const CROSSHAIR_SPEED: i32 = 700;
struct Controller {
    bindings: Vec<(u32, u32)>,
    /// Pad-held state per bound target, for press/release edges.
    held: Vec<(u32, bool)>,
    last: std::time::Instant,
    connected: u32,
}
static mut CONTROLLER: Option<Controller> = None;

/// One message-loop pass: XInput crosshair, shots, pump and switches.
pub unsafe fn controller_frame(focused: bool) {
    let c = (*ptr::addr_of_mut!(CONTROLLER)).get_or_insert_with(|| {
        let text = std::env::var("ARCADE_PAD").unwrap_or_else(|_| DEFAULT_PAD.into());
        let mut bindings = vec![(0, 0); 128];
        let n = pad::parse_bindings(text.as_bytes(), &mut bindings);
        bindings.truncate(n);
        if !xinput::available() {
            eprintln!("[water] XInput unavailable; controllers disabled");
        }
        Controller {
            bindings,
            held: Vec::new(),
            last: std::time::Instant::now(),
            connected: 0,
        }
    });
    let state = if focused { xinput::read() } else { pad::Pad::default() };
    if focused && xinput::connected() != c.connected {
        c.connected = xinput::connected();
        eprintln!("[water] {} XInput controller(s) connected", c.connected);
    }
    if state.held(pad::BACK) && state.held(pad::START) {
        release();
        eprintln!("[water] Back + Start: exiting");
        std::process::exit(0);
    }
    let elapsed = c.last.elapsed().as_millis() as u32;
    c.last = std::time::Instant::now();
    let mut cursor = pad::Cursor::new(AIM_X, AIM_Y);
    if cursor.step(state.aim(), elapsed, CROSSHAIR_SPEED, 640, 480) {
        aim(cursor.x, cursor.y, false);
    }
    for &(target, _) in &c.bindings {
        if !c.held.iter().any(|h| h.0 == target) {
            c.held.push((target, false));
        }
    }
    let mut changes = Vec::new();
    for (target, was) in &mut c.held {
        let now = pad::target_held(&state, &c.bindings, *target);
        if now != *was {
            *was = now;
            changes.push((*target, now));
        }
    }
    for (target, down) in changes {
        match target {
            PAD_FIRE => {
                if down {
                    aim(cursor.x, cursor.y, true);
                }
                switch(1, down);
            }
            PAD_PUMP => switch(3, down),
            1 | 5 | 6 | 7 | 8 | 10 | 11 => switch(target as usize, down),
            _ => {}
        }
    }
}

/// Where the crosshair is drawn, in 640x480 game coordinates.
pub unsafe fn aim_position() -> (i32, i32) {
    (AIM_X, AIM_Y)
}

pub unsafe fn aim(x: i32, y: i32, shoot: bool) {
    AIM_X = x.clamp(0, 639);
    AIM_Y = y.clamp(0, 479);
    if CALLBACK == 0 {
        return;
    }
    let f: unsafe extern "C" fn(u32, i32, i32, i32, i32, u32, u32) -> i32 =
        std::mem::transmute(CALLBACK);
    let empty = [0u32];
    f(
        if shoot { 1 } else { 9 },
        x.clamp(0, 639),
        y.clamp(0, 479),
        255,
        255,
        empty.as_ptr() as u32,
        empty.as_ptr() as u32,
    );
    if shoot {
        eprintln!("[water] shot {x},{y}");
    }
}
pub unsafe fn mouse(button: i32, down: bool, x: i32, y: i32) {
    aim(x, y, button == 0 && down);
    switch(if button == 0 { 1 } else { 3 }, down);
}
