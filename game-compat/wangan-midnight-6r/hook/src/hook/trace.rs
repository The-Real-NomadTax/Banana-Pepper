//! Startup diagnostics (`ARCADE_WMMT6R_TRACE=1`, set by the probe action):
//! logs each state the game's boot sequencer (`game::InfoSystem::Impl::process`)
//! passes through.
//!
//! The sequencer's only call site is redirected through a stub placed next to
//! the image (a `call rel32` must reach it) that stores `this` and continues
//! into the original function. A thread then watches the state field.
use super::log;
use core::{ffi::c_void, ptr};

#[link(name = "kernel32", kind = "raw-dylib")]
unsafe extern "system" {
    fn VirtualAlloc(address: *mut c_void, size: usize, kind: u32, protect: u32) -> *mut u8;
    fn VirtualProtect(address: *mut c_void, size: usize, protect: u32, old: *mut u32) -> i32;
    fn FlushInstructionCache(process: isize, address: *const c_void, size: usize) -> i32;
    fn GetCurrentProcess() -> isize;
    fn CreateThread(
        attributes: *mut c_void,
        stack: usize,
        start: unsafe extern "system" fn(*mut c_void) -> u32,
        parameter: *mut c_void,
        flags: u32,
        id: *mut u32,
    ) -> isize;
    fn Sleep(ms: u32);
    fn GetTickCount() -> u32;
    fn GetCurrentThreadId() -> u32;
    fn GetCurrentProcessId() -> u32;
    fn CreateToolhelp32Snapshot(flags: u32, process: u32) -> isize;
    fn Thread32First(snapshot: isize, entry: *mut ThreadEntry) -> i32;
    fn Thread32Next(snapshot: isize, entry: *mut ThreadEntry) -> i32;
    fn OpenThread(access: u32, inherit: i32, id: u32) -> isize;
    fn SuspendThread(thread: isize) -> u32;
    fn ResumeThread(thread: isize) -> u32;
    fn GetThreadContext(thread: isize, context: *mut u8) -> i32;
    fn CloseHandle(handle: isize) -> i32;
    fn VirtualQuery(address: *const c_void, info: *mut [usize; 6], size: usize) -> usize;
}

#[repr(C)]
struct ThreadEntry {
    size: u32,
    usage: u32,
    thread: u32,
    owner: u32,
    base_priority: i32,
    delta_priority: i32,
    flags: u32,
}

/// x64 `CONTEXT` (1232 bytes, 16-byte aligned).
#[repr(C, align(16))]
struct Context([u8; 1232]);

/// Log each other thread's instruction pointer and the exe return addresses
/// on its stack, to see where a stalled boot is waiting.
fn sample_threads() {
    let me = unsafe { GetCurrentThreadId() };
    let process = unsafe { GetCurrentProcessId() };
    let snapshot = unsafe { CreateToolhelp32Snapshot(0x4, 0) };
    if snapshot == -1 {
        return;
    }
    let mut entry = ThreadEntry {
        size: core::mem::size_of::<ThreadEntry>() as u32,
        usage: 0,
        thread: 0,
        owner: 0,
        base_priority: 0,
        delta_priority: 0,
        flags: 0,
    };
    let mut more = unsafe { Thread32First(snapshot, &mut entry) } != 0;
    while more {
        if entry.owner == process && entry.thread != me {
            // SUSPEND_RESUME | GET_CONTEXT | QUERY_INFORMATION
            let thread = unsafe { OpenThread(0x2 | 0x8 | 0x40, 0, entry.thread) };
            if thread != 0 {
                let mut context = Context([0; 1232]);
                // ContextFlags = CONTEXT_AMD64 | CONTROL | INTEGER
                context.0[0x30..0x34].copy_from_slice(&0x10_0003u32.to_le_bytes());
                unsafe { SuspendThread(thread) };
                if unsafe { GetThreadContext(thread, context.0.as_mut_ptr()) } != 0 {
                    let rip = u64::from_le_bytes(context.0[0xf8..0x100].try_into().unwrap());
                    let rsp = u64::from_le_bytes(context.0[0x98..0xa0].try_into().unwrap());
                    let mut line = [0u8; 240];
                    let mut n = 0;
                    let mut push = |bytes: &[u8]| {
                        for &b in bytes {
                            if n < line.len() {
                                line[n] = b;
                                n += 1;
                            }
                        }
                    };
                    let (mut a, mut b) = ([0u8; 16], [0u8; 16]);
                    push(b"thread ");
                    push(super::hex(entry.thread.into(), &mut a));
                    push(b" at ");
                    push(super::hex(rip, &mut b));
                    push(b":");
                    // Only the committed region holding the stack pointer.
                    let mut info = [0usize; 6];
                    let end = if unsafe { VirtualQuery(rsp as *const c_void, &mut info, 48) } != 0 {
                        (info[0] + info[3]) as u64
                    } else {
                        rsp
                    };
                    let mut hits = 0;
                    for i in 0..((end.saturating_sub(rsp) / 8).min(16384)) {
                        let v = unsafe { ptr::read_volatile((rsp + i * 8) as *const u64) };
                        if (0x1_4000_1000..0x1_4100_a000).contains(&v) && hits < 14 {
                            let mut h = [0u8; 16];
                            push(b" ");
                            push(super::hex(v, &mut h));
                            hits += 1;
                        }
                    }
                    log(&[&line[..n]]);
                }
                unsafe {
                    ResumeThread(thread);
                    CloseHandle(thread);
                }
            }
        }
        more = unsafe { Thread32Next(snapshot, &mut entry) } != 0;
    }
    unsafe { CloseHandle(snapshot) };
}

/// `call game::InfoSystem::Impl::process` in the per-frame update.
const CALL_SITE: usize = 0x1_4071_9ebc;
const CALL_BYTES: [u8; 5] = [0xe8, 0xbf, 0xbf, 0xff, 0xff];
const PROCESS: usize = 0x1_4071_5e80;
/// Offsets in the sequencer object: its state and its mode.
const STATE: usize = 0x3f8;
const MODE: usize = 0x378;
/// The dongle manager (`*DONGLE`): its status, whether its identity was
/// stored, and whether its record caches hold valid records.
const DONGLE: usize = 0x1_4205_02d8;
/// The network manager (`*NETWORK`): its state, and its auth service
/// (`*(network + 0x2c8)`): initialised and started flags.
const NETWORK: usize = 0x1_4205_2ca8;
const NETWORK_STATE: usize = 0x620;
const NETWORK_AUTH: usize = 0x2c8;
const DONGLE_FIELDS: [(usize, &[u8]); 4] = [
    (0x24, b"status"),
    (0x20, b"identity"),
    (0xb8, b"records"),
    (0xc9, b"record0"),
];

static mut THIS: usize = 0;

pub fn install() {
    if unsafe { core::slice::from_raw_parts(CALL_SITE as *const u8, 5) } != CALL_BYTES {
        log(&[b"trace: unexpected sequencer call site; not tracing"]);
        return;
    }
    // A page within 2 GiB below the image.
    let mut stub: *mut u8 = ptr::null_mut();
    let mut at = 0x1_3ff0_0000usize;
    while stub.is_null() && at > 0x1_0000_0000 {
        stub = unsafe { VirtualAlloc(at as *mut c_void, 0x1000, 0x3000, 0x40) };
        at -= 0x10_0000;
    }
    if stub.is_null() {
        log(&[b"trace: no room for the stub; not tracing"]);
        return;
    }
    // mov rax, &THIS; mov [rax], rcx; mov rax, PROCESS; jmp rax
    let this = ptr::addr_of_mut!(THIS) as u64;
    let mut code = [0u8; 25];
    code[..2].copy_from_slice(&[0x48, 0xb8]);
    code[2..10].copy_from_slice(&this.to_le_bytes());
    code[10..13].copy_from_slice(&[0x48, 0x89, 0x08]);
    code[13..15].copy_from_slice(&[0x48, 0xb8]);
    code[15..23].copy_from_slice(&(PROCESS as u64).to_le_bytes());
    code[23..25].copy_from_slice(&[0xff, 0xe0]);
    unsafe { ptr::copy_nonoverlapping(code.as_ptr(), stub, code.len()) };
    let relative = (stub as i64 - (CALL_SITE as i64 + 5)) as i32;
    let mut old = 0;
    unsafe {
        VirtualProtect(CALL_SITE as *mut c_void, 5, 0x40, &mut old);
        ptr::copy_nonoverlapping(relative.to_le_bytes().as_ptr(), (CALL_SITE + 1) as *mut u8, 4);
        VirtualProtect(CALL_SITE as *mut c_void, 5, old, &mut old);
        FlushInstructionCache(GetCurrentProcess(), CALL_SITE as *const c_void, 5);
        CreateThread(ptr::null_mut(), 0, watch, ptr::null_mut(), 0, ptr::null_mut());
    }
    log(&[b"trace: watching the boot sequencer"]);
}

fn read_pointer(address: usize) -> usize {
    if address < 0x10000 {
        return 0;
    }
    unsafe { ptr::read_volatile(address as *const usize) }
}

fn watch_network(last: &mut [u32; 7]) {
    let network = read_pointer(read_pointer(NETWORK));
    if network == 0 {
        return;
    }
    let auth = read_pointer(read_pointer(network + NETWORK_AUTH));
    let now = [
        unsafe { ptr::read_volatile((network + NETWORK_STATE) as *const u32) },
        if auth == 0 { u32::MAX - 1 } else { u32::from(unsafe { *((auth + 0x20) as *const u8) }) },
        if auth == 0 { u32::MAX - 1 } else { u32::from(unsafe { *((auth + 0x21) as *const u8) }) },
        // The cabinet link: blocked flag, link object present, node, flags.
        unsafe { ptr::read_volatile((network + 0xb8) as *const u32) },
        u32::from(read_pointer(network + 0x2b8) != 0),
        unsafe { ptr::read_volatile((network + 0x170) as *const u32) },
        unsafe { ptr::read_volatile((network + 0x174) as *const u32) },
    ];
    let names: [&[u8]; 7] = [
        b"network state",
        b"auth flag 20",
        b"auth flag 21",
        b"link blocked",
        b"link object",
        b"link node",
        b"link flags",
    ];
    for (i, name) in names.iter().enumerate() {
        if now[i] != last[i] {
            last[i] = now[i];
            let mut h = [0u8; 16];
            log(&[name, b" = ", super::hex(now[i].into(), &mut h)]);
        }
    }
}

/// The exe return addresses on the current stack, as text.
pub fn callers(out: &mut [u8; 160]) -> &[u8] {
    let marker = 0u64;
    let rsp = ptr::addr_of!(marker) as u64;
    // Only the committed region holding the stack pointer.
    let mut info = [0usize; 6];
    let end = if unsafe { VirtualQuery(rsp as *const c_void, &mut info, 48) } != 0 {
        (info[0] + info[3]) as u64
    } else {
        rsp
    };
    let mut n = 0;
    let mut hits = 0;
    for i in 0..(end.saturating_sub(rsp) / 8).min(1024) {
        let v = unsafe { ptr::read_volatile((rsp + i * 8) as *const u64) };
        if (0x1_4000_1000..0x1_4100_a000).contains(&v) && hits < 10 {
            let mut h = [0u8; 16];
            let t = super::hex(v, &mut h);
            if n + t.len() + 1 > out.len() {
                break;
            }
            out[n] = b' ';
            out[n + 1..n + 1 + t.len()].copy_from_slice(t);
            n += t.len() + 1;
            hits += 1;
        }
    }
    &out[..n]
}

/// The network manager's strings (std::string fields), logged on change:
/// where the game keeps its server URI and host.
fn watch_network_strings(last: &mut [u64; 0x90]) {
    let network = read_pointer(read_pointer(NETWORK));
    if network == 0 {
        return;
    }
    // The server client's settings object (`network + 0x2d8`).
    let client = read_pointer(network + 0x2d8);
    for offset in [0x60usize, 0x88] {
        if let Some(text) = string_at(client + offset).filter(|t| !t.is_empty()) {
            let slot = &mut last[0x80 + offset / 0x28];
            let hash = text.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &b| (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3));
            if *slot != hash {
                *slot = hash;
                let mut h = [0u8; 16];
                log(&[b"server client +0x", super::hex(offset as u64, &mut h), b" = \"", text, b"\""]);
            }
        }
    }
    for (i, offset) in (0x200..0x600).step_by(0x8).enumerate() {
        let Some(text) = string_at(network + offset) else { continue };
        if text.is_empty() || !text.iter().all(|b| (0x20..0x7f).contains(b)) {
            continue;
        }
        let mut hash = 0xcbf2_9ce4_8422_2325u64 ^ offset as u64;
        for &b in text {
            hash = (hash ^ u64::from(b)).wrapping_mul(0x100_0000_01b3);
        }
        let slot = &mut last[i];
        if *slot != hash {
            *slot = hash;
            let mut h = [0u8; 16];
            log(&[b"network +0x", super::hex(offset as u64, &mut h), b" = \"", text, b"\""]);
        }
    }
}

fn watch_dongle(last: &mut [u32; 4]) {
    let manager = unsafe { ptr::read_volatile(DONGLE as *const usize) };
    if manager == 0 {
        return;
    }
    let manager = unsafe { ptr::read_volatile(manager as *const usize) };
    if manager == 0 {
        return;
    }
    for (i, &(offset, name)) in DONGLE_FIELDS.iter().enumerate() {
        let value = if offset == 0x24 {
            unsafe { ptr::read_volatile((manager + offset) as *const u32) }
        } else {
            u32::from(unsafe { ptr::read_volatile((manager + offset) as *const u8) })
        };
        if value != last[i] {
            last[i] = value;
            let mut h = [0u8; 16];
            log(&[b"dongle ", name, b" = ", super::hex(value.into(), &mut h)]);
        }
    }
}

/// The sequencer's link peers (`this + 0x560`, 0x30-byte entries: status
/// at +0, serial string at +8): four drive cabinets, then the terminal.
fn watch_peers(this: usize, last: &mut [u32; 6]) {
    let (begin, end) = (read_pointer(this + 0x560), read_pointer(this + 0x568));
    let count = if begin == 0 || end < begin { 0 } else { (end - begin) / 0x30 };
    let mut now = [u32::MAX; 6];
    now[0] = count as u32;
    for i in 0..count.min(5) {
        now[1 + i] = unsafe { ptr::read_volatile((begin + i * 0x30) as *const u32) };
    }
    if now != *last {
        *last = now;
        let mut line = [0u8; 128];
        let mut n = 0;
        for v in now {
            let mut h = [0u8; 16];
            let t = super::hex(v.into(), &mut h);
            line[n..n + t.len()].copy_from_slice(t);
            n += t.len();
            line[n] = b' ';
            n += 1;
        }
        log(&[b"peers (count, status...): ", &line[..n]]);
    }
}

unsafe extern "system" fn watch(_: *mut c_void) -> u32 {
    let mut peers_last = [0u32; 6];
    let mut last = (u32::MAX, u32::MAX);
    let mut dongle_last = [u32::MAX; 4];
    let mut network_last = [u32::MAX; 7];
    let mut strings_last = [0u64; 0x90];
    let mut since = unsafe { GetTickCount() };
    let mut sampled = false;
    loop {
        unsafe { Sleep(50) };
        watch_dongle(&mut dongle_last);
        watch_network(&mut network_last);
        watch_network_strings(&mut strings_last);
        let this = unsafe { ptr::read_volatile(ptr::addr_of!(THIS)) };
        if this == 0 {
            continue;
        }
        watch_peers(this, &mut peers_last);
        let now = unsafe {
            (
                ptr::read_volatile((this + STATE) as *const u32),
                ptr::read_volatile((this + MODE) as *const u32),
            )
        };
        if now == last {
            if !sampled && unsafe { GetTickCount() }.wrapping_sub(since) > 20_000 {
                sampled = true;
                log(&[b"sequencer stalled for 20 s; thread stacks:"]);
                sample_threads();
            }
        } else {
            last = now;
            since = unsafe { GetTickCount() };
            sampled = false;
            let (mut a, mut b) = ([0u8; 16], [0u8; 16]);
            log(&[
                b"sequencer state 0x",
                super::hex(now.0.into(), &mut a),
                b" mode ",
                super::hex(now.1.into(), &mut b),
            ]);
        }
    }
}

// ---------------------------------------------------------------------------
// Breakpoints (`ARCADE_WMMT6R_BREAK=140b01680,...`, hexadecimal exe
// addresses): each hit logs the argument registers and the exe return
// addresses on the stack, then steps over the original instruction and
// re-arms. For finding which code path reaches a function.

#[link(name = "kernel32", kind = "raw-dylib")]
unsafe extern "system" {
    fn AddVectoredExceptionHandler(first: u32, handler: usize) -> *mut c_void;
}

const MAX_BREAKS: usize = 8;
static mut BREAKS: [(usize, u8, u32); MAX_BREAKS] = [(0, 0, 0); MAX_BREAKS];
static REARM: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

fn write_byte(address: usize, value: u8) {
    let mut old = 0;
    unsafe {
        VirtualProtect(address as *mut c_void, 1, 0x40, &mut old);
        ptr::write_volatile(address as *mut u8, value);
        VirtualProtect(address as *mut c_void, 1, old, &mut old);
        FlushInstructionCache(GetCurrentProcess(), address as *const c_void, 1);
    }
}

pub fn install_breakpoints(list: &[u8]) {
    let breaks = unsafe { &mut *ptr::addr_of_mut!(BREAKS) };
    let mut count = 0;
    for word in list.split(|&b| b == b',') {
        let mut address = 0usize;
        for &c in word {
            let digit = match c {
                b'0'..=b'9' => c - b'0',
                b'a'..=b'f' => c - b'a' + 10,
                b'A'..=b'F' => c - b'A' + 10,
                _ => continue,
            };
            address = address * 16 + usize::from(digit);
        }
        if !(0x1_4000_1000..0x1_4100_a000).contains(&address) || count == MAX_BREAKS {
            continue;
        }
        breaks[count] = (address, unsafe { *(address as *const u8) }, 0);
        write_byte(address, 0xcc);
        count += 1;
    }
    if count > 0 {
        unsafe { AddVectoredExceptionHandler(1, breakpoint as *const () as usize) };
        log(&[b"trace: breakpoints armed"]);
    }
}

/// Committed, readable memory at `address` for `len` bytes.
fn readable(address: usize, len: usize) -> bool {
    let mut info = [0usize; 6];
    if address < 0x10000 || unsafe { VirtualQuery(address as *const c_void, &mut info, 48) } == 0 {
        return false;
    }
    // MEMORY_BASIC_INFORMATION: base, allocation base, protect (u32) +
    // pad, size, then state/protect/type (u32s) at +0x20.
    let state = info[4] as u32;
    let protect = (info[4] >> 32) as u32;
    state == 0x1000 && protect & 0x66 != 0 && address + len <= info[0] + info[3]
}

fn string_at(address: usize) -> Option<&'static [u8]> {
    if !readable(address, 0x20) {
        return None;
    }
    let size = unsafe { ptr::read_unaligned((address + 0x10) as *const usize) };
    let capacity = unsafe { ptr::read_unaligned((address + 0x18) as *const usize) };
    if size > capacity || capacity > 1 << 20 || size > 200 {
        return None;
    }
    let data = if capacity >= 16 { unsafe { ptr::read_unaligned(address as *const usize) } } else { address };
    if !readable(data, size) {
        return None;
    }
    Some(unsafe { core::slice::from_raw_parts(data as *const u8, size) })
}

unsafe extern "system" fn breakpoint(pointers: *const *const u8) -> i32 {
    let (record, context) = unsafe { (*pointers, *pointers.add(1) as *mut u8) };
    let code = unsafe { ptr::read_unaligned(record as *const u32) };
    let get = |offset: usize| unsafe { ptr::read_unaligned(context.add(offset) as *const u64) };
    let breaks = unsafe { &mut *ptr::addr_of_mut!(BREAKS) };
    match code {
        0x8000_0003 => {
            let at = unsafe { ptr::read_unaligned(record.add(0x10) as *const u64) } as usize;
            let Some(entry) = breaks.iter_mut().find(|b| b.0 == at) else {
                return 0;
            };
            entry.2 += 1;
            if entry.2 <= 20 {
                let mut line = [0u8; 250];
                let mut n = 0;
                let mut push = |bytes: &[u8]| {
                    for &b in bytes {
                        if n < line.len() {
                            line[n] = b;
                            n += 1;
                        }
                    }
                };
                let mut h = [0u8; 16];
                push(b"break ");
                push(super::hex(at as u64, &mut h));
                for (name, offset) in [(&b" rax="[..], 0x78), (b" rcx=", 0x80), (b" rdx=", 0x88), (b" r8=", 0xb8), (b" r9=", 0xc0)] {
                    let mut h = [0u8; 16];
                    push(name);
                    push(super::hex(get(offset), &mut h));
                }
                // r8 read as an MSVC std::string (text inline below 16
                // bytes of capacity, else behind a pointer).
                if let Some(text) = string_at(get(0xb8) as usize) {
                    push(b" r8=\"");
                    push(text);
                    push(b"\"");
                }
                push(b" <-");
                let rsp = get(0x98);
                let mut hits = 0;
                for i in 0..512u64 {
                    let v = unsafe { ptr::read_volatile((rsp + i * 8) as *const u64) };
                    if (0x1_4000_1000..0x1_4100_a000).contains(&v) && hits < 10 {
                        let mut h = [0u8; 16];
                        push(b" ");
                        push(super::hex(v, &mut h));
                        hits += 1;
                    }
                }
                log(&[&line[..n]]);
            }
            write_byte(at, entry.1);
            unsafe {
                ptr::write_unaligned(context.add(0xf8) as *mut u64, at as u64);
                let flags = context.add(0x44) as *mut u32;
                ptr::write_unaligned(flags, ptr::read_unaligned(flags) | 0x100);
            }
            REARM.store(at, core::sync::atomic::Ordering::Relaxed);
            -1
        }
        0x8000_0004 => {
            let at = REARM.swap(0, core::sync::atomic::Ordering::Relaxed);
            if at == 0 {
                return 0;
            }
            write_byte(at, 0xcc);
            -1
        }
        _ => 0,
    }
}
