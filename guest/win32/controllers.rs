//! Every game controller a session's input methods read, for the 32-bit
//! Windows adapters: XInput (`xinput.rs`) and DirectInput (`dinput.rs`),
//! merged per player by [`pad::Inputs`] (`ARCADE_PLAYERS`, `ARCADE_INPUTS`
//! from settings.cfg). Water adds Raw Input mice itself.
//!
//! No `std` use; the including module provides `pad`, `xinput` and `dinput`
//! as siblings.
use super::pad::{self, Pad};
use core::ptr;

#[cfg_attr(
    target_arch = "x86",
    link(name = "kernel32", kind = "raw-dylib", import_name_type = "undecorated")
)]
#[cfg_attr(not(target_arch = "x86"), link(name = "kernel32", kind = "raw-dylib"))]
unsafe extern "system" {
    #[link_name = "GetEnvironmentVariableA"]
    fn controllers_get_env(name: *const u8, value: *mut u8, size: u32) -> u32;
}

static mut INPUTS: Option<pad::Inputs> = None;

/// An environment variable's bytes (at most 63), if set.
fn env<'a>(name: &[u8], buffer: &'a mut [u8; 64]) -> Option<&'a [u8]> {
    let n = unsafe { controllers_get_env(name.as_ptr(), buffer.as_mut_ptr(), buffer.len() as u32) };
    (n > 0 && (n as usize) < buffer.len()).then(|| &buffer[..n as usize])
}

/// Who plays with which device (settings.cfg `players`, `p1_input`,
/// `p2_input`), read once.
pub fn inputs() -> pad::Inputs {
    // Each adapter polls from a single game/message thread.
    let cached = unsafe { &mut *ptr::addr_of_mut!(INPUTS) };
    *cached.get_or_insert_with(|| {
        let (mut players, mut inputs) = ([0u8; 64], [0u8; 64]);
        pad::Inputs::parse(
            env(b"ARCADE_PLAYERS\0", &mut players),
            env(b"ARCADE_INPUTS\0", &mut inputs),
        )
    })
}

/// Each player's XInput and DirectInput controllers.
pub fn read_players(inputs: &pad::Inputs) -> [Pad; pad::MAX_PLAYERS] {
    let x = super::xinput::read_players(inputs);
    let d = super::dinput::read_players(inputs);
    [x[0].merge(d[0]), x[1].merge(d[1])]
}

/// Player one's controllers (the single-seat racers).
pub fn read() -> Pad {
    read_players(&inputs())[0]
}

/// Controllers being read (XInput slots and DirectInput devices).
pub fn connected() -> u32 {
    super::xinput::connected() + super::dinput::connected()
}
