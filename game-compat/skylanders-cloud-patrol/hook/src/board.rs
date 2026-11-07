//! Replacement for the cabinet's Kaboom I/O board client (`KaboomComDll.dll`),
//! which talks to the coin mechanism, ticket feeder and keypad over a serial
//! port. The same adapter DLL is staged under this name too; when the game
//! loads it as the board, the coin action inserts coins and every ticket
//! request is paid out at once.
//!
//! The API is cdecl and every call returns a status byte: 0 success,
//! 7 not initialised, 8 communication error.
use super::{CreateThread, GetTickCount, Sleep, action, controllers, log, log_number, pad};
use core::{
    ptr,
    sync::atomic::{AtomicBool, AtomicU16, Ordering::SeqCst},
};

const OK: u8 = 0;
const NOT_INITIALISED: u8 = 7;
/// Action target of "Insert coin" in `controls.rs`.
const COIN: u32 = 2;

static mut OPEN: bool = false;
static mut THREAD: bool = false;
/// Coins inserted but not yet shown to the game (added by the coin thread).
static PENDING: AtomicU16 = AtomicU16::new(0);
/// The board's first coin counter as the game reads it. The game polls the
/// counter, credits ONE coin for every poll in which it went up (whatever
/// the increase), and resets it after crediting. So each poll raises it by
/// at most one while coins are pending: quick coins are queued, not merged.
static SHOWN: AtomicU16 = AtomicU16::new(0);
/// Set by a reset. The game only updates its remembered count when it
/// polls, so the first poll after a reset reports the cleared count
/// without raising it; otherwise the next raise (0 to 1) would not look
/// like an increase against the stale count.
static RESET: AtomicBool = AtomicBool::new(false);
/// Tickets paid out since the last reset.
static mut TICKETS: u16 = 0;

/// Watches the coin action; an edge inserts one coin.
unsafe extern "system" fn coin_thread(_parameter: *mut u8) -> u32 {
    let mut held = false;
    let mut last = GetTickCount();
    while OPEN {
        Sleep(10);
        let state = if super::focused() {
            controllers::read()
        } else {
            pad::Pad::default()
        };
        let now_held = super::focused() && action(COIN, &state);
        if now_held && !held && GetTickCount().wrapping_sub(last) > 60 {
            last = GetTickCount();
            let pending = PENDING.fetch_add(1, SeqCst).wrapping_add(1);
            log_number(
                b"arcade-skylanders: coin inserted, waiting to be read: ",
                i32::from(pending),
            );
        }
        held = now_held;
    }
    THREAD = false;
    0
}

unsafe fn status() -> u8 {
    if OPEN { OK } else { NOT_INITIALISED }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn InitLib(_port: u16) -> u8 {
    log(b"arcade-skylanders: coin/ticket board ready (no serial port)");
    OPEN = true;
    if !THREAD {
        THREAD = true;
        if CreateThread(
            ptr::null(),
            0,
            coin_thread,
            ptr::null_mut(),
            0,
            ptr::null_mut(),
        ) == 0
        {
            THREAD = false;
        }
    }
    OK
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn CloseLib() {
    OPEN = false;
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn CheckComm() -> u8 {
    status()
}
unsafe fn write16(out: *mut u16, value: u16) {
    if !out.is_null() {
        *out = value;
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn GetCurrentCoinCount(coins: *mut u16, other: *mut u16) -> u8 {
    let raise = !RESET.swap(false, SeqCst)
        && PENDING
            .try_update(SeqCst, SeqCst, |p| p.checked_sub(1))
            .is_ok();
    let count = if raise {
        SHOWN.fetch_add(1, SeqCst).wrapping_add(1)
    } else {
        SHOWN.load(SeqCst)
    };
    write16(coins, count);
    write16(other, 0);
    status()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ResetCoinCount(coins: *mut u16, other: *mut u16) -> u8 {
    write16(coins, SHOWN.swap(0, SeqCst));
    RESET.store(true, SeqCst);
    write16(other, 0);
    status()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn GetKeypadCount(keys: *mut u8) -> u8 {
    if !keys.is_null() {
        ptr::write_bytes(keys, 0, 4);
    }
    status()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn SetTicketFeederTicketToFeedCount(count: u16) -> u8 {
    // Paid out immediately: there is no feeder to wait for.
    TICKETS = TICKETS.wrapping_add(count);
    if count > 0 {
        log_number(b"arcade-skylanders: tickets paid out ", i32::from(count));
    }
    status()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn GetTicketFeederTicketToFeedCount(remaining: *mut u16) -> u8 {
    write16(remaining, 0);
    status()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn GetTicketFeederCount(paid: *mut u16, empty: *mut u16) -> u8 {
    write16(paid, TICKETS);
    write16(empty, 0);
    status()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ResetTicketFeeder() -> u8 {
    TICKETS = 0;
    status()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ResetEmptyTicketFeeder(empty: *mut u16) -> u8 {
    write16(empty, 0);
    status()
}
