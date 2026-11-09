//! The cabinet's authentication service (AMAuthd), in process.
//!
//! On a cabinet `AMAuthd.exe` is a COM local server; the game creates it with
//! `CoCreateInstance(CLSID_AUTH, CLSCTX_LOCAL_SERVER, IID_AUTH)` and polls it
//! for the network configuration and the ALL.Net/Mucha authentication state.
//! This object answers those calls itself: every method succeeds, output
//! structures stay as the game zeroed them except for the fields filled in
//! [`method`], and each method's first call is logged.
use super::log;
use core::{
    ffi::c_void,
    ptr,
    sync::atomic::{AtomicU32, AtomicU64, Ordering},
};

const CLSID_AUTH: [u8; 16] = [
    0x03, 0xbb, 0x03, 0x46, 0x8d, 0x05, 0xd9, 0x43, 0xb9, 0x6f, 0x63, 0x9b, 0xe9, 0x08, 0xc1, 0xed,
];
const IID_AUTH: [u8; 16] = [
    0x50, 0x51, 0x5a, 0x04, 0xb3, 0xd2, 0x90, 0x45, 0xa3, 0x8b, 0xc1, 0x15, 0x86, 0x78, 0xe1, 0xac,
];
const IID_UNKNOWN: [u8; 16] = [0, 0, 0, 0, 0, 0, 0, 0, 0xc0, 0, 0, 0, 0, 0, 0, 0x46];

const S_OK: i32 = 0;
const E_NOINTERFACE: i32 = 0x8000_4002u32 as i32;

/// The cabinet's network as the game plans it (its own constants): the
/// drive cabinet at 192.168.92.11 behind the content router 192.168.92.254.
/// The hook presents the same addresses to the game's IP Helper queries
/// (`network.rs`).
const ADDRESS: &[u8; 16] = b"192.168.92.11\0\0\0";
const GATEWAY: &[u8; 16] = b"192.168.92.254\0\0";

/// Methods after IUnknown's three.
const METHODS: usize = 40;

#[repr(C)]
struct Object {
    vtable: *const Vtable,
}
unsafe impl Sync for Object {}

#[repr(C)]
struct Vtable([*const (); 3 + METHODS]);
unsafe impl Sync for Vtable {}

static REFERENCES: AtomicU32 = AtomicU32::new(1);
/// Bit per method already logged.
static SEEN: AtomicU64 = AtomicU64::new(0);

fn guid(p: *const u8) -> [u8; 16] {
    let mut g = [0u8; 16];
    if !p.is_null() {
        unsafe { ptr::copy_nonoverlapping(p, g.as_mut_ptr(), 16) };
    }
    g
}

/// `CoCreateInstance` for the auth class; `None` for every other class.
pub fn create(class: *const u8, interface: *const u8, out: *mut *mut c_void) -> Option<i32> {
    if guid(class) != CLSID_AUTH {
        return None;
    }
    if out.is_null() {
        return Some(0x8000_4003u32 as i32); // E_POINTER
    }
    let iid = guid(interface);
    if iid != IID_AUTH && iid != IID_UNKNOWN {
        unsafe { *out = ptr::null_mut() };
        return Some(E_NOINTERFACE);
    }
    REFERENCES.fetch_add(1, Ordering::Relaxed);
    unsafe { *out = ptr::addr_of!(OBJECT) as *mut c_void };
    log(&[b"auth: service created (emulated)"]);
    Some(S_OK)
}

unsafe extern "system" fn query_interface(this: *mut c_void, iid: *const u8, out: *mut *mut c_void) -> i32 {
    if out.is_null() {
        return 0x8000_4003u32 as i32;
    }
    let iid = guid(iid);
    if iid == IID_AUTH || iid == IID_UNKNOWN {
        REFERENCES.fetch_add(1, Ordering::Relaxed);
        unsafe { *out = this };
        S_OK
    } else {
        unsafe { *out = ptr::null_mut() };
        E_NOINTERFACE
    }
}

unsafe extern "system" fn add_ref(_: *mut c_void) -> u32 {
    REFERENCES.fetch_add(1, Ordering::Relaxed) + 1
}

/// The object is static; references are only counted.
unsafe extern "system" fn release(_: *mut c_void) -> u32 {
    REFERENCES.fetch_sub(1, Ordering::Relaxed).saturating_sub(1).max(1)
}

/// One auth method: `index` counts from the first method after IUnknown.
fn method(index: usize, a: usize, _b: usize, _c: usize) -> i32 {
    if index < 64 && SEEN.fetch_or(1 << index, Ordering::Relaxed) & (1 << index) == 0 {
        let (mut h, mut p) = ([0u8; 16], [0u8; 16]);
        log(&[
            b"auth: method ",
            super::hex(index as u64, &mut h),
            b" (vtable +0x",
            super::hex(((index + 3) * 8) as u64, &mut p),
            b")",
        ]);
    }
    let put = |offset: usize, bytes: &[u8]| unsafe {
        ptr::copy_nonoverlapping(bytes.as_ptr(), (a + offset) as *mut u8, bytes.len())
    };
    match index {
        6 if a != 0 => {
            // Service state (vtable +0x48, 0xC8 bytes): ALL.Net
            // authentication finished (state 0xF), succeeded (result 0) and
            // complete (progress 2); the sequence number at +0x0C tells the
            // game to fetch the location (method 11) once. The
            // authentication type at +0xB8: 0 makes the game run offline, 1
            // is ALL.Net (the cabinet's AMConfig.ini `authType`).
            put(0x00, &0xfu32.to_le_bytes());
            put(0x04, &0u32.to_le_bytes());
            put(0x08, &2u32.to_le_bytes());
            put(0x0c, &1u32.to_le_bytes());
            // Mucha (the online services' authentication) likewise:
            // finished (0xF), result 0, authenticated (2).
            put(0x18, &0xfu32.to_le_bytes());
            put(0x1c, &0u32.to_le_bytes());
            put(0x20, &2u32.to_le_bytes());
            put(0xb8, &1u32.to_le_bytes());
        }
        7 if a != 0 => {
            // Network and cabinet configuration (vtable +0x50, 0xA8 bytes):
            // the PCB serial at +0x20, which must equal the dongle's; the
            // cabinet's address at +0x30 and gateway at +0x40; the network
            // type at +0x94.
            put(0x20, super::devices::SERIAL);
            put(0x30, ADDRESS);
            put(0x40, GATEWAY);
            put(0x94, &1u32.to_le_bytes());
        }
        11 if a != 0 => {
            // Location (vtable +0x70, 0x8A2 bytes): the game server's URI
            // at +0x000 (ALL.Net's `uri`). Cabinets take only an https URI
            // and reach it through their terminal.
            put(0x000, super::server::URI);
        }
        _ => {}
    }
    S_OK
}

macro_rules! methods {
    ($($index:literal => $name:ident),* $(,)?) => {
        $(unsafe extern "system" fn $name(_: *mut c_void, a: usize, b: usize, c: usize) -> i32 {
            method($index, a, b, c)
        })*
        static VTABLE: Vtable = Vtable([
            query_interface as *const (),
            add_ref as *const (),
            release as *const (),
            $($name as *const ()),*
        ]);
    };
}
methods! {
    0 => m0, 1 => m1, 2 => m2, 3 => m3, 4 => m4, 5 => m5, 6 => m6, 7 => m7,
    8 => m8, 9 => m9, 10 => m10, 11 => m11, 12 => m12, 13 => m13, 14 => m14,
    15 => m15, 16 => m16, 17 => m17, 18 => m18, 19 => m19, 20 => m20, 21 => m21,
    22 => m22, 23 => m23, 24 => m24, 25 => m25, 26 => m26, 27 => m27, 28 => m28,
    29 => m29, 30 => m30, 31 => m31, 32 => m32, 33 => m33, 34 => m34, 35 => m35,
    36 => m36, 37 => m37, 38 => m38, 39 => m39,
}

static OBJECT: Object = Object {
    vtable: ptr::addr_of!(VTABLE),
};
