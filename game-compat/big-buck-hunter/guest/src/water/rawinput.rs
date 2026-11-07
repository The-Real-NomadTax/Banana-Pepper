#![allow(unsafe_op_in_unsafe_fn)]
//! Raw Input mice and light guns for Water's Big Buck window: each device is
//! read on its own, so two mice or guns can aim two guns (settings.cfg
//! `p1_input` / `p2_input = rawinput`). Devices are numbered in the order
//! they are first used and assigned by `pad::Inputs::device_player`.
//!
//! Relative mice move an aim point across the window; absolute devices
//! (most light guns) point directly. Buttons follow `pad::MOUSE_BUTTONS`
//! (left = RT, right = LT, middle = A, side = Start, extra = Back).
use crate::pad;
use std::ptr;
use windows_sys::Win32::Foundation::{HWND, LPARAM};
use windows_sys::Win32::UI::Input::{
    GetRawInputData, HRAWINPUT, RAWINPUT, RAWINPUTDEVICE, RAWINPUTHEADER, RID_INPUT,
    RIDEV_INPUTSINK, RIM_TYPEMOUSE, RegisterRawInputDevices,
};

/// Relative mouse counts across the whole window (pointer sensitivity).
const COUNTS_PER_WINDOW: i64 = 1600;
const MOUSE_MOVE_ABSOLUTE: u16 = 1;

struct Mouse {
    device: usize,
    buttons: u32,
    pointer: Option<(i64, i64)>,
}

static mut MICE: Vec<Mouse> = Vec::new();

/// Starts Raw Input for `window` when a player uses it.
pub unsafe fn register(window: HWND) {
    if !crate::jamma::inputs().uses(pad::Method::RawInput) {
        return;
    }
    let device = RAWINPUTDEVICE {
        usUsagePage: 1, // generic desktop
        usUsage: 2,     // mouse
        dwFlags: RIDEV_INPUTSINK,
        hwndTarget: window,
    };
    if RegisterRawInputDevices(&device, 1, std::mem::size_of::<RAWINPUTDEVICE>() as u32) == 0 {
        eprintln!(
            "[arcade] Raw Input unavailable: {}",
            std::io::Error::last_os_error()
        );
    } else {
        eprintln!("[arcade] Raw Input: each mouse or light gun is read on its own");
    }
}

/// A `WM_INPUT` message.
pub unsafe fn input(l: LPARAM) {
    let mut data: RAWINPUT = std::mem::zeroed();
    let mut size = std::mem::size_of::<RAWINPUT>() as u32;
    let read = GetRawInputData(
        l as HRAWINPUT,
        RID_INPUT,
        (&mut data as *mut RAWINPUT).cast(),
        &mut size,
        std::mem::size_of::<RAWINPUTHEADER>() as u32,
    );
    if read == u32::MAX || data.header.dwType != RIM_TYPEMOUSE {
        return;
    }
    let mice = &mut *ptr::addr_of_mut!(MICE);
    let device = data.header.hDevice as usize;
    let index = match mice.iter().position(|m| m.device == device) {
        Some(i) => i,
        None => {
            mice.push(Mouse {
                device,
                buttons: 0,
                pointer: None,
            });
            eprintln!("[arcade] Raw Input mouse {} found", mice.len());
            mice.len() - 1
        }
    };
    let mouse = &mut mice[index];
    let raw = data.data.mouse;
    let flags = raw.Anonymous.Anonymous.usButtonFlags;
    // Down/up flag pairs for left, right, middle, button 4, button 5.
    for (i, &mask) in pad::MOUSE_BUTTONS.iter().enumerate() {
        if flags & (1 << (2 * i)) != 0 {
            mouse.buttons |= mask;
        }
        if flags & (2 << (2 * i)) != 0 {
            mouse.buttons &= !mask;
        }
    }
    let full = i64::from(pad::POINTER_MAX);
    if raw.usFlags & MOUSE_MOVE_ABSOLUTE != 0 {
        mouse.pointer = Some((
            i64::from(raw.lLastX).clamp(0, full),
            i64::from(raw.lLastY).clamp(0, full),
        ));
    } else if raw.lLastX != 0 || raw.lLastY != 0 {
        let (x, y) = mouse.pointer.get_or_insert((full / 2, full / 2));
        *x = (*x + i64::from(raw.lLastX) * full / COUNTS_PER_WINDOW).clamp(0, full);
        *y = (*y + i64::from(raw.lLastY) * full / COUNTS_PER_WINDOW).clamp(0, full);
    }
}

/// Each player's Raw Input mice and light guns.
pub unsafe fn read_players(inputs: &pad::Inputs) -> [pad::Pad; pad::MAX_PLAYERS] {
    let mut out = [pad::Pad::default(); pad::MAX_PLAYERS];
    for (index, mouse) in (*ptr::addr_of!(MICE)).iter().enumerate() {
        let Some(player) = inputs.device_player(pad::Device::Mouse, index) else {
            continue;
        };
        let mut state = pad::Pad {
            buttons: mouse.buttons,
            pointer: mouse.pointer.map(|(x, y)| (x as u16, y as u16)),
            ..pad::Pad::default()
        };
        state.lt = if state.held(pad::LT) { 255 } else { 0 };
        state.rt = if state.held(pad::RT) { 255 } else { 0 };
        out[player] = out[player].merge(state);
    }
    out
}
