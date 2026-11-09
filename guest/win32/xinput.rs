//! XInput controllers (Xbox and compatible pads) for the 32-bit Windows
//! adapters: Super Cars and Ford hook DLLs and Water. Works on Windows and
//! under Wine, whose built-in XInput exposes Linux controllers.
//!
//! XInput is loaded on first use (never from DllMain). Up to four controllers
//! are merged into one [`Pad`]. Empty slots are probed once a second because
//! `XInputGetState` on a disconnected slot is slow.
//!
//! The including module must provide `pad` (banana-input's `pad.rs`) as a
//! sibling, either with `#[path]` or a `use`.
use super::pad::Pad;
use core::ptr;

#[cfg_attr(
    target_arch = "x86",
    link(name = "kernel32", kind = "raw-dylib", import_name_type = "undecorated")
)]
#[cfg_attr(not(target_arch = "x86"), link(name = "kernel32", kind = "raw-dylib"))]
unsafe extern "system" {
    #[link_name = "LoadLibraryA"]
    fn xinput_load_library(name: *const u8) -> isize;
    #[link_name = "GetProcAddress"]
    fn xinput_get_proc_address(module: isize, name: *const u8) -> *const u8;
    #[link_name = "GetTickCount"]
    fn xinput_get_tick_count() -> u32;
}

#[repr(C)]
#[derive(Default)]
struct State {
    packet: u32,
    buttons: u16,
    lt: u8,
    rt: u8,
    lx: i16,
    ly: i16,
    rx: i16,
    ry: i16,
}

type GetState = unsafe extern "system" fn(u32, *mut State) -> u32;

struct Reader {
    tried: bool,
    get_state: usize,
    connected: [bool; 4],
    next_probe: u32,
}

static mut READER: Reader = Reader {
    tried: false,
    get_state: 0,
    connected: [false; 4],
    next_probe: 0,
};

fn reader() -> &'static mut Reader {
    // Each adapter polls from a single game/message thread.
    unsafe { &mut *ptr::addr_of_mut!(READER) }
}

/// Loads XInput if needed. Returns false if no XInput DLL is available.
pub fn available() -> bool {
    let r = reader();
    if !r.tried {
        r.tried = true;
        for name in [
            &b"xinput1_4.dll\0"[..],
            &b"xinput1_3.dll\0"[..],
            &b"xinput9_1_0.dll\0"[..],
        ] {
            let module = unsafe { xinput_load_library(name.as_ptr()) };
            if module == 0 {
                continue;
            }
            let f = unsafe { xinput_get_proc_address(module, b"XInputGetState\0".as_ptr()) };
            if !f.is_null() {
                r.get_state = f as usize;
                break;
            }
        }
    }
    r.get_state != 0
}

/// Number of controllers seen on the last [`read`].
pub fn connected() -> u32 {
    reader().connected.iter().filter(|c| **c).count() as u32
}

/// Connected controllers per player (`pad::Inputs::device_player`, in slot
/// order). Nothing is read while no player uses XInput.
pub fn read_players(inputs: &super::pad::Inputs) -> [Pad; super::pad::MAX_PLAYERS] {
    let mut out = [Pad::default(); super::pad::MAX_PLAYERS];
    if inputs.device_player(super::pad::Device::XInput, 0).is_none() || !available() {
        return out;
    }
    let r = reader();
    let get: GetState = unsafe { core::mem::transmute(r.get_state) };
    let now = unsafe { xinput_get_tick_count() };
    let probe = now.wrapping_sub(r.next_probe) as i32 >= 0;
    if probe {
        r.next_probe = now.wrapping_add(1000);
    }
    let mut index = 0;
    for slot in 0..4 {
        if !r.connected[slot] && !probe {
            continue;
        }
        let mut state = State::default();
        let result = unsafe { get(slot as u32, &mut state) };
        r.connected[slot] = result == 0;
        if result == 0 {
            let Some(player) = inputs.device_player(super::pad::Device::XInput, index) else {
                continue;
            };
            index += 1;
            out[player] = out[player].merge(Pad::from_xinput(
                state.buttons,
                state.lt,
                state.rt,
                state.lx,
                state.ly,
                state.rx,
                state.ry,
            ));
        }
    }
    out
}
