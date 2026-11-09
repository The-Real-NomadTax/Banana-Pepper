//! The Windows side: start-up checks, the game's serial calls and logging.
//!
//! `DllMain` runs after the loader has bound `wmn6r.exe`'s imports and
//! before any of its code. It checks that the image is the supported build,
//! then points some of the exe's own imports at this library: the KERNEL32
//! serial-port calls (COM3 is the JVS board, other COM ports report as
//! absent, every other handle goes to Windows unchanged) and ole32's
//! `CoCreateInstance` (the auth service, `auth.rs`).
#![allow(non_snake_case)]

mod auth;
#[allow(dead_code)]
#[path = "../../../../guest/win32/controllers.rs"]
mod controllers;
mod devices;
#[allow(dead_code)]
#[path = "../../../../guest/win32/dinput.rs"]
mod dinput;
mod input;
mod media;
mod network;
use media::{create_source_reader_from_media_source, create_source_reader_from_url};
use network::{
    get_ip_addr_table, get_ip_forward_table, get_queued_completion_status, getaddrinfo, wsa_ioctl, wsa_recv_from,
    wsa_send_to,
};
#[allow(dead_code)]
#[path = "../../../../crates/banana-input/src/pad.rs"]
mod pad;
#[path = "../../../../guest/win32/rt.rs"]
mod rt;
mod server;
mod trace;
#[allow(dead_code)]
#[path = "../../../../guest/win32/xinput.rs"]
mod xinput;

use core::{
    ffi::c_void,
    ptr,
    sync::atomic::{AtomicBool, AtomicIsize, Ordering},
};

#[link(name = "kernel32", kind = "raw-dylib")]
unsafe extern "system" {
    fn GetModuleHandleA(name: *const u8) -> isize;
    fn VirtualProtect(address: *mut c_void, size: usize, protect: u32, old: *mut u32) -> i32;
    fn GetStdHandle(which: u32) -> isize;
    fn WriteFile(file: isize, buffer: *const u8, len: u32, written: *mut u32, o: *mut c_void) -> i32;
    fn CreateEventA(attributes: *mut c_void, manual: i32, initial: i32, name: *const u8) -> isize;
    fn SetLastError(code: u32);
    fn DisableThreadLibraryCalls(module: isize) -> i32;
    fn AddVectoredExceptionHandler(first: u32, handler: usize) -> *mut c_void;
    pub fn ExitProcess(code: u32) -> !;
}

// ---------------------------------------------------------------------------
// The supported image (SHA-256 bf9fded6…2be, checked by the plan).

/// PE header fields of the one supported `wmn6r.exe`.
const TIMESTAMP: u32 = 0x5dee_006c;
const SIZE_OF_IMAGE: u32 = 0x021d_4000;

// ---------------------------------------------------------------------------
// Logging to the session log (Wine passes stderr through).

pub fn log(parts: &[&[u8]]) {
    let mut line = [0u8; 256];
    let mut n = 0;
    for part in parts.iter().chain(core::iter::once(&&b"\n"[..])) {
        for &b in *part {
            if n < line.len() {
                line[n] = b;
                n += 1;
            }
        }
    }
    let mut written = 0;
    unsafe {
        WriteFile(
            GetStdHandle(-12i32 as u32),
            b"[wmmt6r] ".as_ptr(),
            9,
            &mut written,
            ptr::null_mut(),
        );
        WriteFile(
            GetStdHandle(-12i32 as u32),
            line.as_ptr(),
            n as u32,
            &mut written,
            ptr::null_mut(),
        )
    };
}

/// `value` as hexadecimal digits (for log lines).
pub fn hex(value: u64, out: &mut [u8; 16]) -> &[u8] {
    let digits = (((64 - value.leading_zeros()) + 3) / 4).max(1) as usize;
    for i in 0..digits {
        out[digits - 1 - i] = b"0123456789abcdef"[((value >> (4 * i)) & 15) as usize];
    }
    &out[..digits]
}

// ---------------------------------------------------------------------------
// The exe's KERNEL32 and ole32 imports.

type CreateFileA = unsafe extern "system" fn(*const u8, u32, u32, *mut c_void, u32, u32, isize) -> isize;
type ReadFile = unsafe extern "system" fn(isize, *mut u8, u32, *mut u32, *mut c_void) -> i32;
type WriteFileFn = unsafe extern "system" fn(isize, *const u8, u32, *mut u32, *mut c_void) -> i32;
type OneHandle = unsafe extern "system" fn(isize) -> i32;
type HandlePointer = unsafe extern "system" fn(isize, *mut c_void) -> i32;
type HandleValue = unsafe extern "system" fn(isize, u32) -> i32;
type HandleTwoPointers = unsafe extern "system" fn(isize, *mut u32, *mut u32) -> i32;

/// Each hooked import: name, replacement, and the original once found.
struct Import {
    name: &'static [u8],
    replacement: *const (),
    original: AtomicIsize,
}
// Read-only apart from the atomic.
unsafe impl Sync for Import {}

macro_rules! imports {
    ($($name:literal => $f:ident),* $(,)?) => {
        static IMPORTS: [Import; [$(stringify!($f)),*].len()] = [$(Import {
            name: $name,
            replacement: $f as *const (),
            original: AtomicIsize::new(0),
        }),*];
    };
}
imports! {
    b"CreateFileA" => create_file,
    b"ReadFile" => read_file,
    b"WriteFile" => write_file,
    b"CloseHandle" => close_handle,
    b"GetCommState" => get_comm_state,
    b"SetCommState" => set_comm_state,
    b"SetCommTimeouts" => set_comm_timeouts,
    b"GetCommModemStatus" => modem_status,
    b"PurgeComm" => purge_comm,
    b"ClearCommError" => clear_comm_error,
    b"EscapeCommFunction" => escape_comm_function,
    b"SetupComm" => setup_comm,
    b"CoCreateInstance" => co_create_instance,
    b"GetIfEntry" => get_if_entry,
    b"GetIpAddrTable" => get_ip_addr_table,
    b"GetIpForwardTable" => get_ip_forward_table,
    b"WSASendTo" => wsa_send_to,
    b"WSARecvFrom" => wsa_recv_from,
    b"GetQueuedCompletionStatus" => get_queued_completion_status,
    b"getaddrinfo" => getaddrinfo,
    b"WSAIoctl" => wsa_ioctl,
    b"MFCreateSourceReaderFromURL" => create_source_reader_from_url,
    b"MFCreateSourceReaderFromMediaSource" => create_source_reader_from_media_source,
}

/// WS2_32 imports the exe takes by ordinal: (ordinal, replacement, original).
static ORDINALS: [(u64, Import); 4] = [
    (
        2,
        Import {
            name: b"bind",
            replacement: network::bind as *const (),
            original: AtomicIsize::new(0),
        },
    ),
    (
        6,
        Import {
            name: b"getsockname",
            replacement: network::getsockname as *const (),
            original: AtomicIsize::new(0),
        },
    ),
    (
        4,
        Import {
            name: b"connect",
            replacement: network::connect as *const (),
            original: AtomicIsize::new(0),
        },
    ),
    (
        21,
        Import {
            name: b"setsockopt",
            replacement: network::setsockopt as *const (),
            original: AtomicIsize::new(0),
        },
    ),
];

fn original_ordinal(ordinal: u64) -> usize {
    ORDINALS
        .iter()
        .find(|(o, _)| *o == ordinal)
        .map_or(0, |(_, i)| i.original.load(Ordering::Relaxed) as usize)
}

/// The cabinet's own network interface, as the game's link check reads it
/// (`MIB_IFROW`: index at +0x200, operational status at +0x220).
unsafe extern "system" fn get_if_entry(row: *mut u8) -> u32 {
    let f: unsafe extern "system" fn(*mut u8) -> u32 =
        unsafe { core::mem::transmute(original(b"GetIfEntry")) };
    let result = unsafe { f(row) };
    static LOGGED: AtomicIsize = AtomicIsize::new(0);
    if LOGGED.fetch_add(1, Ordering::Relaxed) < 4 {
        let (mut a, mut b, mut c) = ([0u8; 16], [0u8; 16], [0u8; 16]);
        let index = unsafe { ptr::read_unaligned(row.add(0x200) as *const u32) };
        let status = unsafe { ptr::read_unaligned(row.add(0x220) as *const u32) };
        log(&[
            b"GetIfEntry index ",
            hex(index.into(), &mut a),
            b" result ",
            hex(result.into(), &mut b),
            b" status ",
            hex(status.into(), &mut c),
        ]);
    }
    result
}

type CoCreateInstance =
    unsafe extern "system" fn(*const u8, *mut c_void, u32, *const u8, *mut *mut c_void) -> i32;

unsafe extern "system" fn co_create_instance(
    class: *const u8,
    outer: *mut c_void,
    context: u32,
    interface: *const u8,
    out: *mut *mut c_void,
) -> i32 {
    if let Some(result) = auth::create(class, interface, out) {
        return result;
    }
    let f: CoCreateInstance = unsafe { core::mem::transmute(original(b"CoCreateInstance")) };
    unsafe { f(class, outer, context, interface, out) }
}

fn original(name: &[u8]) -> usize {
    IMPORTS
        .iter()
        .find(|i| i.name == name)
        .map_or(0, |i| i.original.load(Ordering::Relaxed) as usize)
}

/// The JVS port's handle (a private event object), or 0.
static JVS: AtomicIsize = AtomicIsize::new(0);

fn is_jvs(handle: isize) -> bool {
    handle != 0 && handle == JVS.load(Ordering::Relaxed)
}

/// `COM3`, `\\.\COM3` -> 3.
fn com_port(name: *const u8) -> Option<u8> {
    let mut text = [0u8; 16];
    let mut n = 0;
    while n < text.len() {
        let b = unsafe { *name.add(n) };
        if b == 0 {
            break;
        }
        text[n] = b.to_ascii_uppercase();
        n += 1;
    }
    let text = &text[..n];
    let text = text.strip_prefix(b"\\\\.\\").unwrap_or(text);
    match text {
        [b'C', b'O', b'M', d @ b'1'..=b'9'] => Some(d - b'0'),
        _ => None,
    }
}

unsafe extern "system" fn create_file(
    name: *const u8,
    access: u32,
    share: u32,
    security: *mut c_void,
    disposition: u32,
    flags: u32,
    template: isize,
) -> isize {
    match (!name.is_null()).then(|| com_port(name)).flatten() {
        Some(3) => {
            let handle = unsafe { CreateEventA(ptr::null_mut(), 1, 0, ptr::null()) };
            JVS.store(handle, Ordering::Relaxed);
            devices::jvs_reset();
            log(&[b"COM3: JVS I/O board connected"]);
            handle
        }
        Some(port) => {
            log(&[b"COM", &[b'0' + port], b": no device"]);
            unsafe { SetLastError(2) };
            -1
        }
        None => {
            let f: CreateFileA = unsafe { core::mem::transmute(original(b"CreateFileA")) };
            unsafe { f(name, access, share, security, disposition, flags, template) }
        }
    }
}

unsafe extern "system" fn read_file(
    handle: isize,
    buffer: *mut u8,
    len: u32,
    read: *mut u32,
    overlapped: *mut c_void,
) -> i32 {
    if !is_jvs(handle) {
        let f: ReadFile = unsafe { core::mem::transmute(original(b"ReadFile")) };
        return unsafe { f(handle, buffer, len, read, overlapped) };
    }
    let out = unsafe { core::slice::from_raw_parts_mut(buffer, len as usize) };
    let n = devices::jvs_read(out);
    if !read.is_null() {
        unsafe { *read = n as u32 };
    }
    1
}

unsafe extern "system" fn write_file(
    handle: isize,
    buffer: *const u8,
    len: u32,
    written: *mut u32,
    overlapped: *mut c_void,
) -> i32 {
    if !is_jvs(handle) {
        let f: WriteFileFn = unsafe { core::mem::transmute(original(b"WriteFile")) };
        return unsafe { f(handle, buffer, len, written, overlapped) };
    }
    devices::jvs_write(unsafe { core::slice::from_raw_parts(buffer, len as usize) });
    if !written.is_null() {
        unsafe { *written = len };
    }
    1
}

unsafe extern "system" fn close_handle(handle: isize) -> i32 {
    if is_jvs(handle) {
        JVS.store(0, Ordering::Relaxed);
        log(&[b"COM3: closed"]);
    }
    let f: OneHandle = unsafe { core::mem::transmute(original(b"CloseHandle")) };
    unsafe { f(handle) }
}

/// Comm calls on the JVS port succeed; others go to Windows.
macro_rules! comm {
    ($f:ident, $name:literal, ($($arg:ident: $t:ty),*)) => {
        unsafe extern "system" fn $f(handle: isize, $($arg: $t),*) -> i32 {
            if is_jvs(handle) {
                return 1;
            }
            let f: unsafe extern "system" fn(isize, $($t),*) -> i32 =
                unsafe { core::mem::transmute(original($name)) };
            unsafe { f(handle, $($arg),*) }
        }
    };
}
comm!(get_comm_state, b"GetCommState", (dcb: *mut c_void));
comm!(set_comm_state, b"SetCommState", (dcb: *mut c_void));
comm!(set_comm_timeouts, b"SetCommTimeouts", (timeouts: *mut c_void));
comm!(escape_comm_function, b"EscapeCommFunction", (function: u32));
comm!(setup_comm, b"SetupComm", (input: u32, output: u32));

unsafe extern "system" fn modem_status(handle: isize, status: *mut u32) -> i32 {
    if !is_jvs(handle) {
        let f: HandlePointer = unsafe { core::mem::transmute(original(b"GetCommModemStatus")) };
        return unsafe { f(handle, status.cast()) };
    }
    // CTS always; DSR once the board has an address (the JVS sense line).
    if !status.is_null() {
        unsafe { *status = 0x10 | if devices::jvs_addressed() { 0x20 } else { 0 } };
    }
    1
}

unsafe extern "system" fn purge_comm(handle: isize, flags: u32) -> i32 {
    if !is_jvs(handle) {
        let f: HandleValue = unsafe { core::mem::transmute(original(b"PurgeComm")) };
        return unsafe { f(handle, flags) };
    }
    if flags & 0x8 != 0 {
        devices::jvs_purge();
    }
    1
}

unsafe extern "system" fn clear_comm_error(handle: isize, errors: *mut u32, stat: *mut u32) -> i32 {
    if !is_jvs(handle) {
        let f: HandleTwoPointers = unsafe { core::mem::transmute(original(b"ClearCommError")) };
        return unsafe { f(handle, errors, stat) };
    }
    unsafe {
        if !errors.is_null() {
            *errors = 0;
        }
        if !stat.is_null() {
            // COMSTAT: flags, bytes waiting to be read, bytes to send.
            *stat = 0;
            *stat.add(1) = devices::jvs_pending() as u32;
            *stat.add(2) = 0;
        }
    }
    1
}

// ---------------------------------------------------------------------------
// Installation.

unsafe fn read32(address: usize) -> u32 {
    unsafe { ptr::read_unaligned(address as *const u32) }
}

fn name_is(address: usize, name: &[u8]) -> bool {
    name.iter()
        .enumerate()
        .all(|(i, &b)| unsafe { *((address + i) as *const u8) } == b)
        && unsafe { *((address + name.len()) as *const u8) } == 0
}

fn name_is_ignore_case(address: usize, name: &[u8]) -> bool {
    name.iter().enumerate().all(|(i, &b)| {
        unsafe { *((address + i) as *const u8) }.eq_ignore_ascii_case(&b)
    }) && unsafe { *((address + name.len()) as *const u8) } == 0
}

/// Point the exe's KERNEL32 and ole32 imports in [`IMPORTS`] at this library.
unsafe fn install(base: usize) -> Result<(), &'static [u8]> {
    let nt = base + unsafe { read32(base + 0x3c) } as usize;
    if unsafe { read32(nt) } != 0x4550
        || unsafe { read32(nt + 8) } != TIMESTAMP
        || unsafe { read32(nt + 24 + 56) } != SIZE_OF_IMAGE
    {
        return Err(b"wmn6r.exe is not the supported build; no hooks installed");
    }
    let directory = base + unsafe { read32(nt + 24 + 112 + 8) } as usize;
    let mut descriptor = directory;
    loop {
        let name = unsafe { read32(descriptor + 12) } as usize;
        if name == 0 {
            break;
        }
        if name_is_ignore_case(base + name, b"KERNEL32.dll")
            || name_is_ignore_case(base + name, b"ole32.dll")
            || name_is_ignore_case(base + name, b"IPHLPAPI.DLL")
            || name_is_ignore_case(base + name, b"WS2_32.dll")
            || name_is_ignore_case(base + name, b"MFReadWrite.dll")
        {
            let names = base + unsafe { read32(descriptor) } as usize;
            let slots = base + unsafe { read32(descriptor + 16) } as usize;
            let mut i = 0;
            loop {
                let entry = unsafe { ptr::read_unaligned((names + i * 8) as *const u64) };
                if entry == 0 {
                    break;
                }
                let by_ordinal = (entry >> 63 == 1
                    && name_is_ignore_case(base + name, b"WS2_32.dll"))
                .then(|| ORDINALS.iter().find(|(o, _)| *o == entry & 0xffff))
                .flatten()
                .map(|(_, i)| i);
                let by_name = (entry >> 63 == 0)
                    .then(|| {
                        let function = base + entry as usize + 2;
                        IMPORTS.iter().find(|m| name_is(function, m.name))
                    })
                    .flatten();
                {
                    if let Some(import) = by_name.or(by_ordinal) {
                        let slot = (slots + i * 8) as *mut usize;
                        let mut old = 0;
                        unsafe {
                            VirtualProtect(slot.cast(), 8, 0x04, &mut old);
                            import.original.store(*slot as isize, Ordering::Relaxed);
                            *slot = import.replacement as usize;
                            VirtualProtect(slot.cast(), 8, old, &mut old);
                        }
                    }
                }
                i += 1;
            }
        }
        descriptor += 20;
    }
    for i in &IMPORTS {
        if i.original.load(Ordering::Relaxed) == 0 {
            log(&[b"wmn6r.exe does not import ", i.name]);
        }
    }
    Ok(())
}

/// Crash reports for the session log: the exception, and the exe return
/// addresses on the faulting thread's stack (innermost first, then the
/// outermost frames), which is enough to follow a recursion or a fault back
/// into the game.
unsafe extern "system" fn crash_report(pointers: *const *const u8) -> i32 {
    let (record, context) = unsafe { (*pointers, *pointers.add(1)) };
    let code = unsafe { ptr::read_unaligned(record as *const u32) };
    if !matches!(code, 0xc000_0005 | 0xc000_00fd | 0xc000_001d | 0xc000_0094) {
        return 0;
    }
    let rsp = unsafe { ptr::read_unaligned(context.add(0x98) as *const u64) };
    let rip = unsafe { ptr::read_unaligned(context.add(0xf8) as *const u64) };
    let top: u64;
    unsafe { core::arch::asm!("mov {}, gs:[8]", out(reg) top) };
    let (mut a, mut b) = ([0u8; 16], [0u8; 16]);
    log(&[b"crash ", hex(code.into(), &mut a), b" at ", hex(rip, &mut b)]);
    let exe = |v: u64| (0x1_4000_1000..0x1_4100_a000).contains(&v);
    let words = ((top - rsp) / 8) as usize;
    let stack = rsp as *const u64;
    let line = |from: usize, to: usize, step: isize| {
        let mut hits = 0;
        let mut i = from as isize;
        while i != to as isize && hits < 40 {
            let v = unsafe { *stack.offset(i) };
            if exe(v) {
                let mut h = [0u8; 16];
                log(&[b"  ", hex(v, &mut h)]);
                hits += 1;
            }
            i += step;
        }
    };
    line(0, words, 1);
    log(&[b"  ... outermost:"]);
    line(words - 1, 0, -1);
    0
}

static STARTED: AtomicBool = AtomicBool::new(false);

#[unsafe(no_mangle)]
pub extern "system" fn _DllMainCRTStartup(module: isize, reason: u32, _reserved: *mut c_void) -> i32 {
    if reason == 1 && !STARTED.swap(true, Ordering::Relaxed) {
        unsafe {
            DisableThreadLibraryCalls(module);
            AddVectoredExceptionHandler(1, crash_report as *const () as usize);
        }
        let base = unsafe { GetModuleHandleA(ptr::null()) } as usize;
        match unsafe { install(base) } {
            Ok(()) => {
                log(&[b"cabinet devices ready"]);
                network::start_terminal();
                server::start();
                let mut flag = [0u8; 4];
                if input::env(b"ARCADE_WMMT6R_TRACE\0", &mut flag) == Some(b"1") {
                    trace::install();
                    let mut list = [0u8; 160];
                    if let Some(list) = input::env(b"ARCADE_WMMT6R_BREAK\0", &mut list) {
                        trace::install_breakpoints(list);
                    }
                }
            }
            Err(e) => log(&[e]),
        }
    }
    1
}

// The stack probe MSVC-target code calls for frames over a page (normally
// from the CRT): touch each page from the caller's stack pointer down to
// `rsp - rax`, preserving every register.
core::arch::global_asm!(
    ".globl __chkstk",
    "__chkstk:",
    "push rcx",
    "push rax",
    "lea rcx, [rsp + 24]",
    "cmp rax, 0x1000",
    "jb 2f",
    "1:",
    "sub rcx, 0x1000",
    "test [rcx], rcx",
    "sub rax, 0x1000",
    "cmp rax, 0x1000",
    "ja 1b",
    "2:",
    "sub rcx, rax",
    "test [rcx], rcx",
    "pop rax",
    "pop rcx",
    "ret",
);

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    log(&[b"internal error; exiting"]);
    unsafe { ExitProcess(3) }
}
