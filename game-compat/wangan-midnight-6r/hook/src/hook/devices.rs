//! The cabinet device APIs the game imports from this library, and the JVS
//! board behind COM3.
use super::log;
use crate::board::Board;
use core::{
    ptr,
    sync::atomic::{AtomicBool, AtomicU32, Ordering},
};

// ---------------------------------------------------------------------------
// JVS board (COM3). The game's JVS thread is the only caller; the lock only
// guards against a second thread touching the port.

static LOCK: AtomicBool = AtomicBool::new(false);
static mut BOARD: Board = Board::new();
static mut CONTROLS: Option<super::input::Controls> = None;
static UNKNOWN_LOGGED: AtomicU32 = AtomicU32::new(0);

fn with_board<R>(f: impl FnOnce(&mut Board, &mut super::input::Controls) -> R) -> R {
    while LOCK
        .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        core::hint::spin_loop();
    }
    let result = unsafe {
        let controls = (*ptr::addr_of_mut!(CONTROLS)).get_or_insert_with(super::input::Controls::new);
        f(&mut *ptr::addr_of_mut!(BOARD), controls)
    };
    LOCK.store(false, Ordering::Release);
    result
}

pub fn jvs_reset() {
    with_board(|b, _| *b = Board::new());
}

pub fn jvs_write(bytes: &[u8]) {
    with_board(|b, controls| {
        let (inputs, coin) = controls.poll();
        if coin {
            b.insert_coin(0);
        }
        let was = b.addressed();
        b.write(bytes, &mut || inputs);
        if b.addressed() && !was {
            log(&[b"JVS: board addressed"]);
        }
        if let Some(command) = b.unknown.take() {
            // The first few, with the raw request, for the protocol log.
            if UNKNOWN_LOGGED.fetch_add(1, Ordering::Relaxed) < 8 {
                let mut line = [0u8; 200];
                let mut n = 0;
                for &byte in bytes.iter().take(64) {
                    for d in [byte >> 4, byte & 15] {
                        line[n] = b"0123456789abcdef"[usize::from(d)];
                        n += 1;
                    }
                    line[n] = b' ';
                    n += 1;
                }
                let mut h = [0u8; 16];
                log(&[
                    b"JVS: unknown command 0x",
                    super::hex(command.into(), &mut h),
                    b" in ",
                    &line[..n],
                ]);
            }
        }
    })
}

pub fn jvs_read(out: &mut [u8]) -> usize {
    with_board(|b, _| b.read(out))
}

pub fn jvs_pending() -> usize {
    with_board(|b, _| b.pending())
}

pub fn jvs_addressed() -> bool {
    with_board(|b, _| b.addressed())
}

pub fn jvs_purge() {
    with_board(|b, _| {
        let mut sink = [0u8; 64];
        while b.read(&mut sink) > 0 {}
    })
}

// ---------------------------------------------------------------------------
// HASP (Sentinel LDK) dongle. The game logs in to feature 0 and reads and
// writes its read/write memory file. The memory lives in this process only.

const HASP_OK: u32 = 0;
const HASP_INVALID_PARAMETER: u32 = 501;
const HASP_MEMORY: usize = 0x1000;
const HANDLE: u32 = 1;

static mut MEMORY: [u8; HASP_MEMORY] = [0; HASP_MEMORY];
static LOGGED: AtomicU32 = AtomicU32::new(0);

fn hasp_log(what: &[u8], a: u64, b: u64) {
    // The first calls only; the game polls the dongle.
    if LOGGED.fetch_add(1, Ordering::Relaxed) < 24 {
        let (mut x, mut y) = ([0u8; 16], [0u8; 16]);
        log(&[b"HASP ", what, b" ", super::hex(a, &mut x), b" ", super::hex(b, &mut y)]);
    }
}

fn memory() -> &'static mut [u8; HASP_MEMORY] {
    unsafe { &mut *ptr::addr_of_mut!(MEMORY) }
}

/// The game's dongle check reads a 64-byte identity block at 0xD00: the
/// cabinet serial (12 characters), then at 0x3E the 8-bit sum of bytes
/// 0..0x3E and at 0x3F its complement.
const IDENTITY: usize = 0xd00;

fn write_identity(memory: &mut [u8; HASP_MEMORY]) {
    let block = &mut memory[IDENTITY..IDENTITY + 0x40];
    block[..SERIAL.len()].copy_from_slice(SERIAL);
    let sum = block[..0x3e].iter().fold(0u8, |a, &b| a.wrapping_add(b));
    block[0x3e] = sum;
    block[0x3f] = !sum;
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn hasp_login(feature: u32, _vendor: *const u8, handle: *mut u32) -> u32 {
    hasp_log(b"login feature", feature.into(), 0);
    if memory()[IDENTITY + 0x3f] == 0 {
        write_identity(memory());
    }
    if handle.is_null() {
        return HASP_INVALID_PARAMETER;
    }
    unsafe { *handle = HANDLE };
    HASP_OK
}

#[unsafe(no_mangle)]
pub extern "system" fn hasp_logout(_handle: u32) -> u32 {
    HASP_OK
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn hasp_get_size(_handle: u32, file: u32, size: *mut u32) -> u32 {
    hasp_log(b"size of file", file.into(), HASP_MEMORY as u64);
    if size.is_null() {
        return HASP_INVALID_PARAMETER;
    }
    unsafe { *size = HASP_MEMORY as u32 };
    HASP_OK
}

fn range(offset: u32, length: u32) -> Option<core::ops::Range<usize>> {
    let start = offset as usize;
    let end = start.checked_add(length as usize)?;
    (end <= HASP_MEMORY).then_some(start..end)
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn hasp_read(
    _handle: u32,
    _file: u32,
    offset: u32,
    length: u32,
    buffer: *mut u8,
) -> u32 {
    hasp_log(b"read", offset.into(), length.into());
    let Some(r) = range(offset, length).filter(|_| !buffer.is_null()) else {
        return HASP_INVALID_PARAMETER;
    };
    unsafe { ptr::copy_nonoverlapping(memory()[r].as_ptr(), buffer, length as usize) };
    HASP_OK
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn hasp_write(
    _handle: u32,
    _file: u32,
    offset: u32,
    length: u32,
    buffer: *const u8,
) -> u32 {
    // Always logged: the game writes its records rarely.
    let (mut x, mut y) = ([0u8; 16], [0u8; 16]);
    log(&[b"HASP write ", super::hex(offset.into(), &mut x), b" ", super::hex(length.into(), &mut y)]);
    let Some(r) = range(offset, length).filter(|_| !buffer.is_null()) else {
        return HASP_INVALID_PARAMETER;
    };
    unsafe { ptr::copy_nonoverlapping(buffer, memory()[r].as_mut_ptr(), length as usize) };
    HASP_OK
}

/// The dongle's AES transform; this dongle stores plain data.
#[unsafe(no_mangle)]
pub extern "system" fn hasp_decrypt(_handle: u32, _buffer: *mut u8, length: u32) -> u32 {
    hasp_log(b"decrypt", length.into(), 0);
    HASP_OK
}

// ---------------------------------------------------------------------------
// USB dongle finder: the cabinet's USB memory key, identified by a serial of
// its own. The game accepts twelve digits: `2842`, then 1, then 0, 1 or 2,
// then 48 or 99, then any four.

pub const USB_SERIAL: &[u8; 12] = b"284211480001";

/// This desktop cabinet's serial number, in the form the game's dongle check
/// accepts: twelve digits, `2808` (this title), a board digit (0: not tied to
/// a board), a cabinet digit (1 is the terminal, 3 to 5 a driving cabinet;
/// this is a driving cabinet), then six digits whose leading pair is 40 or 99.
pub const SERIAL: &[u8; 12] = b"280803400001";

#[unsafe(no_mangle)]
pub extern "system" fn nbamUsbFinderInitialize() -> u32 {
    log(&[b"USB finder: initialised"]);
    0
}

#[unsafe(no_mangle)]
pub extern "system" fn nbamUsbFinderRelease() -> u32 {
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn nbamUsbFinderGetSerialNumber(index: u32, serial: *mut u8) -> u32 {
    log(&[b"USB finder: serial ", USB_SERIAL]);
    if index != 0 || serial.is_null() {
        return 1;
    }
    // A C string: the caller's buffer holds 12 digits and the terminator.
    unsafe {
        ptr::copy_nonoverlapping(USB_SERIAL.as_ptr(), serial, USB_SERIAL.len());
        *serial.add(USB_SERIAL.len()) = 0;
    }
    0
}

// ---------------------------------------------------------------------------
// Banapass card reader: not connected. Every call fails.

macro_rules! no_reader {
    ($($name:ident),*) => {$(
        #[unsafe(no_mangle)]
        pub extern "system" fn $name() -> i32 {
            log(&[b"card reader: ", stringify!($name).as_bytes(), b" (no reader)"]);
            -1
        }
    )*};
}
no_reader!(
    BngRwAttach,
    BngRwFin,
    BngRwGetVersion,
    BngRwInit,
    BngRwIsCmdExec,
    BngRwReqAction,
    BngRwReqBeep,
    BngRwReqCancel,
    BngRwReqLed,
    BngRwReqSendUrlTo,
    BngRwReqWaitTouch
);
