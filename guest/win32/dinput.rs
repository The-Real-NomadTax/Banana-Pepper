//! DirectInput 8 game controllers (generic gamepads, arcade sticks, wheels)
//! for the 32-bit Windows adapters: the Super Cars and Ford hook DLLs and
//! Water. Works on Windows and under Wine, whose DirectInput exposes Linux
//! joysticks.
//!
//! The system directory's `dinput8.dll` is loaded on first use (never from
//! DllMain; the Super Cars adapter itself is the game's `dinput8.dll`). Attached game
//! controllers are enumerated every few seconds; each is read with the
//! standard `DIJOYSTATE` layout (built here, as `c_dfDIJoystick` is static
//! data in dinput8.lib). Numbered buttons follow [`pad::DINPUT_BUTTONS`]
//! (button 1 = A, ...); X/Y is the left stick, Z/Rz the right stick, POV 0
//! the d-pad. Controllers named as Xbox controllers are left to XInput, so a
//! pad is never read twice.
//!
//! No `std` use; the including module provides `pad` as a sibling.
use super::pad::{self, Pad};
use core::{ffi::c_void, ptr};

#[link(
    name = "kernel32",
    kind = "raw-dylib",
    import_name_type = "undecorated"
)]
unsafe extern "system" {
    #[link_name = "LoadLibraryA"]
    fn dinput_load_library(name: *const u8) -> isize;
    #[link_name = "GetProcAddress"]
    fn dinput_get_proc_address(module: isize, name: *const u8) -> *const u8;
    #[link_name = "GetModuleHandleA"]
    fn dinput_get_module_handle(name: *const u8) -> isize;
    #[link_name = "GetTickCount"]
    fn dinput_get_tick_count() -> u32;
    #[link_name = "GetSystemDirectoryA"]
    fn dinput_get_system_directory(buffer: *mut u8, size: u32) -> u32;
}

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
struct Guid(u32, u16, u16, [u8; 8]);

const fn axis_guid(d1: u32) -> Guid {
    Guid(d1, 0xc9f3, 0x11cf, [0xbf, 0xc7, 0x44, 0x45, 0x53, 0x54, 0x00, 0x00])
}
static IID_IDIRECTINPUT8A: Guid = Guid(
    0xbf79_8030,
    0x483a,
    0x4da2,
    [0xaa, 0x99, 0x5d, 0x64, 0xed, 0x36, 0x97, 0x00],
);
static GUID_X: Guid = axis_guid(0xa36d_02e0);
static GUID_Y: Guid = axis_guid(0xa36d_02e1);
static GUID_Z: Guid = axis_guid(0xa36d_02e2);
static GUID_RX: Guid = axis_guid(0xa36d_02f4);
static GUID_RY: Guid = axis_guid(0xa36d_02f5);
static GUID_RZ: Guid = axis_guid(0xa36d_02e3);
static GUID_SLIDER: Guid = axis_guid(0xa36d_02e4);
static GUID_POV: Guid = axis_guid(0xa36d_02f2);

#[repr(C)]
struct ObjectFormat {
    guid: *const Guid,
    offset: u32,
    kind: u32,
    flags: u32,
}
#[repr(C)]
struct DataFormat {
    size: u32,
    object_size: u32,
    flags: u32,
    data_size: u32,
    count: u32,
    objects: *const ObjectFormat,
}

/// `DIJOYSTATE`: 6 axes, 2 sliders, 4 POVs, 32 buttons (80 bytes).
#[repr(C)]
#[derive(Clone, Copy)]
struct JoyState {
    axes: [i32; 6],
    sliders: [i32; 2],
    povs: [u32; 4],
    buttons: [u8; 32],
}

const DIDFT_OPTIONAL: u32 = 0x8000_0000;
const DIDFT_ANYINSTANCE: u32 = 0x00ff_ff00;
const DIDFT_AXIS: u32 = 0x03;
const DIDFT_BUTTON: u32 = 0x0c;
const DIDFT_POV: u32 = 0x10;
const DIDOI_ASPECTPOSITION: u32 = 0x100;
const DIDF_ABSAXIS: u32 = 1;
const DI8DEVCLASS_GAMECTRL: u32 = 4;
const DIEDFL_ATTACHEDONLY: u32 = 1;
const MAX_DEVICES: usize = 8;

/// The objects of `c_dfDIJoystick`, as in Windows' dinput8.lib.
fn objects() -> [ObjectFormat; 44] {
    let axis = |guid: &'static Guid, offset| ObjectFormat {
        guid,
        offset,
        kind: DIDFT_OPTIONAL | DIDFT_AXIS | DIDFT_ANYINSTANCE,
        flags: DIDOI_ASPECTPOSITION,
    };
    let mut list: [ObjectFormat; 44] = unsafe { core::mem::zeroed() };
    for (i, guid) in [&GUID_X, &GUID_Y, &GUID_Z, &GUID_RX, &GUID_RY, &GUID_RZ, &GUID_SLIDER, &GUID_SLIDER]
        .into_iter()
        .enumerate()
    {
        list[i] = axis(guid, 4 * i as u32);
    }
    for i in 0..4 {
        list[8 + i] = ObjectFormat {
            guid: &GUID_POV,
            offset: 32 + 4 * i as u32,
            kind: DIDFT_OPTIONAL | DIDFT_POV | DIDFT_ANYINSTANCE,
            flags: 0,
        };
    }
    for i in 0..32 {
        list[12 + i] = ObjectFormat {
            guid: ptr::null(),
            offset: 48 + i as u32,
            kind: DIDFT_OPTIONAL | DIDFT_BUTTON | DIDFT_ANYINSTANCE,
            flags: 0,
        };
    }
    list
}

type Com = *mut *const usize;

/// Calls vtable slot `index` of a COM object.
unsafe fn method(object: Com, index: usize) -> usize {
    unsafe { *(*object).add(index) }
}

struct Reader {
    tried: bool,
    input: Com,
    devices: [Option<(Guid, Com)>; MAX_DEVICES],
    found: [Option<Guid>; MAX_DEVICES],
    next_scan: u32,
    format: Option<([ObjectFormat; 44], DataFormat)>,
}

static mut READER: Reader = Reader {
    tried: false,
    input: ptr::null_mut(),
    devices: [None; MAX_DEVICES],
    found: [None; MAX_DEVICES],
    next_scan: 0,
    format: None,
};

fn reader() -> &'static mut Reader {
    // Each adapter polls from a single game/message thread.
    unsafe { &mut *ptr::addr_of_mut!(READER) }
}

/// Loads DirectInput 8 if needed. Returns false if it is unavailable.
pub fn available() -> bool {
    let r = reader();
    if !r.tried {
        r.tried = true;
        let mut path = [0u8; 300];
        let n = unsafe { dinput_get_system_directory(path.as_mut_ptr(), 260) } as usize;
        let name = b"\\dinput8.dll\0";
        let module = if n == 0 || n > 260 {
            0
        } else {
            path[n..n + name.len()].copy_from_slice(name);
            unsafe { dinput_load_library(path.as_ptr()) }
        };
        let create = if module == 0 {
            ptr::null()
        } else {
            unsafe { dinput_get_proc_address(module, b"DirectInput8Create\0".as_ptr()) }
        };
        if !create.is_null() {
            type Create = unsafe extern "system" fn(isize, u32, *const Guid, *mut Com, *mut c_void) -> i32;
            let create: Create = unsafe { core::mem::transmute(create) };
            let instance = unsafe { dinput_get_module_handle(ptr::null()) };
            let mut input: Com = ptr::null_mut();
            if unsafe { create(instance, 0x0800, &IID_IDIRECTINPUT8A, &mut input, ptr::null_mut()) } >= 0 {
                r.input = input;
            }
        }
    }
    !r.input.is_null()
}

/// `DIDEVICEINSTANCEA`: size, instance GUID, product GUID, type, instance
/// name[260], product name[260], ...
unsafe extern "system" fn found(instance: *const u8, _context: *mut c_void) -> i32 {
    let guid = unsafe { ptr::read_unaligned(instance.add(4).cast::<Guid>()) };
    let name = unsafe { core::slice::from_raw_parts(instance.add(300), 260) };
    let name = &name[..name.iter().position(|&b| b == 0).unwrap_or(name.len())];
    if is_xinput_name(name) {
        return 1; // DIENUM_CONTINUE: left to XInput
    }
    let r = reader();
    if let Some(slot) = r.found.iter_mut().find(|s| s.is_none()) {
        *slot = Some(guid);
    }
    1
}

/// Xbox controllers also appear in DirectInput; XInput reads those.
fn is_xinput_name(name: &[u8]) -> bool {
    name.windows(4).any(|w| w.eq_ignore_ascii_case(b"xbox"))
        || name.windows(6).any(|w| w.eq_ignore_ascii_case(b"xinput"))
}

fn scan(r: &mut Reader) {
    r.found = [None; MAX_DEVICES];
    type Enum = unsafe extern "system" fn(Com, u32, unsafe extern "system" fn(*const u8, *mut c_void) -> i32, *mut c_void, u32) -> i32;
    let enumerate: Enum = unsafe { core::mem::transmute(method(r.input, 4)) };
    unsafe { enumerate(r.input, DI8DEVCLASS_GAMECTRL, found, ptr::null_mut(), DIEDFL_ATTACHEDONLY) };
    let format = r.format.get_or_insert_with(|| {
        let objects = objects();
        let format = DataFormat {
            size: 24,
            object_size: 16,
            flags: DIDF_ABSAXIS,
            data_size: 80,
            count: 44,
            objects: ptr::null(),
        };
        (objects, format)
    });
    format.1.objects = format.0.as_ptr();
    let format_ptr: *const DataFormat = &format.1;
    for guid in r.found.iter().flatten() {
        if r.devices.iter().flatten().any(|(g, _)| g == guid) {
            continue;
        }
        let Some(slot) = r.devices.iter_mut().find(|d| d.is_none()) else {
            break;
        };
        type Create = unsafe extern "system" fn(Com, *const Guid, *mut Com, *mut c_void) -> i32;
        let create: Create = unsafe { core::mem::transmute(method(r.input, 3)) };
        let mut device: Com = ptr::null_mut();
        if unsafe { create(r.input, guid, &mut device, ptr::null_mut()) } < 0 || device.is_null() {
            continue;
        }
        type SetFormat = unsafe extern "system" fn(Com, *const DataFormat) -> i32;
        type Simple = unsafe extern "system" fn(Com) -> i32;
        let set_format: SetFormat = unsafe { core::mem::transmute(method(device, 11)) };
        let acquire: Simple = unsafe { core::mem::transmute(method(device, 7)) };
        if unsafe { set_format(device, format_ptr) } < 0 {
            let release: Simple = unsafe { core::mem::transmute(method(device, 2)) };
            unsafe { release(device) };
            continue;
        }
        unsafe { acquire(device) };
        *slot = Some((*guid, device));
    }
}

/// Number of DirectInput controllers being read.
pub fn connected() -> u32 {
    reader().devices.iter().flatten().count() as u32
}

/// A `DIJOYSTATE` as a [`Pad`].
fn to_pad(state: &JoyState) -> Pad {
    let axis = |v: i32| (v.clamp(0, 65535) - 32768) as i16;
    let invert = |v: i16| (-(i32::from(v))).clamp(-32768, 32767) as i16;
    let mut buttons = 0;
    for (i, &mask) in pad::DINPUT_BUTTONS.iter().enumerate() {
        if state.buttons[i] & 0x80 != 0 {
            buttons |= mask;
        }
    }
    // POV in hundredths of a degree clockwise from up; centred = 0xffff.
    let pov = state.povs[0];
    if pov & 0xffff != 0xffff {
        let angle = pov % 36000;
        if !(6750..=29250).contains(&angle) {
            buttons |= pad::UP;
        }
        if (2250..15750).contains(&angle) {
            buttons |= pad::RIGHT;
        }
        if (11250..24750).contains(&angle) {
            buttons |= pad::DOWN;
        }
        if (20250..33750).contains(&angle) {
            buttons |= pad::LEFT;
        }
    }
    let mut pad = Pad {
        buttons,
        lx: axis(state.axes[0]),
        ly: invert(axis(state.axes[1])),
        rx: axis(state.axes[2]),
        ry: invert(axis(state.axes[5])),
        lt: if buttons & pad::LT != 0 { 255 } else { 0 },
        rt: if buttons & pad::RT != 0 { 255 } else { 0 },
        pointer: None,
    };
    pad.update_trigger_bits();
    pad
}

/// DirectInput controllers per player (`pad::Inputs::device_player`, in
/// enumeration order). Nothing is loaded while no player uses DirectInput.
pub fn read_players(inputs: &pad::Inputs) -> [Pad; pad::MAX_PLAYERS] {
    let mut out = [Pad::default(); pad::MAX_PLAYERS];
    if inputs.device_player(pad::Device::DirectInput, 0).is_none() || !available() {
        return out;
    }
    let r = reader();
    let now = unsafe { dinput_get_tick_count() };
    if now.wrapping_sub(r.next_scan) as i32 >= 0 {
        r.next_scan = now.wrapping_add(3000);
        scan(r);
    }
    type Simple = unsafe extern "system" fn(Com) -> i32;
    type GetState = unsafe extern "system" fn(Com, u32, *mut JoyState) -> i32;
    let mut index = 0;
    for slot in r.devices.iter_mut() {
        let Some((_, device)) = *slot else {
            continue;
        };
        let poll: Simple = unsafe { core::mem::transmute(method(device, 25)) };
        let acquire: Simple = unsafe { core::mem::transmute(method(device, 7)) };
        let get: GetState = unsafe { core::mem::transmute(method(device, 9)) };
        let mut state: JoyState = unsafe { core::mem::zeroed() };
        unsafe { poll(device) };
        let mut result = unsafe { get(device, 80, &mut state) };
        if result < 0 {
            // Lost or not acquired: try once more, then drop the device.
            unsafe { acquire(device) };
            result = unsafe { get(device, 80, &mut state) };
        }
        if result < 0 {
            let release: Simple = unsafe { core::mem::transmute(method(device, 2)) };
            unsafe { release(device) };
            *slot = None;
            continue;
        }
        if let Some(player) = inputs.device_player(pad::Device::DirectInput, index) {
            out[player] = out[player].merge(to_pad(&state));
        }
        index += 1;
    }
    out
}
