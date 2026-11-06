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
pub unsafe fn aim(x: i32, y: i32, shoot: bool) {
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
