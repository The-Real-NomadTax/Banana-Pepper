//! The cabinet's network, as the game expects to find it.
//!
//! A drive cabinet sits on a private network: its own address 192.168.92.11
//! behind the content router 192.168.92.254 (the game's built-in plan; it
//! reports E0502 CONTENT ROUTER MISMATCH and keeps the cabinet link closed
//! for any other gateway). The machine's real network is untouched: the
//! game's IP Helper queries see the primary interface (the one carrying the
//! default route) with the cabinet's address and gateway, and sockets the
//! game binds to a cabinet address are bound to every address instead.
use super::log;
use core::{ffi::c_void, ptr};

/// 192.168.92.11 and 192.168.92.254 in network byte order, as `u32` read
/// from memory (little-endian).
const ADDRESS: u32 = u32::from_le_bytes([192, 168, 92, 11]);
const GATEWAY: u32 = u32::from_le_bytes([192, 168, 92, 254]);
const MASK: u32 = u32::from_le_bytes([255, 255, 255, 0]);
/// The terminal's address in the game's plan, and loopback.
const TERMINAL: u32 = u32::from_le_bytes([192, 168, 92, 20]);
const LOOPBACK: u32 = u32::from_le_bytes([127, 0, 0, 1]);
const SUBNET: u32 = ADDRESS & MASK;

#[link(name = "iphlpapi", kind = "raw-dylib")]
unsafe extern "system" {
    #[link_name = "GetIpForwardTable"]
    fn host_forward_table(table: *mut u8, size: *mut u32, order: i32) -> u32;
}

/// The interface carrying the default route, or 0.
fn primary_interface() -> u32 {
    static mut TABLE: [u8; 32768] = [0; 32768];
    let table = unsafe { &mut *ptr::addr_of_mut!(TABLE) };
    let mut size = table.len() as u32;
    if unsafe { host_forward_table(table.as_mut_ptr(), &mut size, 0) } != 0 {
        return 0;
    }
    default_route(table.as_mut_ptr()).map_or(0, |row| unsafe {
        ptr::read_unaligned(row.add(16) as *const u32)
    })
}

/// The first default route (`MIB_IPFORWARDROW`: destination at 0, mask at 4,
/// next hop at 12, interface at 16; rows of 56 bytes after the count).
fn default_route(table: *mut u8) -> Option<*mut u8> {
    let count = unsafe { ptr::read_unaligned(table as *const u32) } as usize;
    (0..count.min(512))
        .map(|i| unsafe { table.add(4 + i * 56) })
        .find(|&row| unsafe {
            ptr::read_unaligned(row as *const u32) == 0
                && ptr::read_unaligned(row.add(4) as *const u32) == 0
        })
}

/// `GetIpForwardTable`: the default route goes through the content router.
pub unsafe extern "system" fn get_ip_forward_table(table: *mut u8, size: *mut u32, order: i32) -> u32 {
    let result = unsafe { host_forward_table(table, size, order) };
    if result == 0 && !table.is_null() {
        if let Some(row) = default_route(table) {
            unsafe { ptr::write_unaligned(row.add(12) as *mut u32, GATEWAY) };
        }
    }
    result
}

/// `GetIpAddrTable`: the primary interface has the cabinet's address
/// (`MIB_IPADDRROW`: address at 0, interface at 4, mask at 8, broadcast flag
/// at 12; rows of 24 bytes after the count). Without a default route the
/// first non-loopback interface is the primary one.
pub unsafe extern "system" fn get_ip_addr_table(table: *mut u8, size: *mut u32, order: i32) -> u32 {
    let f: unsafe extern "system" fn(*mut u8, *mut u32, i32) -> u32 =
        unsafe { core::mem::transmute(super::original(b"GetIpAddrTable")) };
    let result = unsafe { f(table, size, order) };
    if result == 0 && !table.is_null() {
        let count = (unsafe { ptr::read_unaligned(table as *const u32) } as usize).min(512);
        let row = |i: usize| unsafe { table.add(4 + i * 24) };
        let field = |i: usize, at: usize| unsafe { ptr::read_unaligned(row(i).add(at) as *const u32) };
        let primary = cabinet_row(table).map_or(0, |i| field(i, 4));
        static LOGGED: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
        let logging = LOGGED.fetch_add(1, core::sync::atomic::Ordering::Relaxed) < 2;
        for i in 0..count {
            if logging {
                let (mut a, mut b) = ([0u8; 16], [0u8; 16]);
                log(&[
                    b"network: address row interface ",
                    super::hex(field(i, 4).into(), &mut a),
                    b" address ",
                    super::hex(field(i, 0).into(), &mut b),
                ]);
            }
            if field(i, 4) == primary {
                unsafe {
                    ptr::write_unaligned(row(i) as *mut u32, ADDRESS);
                    ptr::write_unaligned(row(i).add(8) as *mut u32, MASK);
                }
            }
        }
        if logging {
            let mut h = [0u8; 16];
            log(&[b"network: cabinet interface ", super::hex(primary.into(), &mut h)]);
        }
    }
    result
}

/// The address table's row for the cabinet's interface: the one carrying
/// the default route, else the first non-loopback one.
fn cabinet_row(table: *const u8) -> Option<usize> {
    let count = (unsafe { ptr::read_unaligned(table as *const u32) } as usize).min(512);
    let field = |i: usize, at: usize| unsafe { ptr::read_unaligned(table.add(4 + i * 24 + at) as *const u32) };
    let loopback = |i: usize| field(i, 0) & 0xff == 127;
    let primary = primary_interface();
    (0..count)
        .find(|&i| field(i, 4) == primary && !loopback(i))
        .or_else(|| (0..count).find(|&i| !loopback(i)))
}

/// The cabinet interface's real address (looked up once), or 0.
fn host_address() -> u32 {
    use core::sync::atomic::{AtomicU32, Ordering};
    static CACHED: AtomicU32 = AtomicU32::new(u32::MAX);
    let cached = CACHED.load(Ordering::Relaxed);
    if cached != u32::MAX {
        return cached;
    }
    let address = lookup_host_address();
    CACHED.store(address, Ordering::Relaxed);
    address
}

fn lookup_host_address() -> u32 {
    static mut TABLE: [u8; 16384] = [0; 16384];
    let table = unsafe { &mut *ptr::addr_of_mut!(TABLE) };
    let mut size = table.len() as u32;
    let f: unsafe extern "system" fn(*mut u8, *mut u32, i32) -> u32 =
        unsafe { core::mem::transmute(super::original(b"GetIpAddrTable")) };
    if unsafe { f(table.as_mut_ptr(), &mut size, 0) } != 0 {
        return 0;
    }
    cabinet_row(table.as_ptr()).map_or(0, |i| unsafe {
        ptr::read_unaligned(table.as_ptr().add(4 + i * 24) as *const u32)
    })
}

/// `getsockname`: a socket on the cabinet interface has the cabinet's
/// address. The game finds its own address this way (a UDP socket
/// connected towards the Internet) before looking it up in the address
/// table.
pub unsafe extern "system" fn getsockname(socket: usize, name: *mut u8, length: *mut i32) -> i32 {
    let f: unsafe extern "system" fn(usize, *mut u8, *mut i32) -> i32 =
        unsafe { core::mem::transmute(super::original_ordinal(6)) };
    let result = unsafe { f(socket, name, length) };
    if result == 0
        && !name.is_null()
        && unsafe { ptr::read_unaligned(name as *const u16) } == 2
    {
        let address = unsafe { ptr::read_unaligned(name.add(4) as *const u32) };
        if address != 0 && address & 0xff != 127 && address == host_address() {
            unsafe { ptr::write_unaligned(name.add(4) as *mut u32, ADDRESS) };
            static LOGGED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
            if !LOGGED.swap(true, core::sync::atomic::Ordering::Relaxed) {
                log_address(b"network: local address shown as ", name);
            }
        }
    }
    result
}

fn log_address(what: &[u8], address: *const u8) {
    if address.is_null() {
        return;
    }
    // sockaddr_in: family, port (big-endian), address.
    let family = unsafe { ptr::read_unaligned(address as *const u16) };
    if family != 2 {
        return;
    }
    let port = u16::from_be_bytes(unsafe { [*address.add(2), *address.add(3)] });
    let mut text = [0u8; 32];
    let mut n = 0;
    for i in 0..4 {
        let mut h = [0u8; 16];
        let digits = decimal(u32::from(unsafe { *address.add(4 + i) }), &mut h);
        text[n..n + digits.len()].copy_from_slice(digits);
        n += digits.len();
        text[n] = if i < 3 { b'.' } else { b':' };
        n += 1;
    }
    let mut h = [0u8; 16];
    let digits = decimal(port.into(), &mut h);
    text[n..n + digits.len()].copy_from_slice(digits);
    n += digits.len();
    log(&[what, &text[..n]]);
}

fn decimal(mut value: u32, out: &mut [u8; 16]) -> &[u8] {
    let mut n = 16;
    loop {
        n -= 1;
        out[n] = b'0' + (value % 10) as u8;
        value /= 10;
        if value == 0 {
            break;
        }
    }
    &out[n..]
}

type Seen = [core::sync::atomic::AtomicU64; 16];
static SENT_TO: Seen = [const { core::sync::atomic::AtomicU64::new(0) }; 16];
static RECEIVED_FROM: Seen = [const { core::sync::atomic::AtomicU64::new(0) }; 16];

/// Whether `to` (a sockaddr_in) was not sent to before; up to 16 are kept.
fn new_destination(to: *const u8) -> bool {
    first_time(&SENT_TO, to)
}

/// Whether the sockaddr_in `to` is new to `seen`.
fn first_time(seen: &Seen, to: *const u8) -> bool {
    use core::sync::atomic::Ordering;
    if to.is_null() {
        return false;
    }
    // Port and address, never 0 for a real destination.
    let key = unsafe { ptr::read_unaligned(to.add(2) as *const u64) } & 0xffff_ffff_ffff | 1 << 63;
    for slot in seen {
        match slot.compare_exchange(0, key, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => return true,
            Err(existing) if existing == key => return false,
            Err(_) => {}
        }
    }
    false
}

/// `bind`: a cabinet address is bound as every address, same port.
pub unsafe extern "system" fn bind(socket: usize, address: *const u8, length: i32) -> i32 {
    let f: unsafe extern "system" fn(usize, *const u8, i32) -> i32 =
        unsafe { core::mem::transmute(super::original_ordinal(2)) };
    log_address(b"network: bind ", address);
    if !address.is_null() && length >= 16 && unsafe { ptr::read_unaligned(address as *const u16) } == 2 {
        let host = unsafe { ptr::read_unaligned(address.add(4) as *const u32) };
        if host & MASK == SUBNET {
            let mut any = [0u8; 16];
            unsafe { ptr::copy_nonoverlapping(address, any.as_mut_ptr(), 16) };
            any[4..8].fill(0);
            return unsafe { f(socket, any.as_ptr(), length) };
        }
    }
    unsafe { f(socket, address, length) }
}

#[link(name = "kernel32", kind = "raw-dylib")]
unsafe extern "system" {
    #[link_name = "GetLastError"]
    fn last_error() -> u32;
    #[link_name = "SetLastError"]
    fn set_last_error(code: u32);
}

/// `GetQueuedCompletionStatus`: failed I/O completions (the cabinet link's
/// receive errors arrive here) are logged, then passed on unchanged.
pub unsafe extern "system" fn get_queued_completion_status(
    port: isize,
    bytes: *mut u32,
    key: *mut usize,
    overlapped: *mut *mut c_void,
    ms: u32,
) -> i32 {
    let f: unsafe extern "system" fn(isize, *mut u32, *mut usize, *mut *mut c_void, u32) -> i32 =
        unsafe { core::mem::transmute(super::original(b"GetQueuedCompletionStatus")) };
    let result = unsafe { f(port, bytes, key, overlapped, ms) };
    if result != 0 && !overlapped.is_null() {
        if let Some(from) = take_pending(unsafe { *overlapped }) {
            show_terminal(from);
        }
    }
    if result == 0 && !overlapped.is_null() && !unsafe { *overlapped }.is_null() {
        let error = unsafe { last_error() };
        static LOGGED: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
        if LOGGED.fetch_add(1, core::sync::atomic::Ordering::Relaxed) < 16 {
            let mut h = [0u8; 16];
            log(&[b"network: I/O completion failed, error ", super::hex(error.into(), &mut h)]);
        }
        unsafe { set_last_error(error) };
    }
    result
}

/// `setsockopt`: multicast options naming a cabinet address as the
/// interface (`IP_MULTICAST_IF` = 9, `IP_ADD_MEMBERSHIP`/`IP_DROP_MEMBERSHIP`
/// = 12/13 with the interface after the group) use the default interface.
pub unsafe extern "system" fn setsockopt(socket: usize, level: i32, name: i32, value: *const u8, length: i32) -> i32 {
    let f: unsafe extern "system" fn(usize, i32, i32, *const u8, i32) -> i32 =
        unsafe { core::mem::transmute(super::original_ordinal(21)) };
    let at = match (level, name) {
        (0, 9) if length >= 4 => Some(0),
        (0, 12 | 13) if length >= 8 => Some(4),
        _ => None,
    };
    let result = if let Some(at) = at.filter(|_| !value.is_null()) {
        let mut copy = [0u8; 16];
        let n = (length as usize).min(16);
        unsafe { ptr::copy_nonoverlapping(value, copy.as_mut_ptr(), n) };
        let interface = u32::from_le_bytes(copy[at..at + 4].try_into().unwrap());
        if interface & MASK == SUBNET {
            copy[at..at + 4].fill(0);
        }
        let result = unsafe { f(socket, level, name, copy.as_ptr(), length) };
        let (mut a, mut b, mut c) = ([0u8; 16], [0u8; 16], [0u8; 16]);
        log(&[
            b"network: multicast option ",
            super::hex(name as u32 as u64, &mut a),
            b" interface ",
            super::hex(interface.into(), &mut b),
            b" result ",
            super::hex(result as u32 as u64, &mut c),
        ]);
        result
    } else {
        if level == 0 && (10..=11).contains(&name) && !value.is_null() && length >= 1 {
            let (mut a, mut b) = ([0u8; 16], [0u8; 16]);
            log(&[
                b"network: multicast option ",
                super::hex(name as u32 as u64, &mut a),
                b" value ",
                super::hex(unsafe { *value }.into(), &mut b),
            ]);
        }
        unsafe { f(socket, level, name, value, length) }
    };
    result
}

/// `WSASendTo`: logged (the first sends to each destination), passed through.
pub unsafe extern "system" fn wsa_send_to(
    socket: usize,
    buffers: *const c_void,
    count: u32,
    sent: *mut u32,
    flags: u32,
    to: *const u8,
    length: i32,
    overlapped: *mut c_void,
    routine: *const c_void,
) -> i32 {
    if new_destination(to) {
        log_address(b"network: send to ", to);
        // WSABUF: length (u32, padded to 8), then the data pointer.
        if !buffers.is_null() && count > 0 {
            let len = unsafe { ptr::read_unaligned(buffers as *const u32) } as usize;
            let data = unsafe { ptr::read_unaligned((buffers as *const u8).add(8) as *const *const u8) };
            let mut line = [0u8; 240];
            let mut n = 0;
            for i in 0..len.min(80) {
                let b = unsafe { *data.add(i) };
                for d in [b >> 4, b & 15] {
                    line[n] = b"0123456789abcdef"[usize::from(d)];
                    n += 1;
                }
                line[n] = b' ';
                n += 1;
            }
            let mut h = [0u8; 16];
            log(&[b"network: packet ", super::hex(len as u64, &mut h), b": ", &line[..n]]);
        }
    }
    let f: unsafe extern "system" fn(
        usize,
        *const c_void,
        u32,
        *mut u32,
        u32,
        *const u8,
        i32,
        *mut c_void,
        *const c_void,
    ) -> i32 = unsafe { core::mem::transmute(super::original(b"WSASendTo")) };
    let result = unsafe { f(socket, buffers, count, sent, flags, to, length, overlapped, routine) };
    if is_link_multicast(to) {
        loop_back(socket, buffers, count);
    }
    result
}

/// Whether `to` is a multicast group on the cabinet link's port.
fn is_link_multicast(to: *const u8) -> bool {
    !to.is_null()
        && unsafe { ptr::read_unaligned(to as *const u16) } == 2
        && u16::from_be_bytes(unsafe { [*to.add(2), *to.add(3)] }) == LINK_PORT
        && unsafe { *to.add(4) } & 0xf0 == 0xe0
}

/// The cabinet hears its own link heartbeat (multicast loopback, which the
/// host may not deliver): a copy goes to its own port on loopback.
fn loop_back(socket: usize, buffers: *const c_void, count: u32) {
    let mut data = [0u8; 2048];
    let mut n = 0;
    for i in 0..count as usize {
        // WSABUF: length (u32, padded to 8), then the data pointer.
        let buffer = unsafe { (buffers as *const u8).add(i * 16) };
        let len = unsafe { ptr::read_unaligned(buffer as *const u32) } as usize;
        let bytes = unsafe { ptr::read_unaligned(buffer.add(8) as *const *const u8) };
        if n + len > data.len() {
            return;
        }
        unsafe { ptr::copy_nonoverlapping(bytes, data.as_mut_ptr().add(n), len) };
        n += len;
    }
    let mut to = [0u8; 16];
    to[0] = 2;
    to[2..4].copy_from_slice(&LINK_PORT.to_be_bytes());
    to[4..8].copy_from_slice(&LOOPBACK.to_le_bytes());
    unsafe { sendto(socket, data.as_ptr(), n as i32, 0, to.as_ptr(), 16) };
}

// ---------------------------------------------------------------------------
// The Wangan terminal. A cabinet group has one; this desktop cabinet gets an
// emulated one that broadcasts the terminal heartbeat and operator settings
// every 100 ms, to the cabinet's own link port on this machine only.

#[link(name = "ws2_32", kind = "raw-dylib")]
unsafe extern "system" {
    fn WSAStartup(version: u16, data: *mut u8) -> i32;
    fn socket(family: i32, kind: i32, protocol: i32) -> usize;
    fn sendto(socket: usize, buffer: *const u8, len: i32, flags: i32, to: *const u8, tolen: i32) -> i32;
}
#[link(name = "kernel32", kind = "raw-dylib")]
unsafe extern "system" {
    fn CreateThread(
        attributes: *mut c_void,
        stack: usize,
        start: unsafe extern "system" fn(*mut c_void) -> u32,
        parameter: *mut c_void,
        flags: u32,
        id: *mut u32,
    ) -> isize;
    fn Sleep(ms: u32);
    fn GetSystemTimeAsFileTime(time: *mut u64);
}

/// The emulated terminal's serial: a terminal (cabinet digit 1) of this
/// title, in the form the dongle check accepts.
const TERMINAL_SERIAL: &[u8; 12] = b"280801400002";
const LINK_PORT: u16 = 50765;
/// The cabinet's software revision, which the game computes at boot. A
/// cabinet group runs one revision, so the terminal reports the same.
const REVISION: usize = 0x1_4202_28ac;

/// Unix seconds.
fn unix_time() -> i32 {
    let mut time = 0u64;
    unsafe { GetSystemTimeAsFileTime(&mut time) };
    (time / 10_000_000).saturating_sub(11_644_473_600) as i32
}

pub fn start_terminal() {
    unsafe { CreateThread(ptr::null_mut(), 0, terminal, ptr::null_mut(), 0, ptr::null_mut()) };
}

unsafe extern "system" fn terminal(_: *mut c_void) -> u32 {
    let mut data = [0u8; 512];
    if unsafe { WSAStartup(0x0202, data.as_mut_ptr()) } != 0 {
        log(&[b"terminal: no Winsock"]);
        return 0;
    }
    let s = unsafe { socket(2, 2, 17) };
    if s == usize::MAX {
        log(&[b"terminal: no socket"]);
        return 0;
    }
    let mut node = [0u8; 8];
    let node = super::input::env(b"ARCADE_WMMT6R_TERMINAL_NODE\0", &mut node)
        .and_then(|t| core::str::from_utf8(t).ok()?.parse::<u8>().ok())
        .unwrap_or(4);
    // sockaddr_in 127.0.0.1:50765
    let mut to = [0u8; 16];
    to[0] = 2;
    to[2..4].copy_from_slice(&LINK_PORT.to_be_bytes());
    to[4..8].copy_from_slice(&[127, 0, 0, 1]);
    log(&[b"terminal: broadcasting as ", TERMINAL_SERIAL]);
    let mut frame_number = 0u32;
    loop {
        let mut message = [0u8; 320];
        let terminal = crate::terminal::Terminal {
            frame_number,
            serial: TERMINAL_SERIAL,
            revision: unsafe { ptr::read_volatile(REVISION as *const i32) },
            sync_time: unix_time(),
        };
        let len = crate::terminal::terminal_message(&terminal, &crate::terminal::Settings::DEFAULT, &mut message);
        let mut packet = [0u8; 340];
        let n = crate::terminal::frame(0, node, &message[..len], &mut packet);
        unsafe { sendto(s, packet.as_ptr(), n as i32, 0, to.as_ptr(), 16) };
        frame_number = frame_number.wrapping_add(1);
        unsafe { Sleep(100) };
    }
}

/// Receives in flight: (OVERLAPPED, sender address buffer).
static PENDING: [(core::sync::atomic::AtomicUsize, core::sync::atomic::AtomicUsize); 16] =
    [const { (core::sync::atomic::AtomicUsize::new(0), core::sync::atomic::AtomicUsize::new(0)) }; 16];

fn add_pending(overlapped: *mut c_void, from: *mut u8) {
    use core::sync::atomic::Ordering;
    for (o, f) in &PENDING {
        if o.load(Ordering::Acquire) == overlapped as usize {
            f.store(from as usize, Ordering::Release);
            return;
        }
    }
    // The receive starts after this, so its completion finds the address.
    for (o, f) in &PENDING {
        if o.compare_exchange(0, overlapped as usize, Ordering::AcqRel, Ordering::Relaxed).is_ok() {
            f.store(from as usize, Ordering::Release);
            return;
        }
    }
}

fn take_pending(overlapped: *mut c_void) -> Option<*mut u8> {
    use core::sync::atomic::Ordering;
    if overlapped.is_null() {
        return None;
    }
    PENDING.iter().find_map(|(o, f)| {
        (o.load(Ordering::Acquire) == overlapped as usize).then(|| {
            let from = f.load(Ordering::Acquire) as *mut u8;
            o.store(0, Ordering::Release);
            from
        })
    })
}

/// Link datagrams on this machine arrive from loopback (the emulated
/// terminal, and the cabinet's own heartbeat looped back by
/// [`wsa_send_to`]) or from the host's address (multicast loopback). The
/// cabinet accepts link peers only from private addresses and keys them by
/// address, so these appear to come from the planned addresses instead: the
/// cabinet's own port from the cabinet's address, anything else on
/// loopback from the terminal's.
fn show_terminal(from: *mut u8) {
    if from.is_null() || unsafe { ptr::read_unaligned(from as *const u16) } != 2 {
        return;
    }
    if first_time(&RECEIVED_FROM, from) {
        log_address(b"network: received from ", from);
    }
    let port = u16::from_be_bytes(unsafe { [*from.add(2), *from.add(3)] });
    let address = unsafe { ptr::read_unaligned(from.add(4) as *const u32) };
    let shown = if address == LOOPBACK {
        if port == LINK_PORT { ADDRESS } else { TERMINAL }
    } else if address != 0 && address == host_address() {
        ADDRESS
    } else {
        return;
    };
    unsafe { ptr::write_unaligned(from.add(4) as *mut u32, shown) };
}

/// `WSARecvFrom`: a datagram from loopback appears to come from the
/// terminal's address (when the receive completes, now or through the
/// completion port); the first calls are logged.
pub unsafe extern "system" fn wsa_recv_from(
    socket: usize,
    buffers: *const c_void,
    count: u32,
    received: *mut u32,
    flags: *mut u32,
    from: *mut u8,
    from_length: *mut i32,
    overlapped: *mut c_void,
    routine: *const c_void,
) -> i32 {
    let f: unsafe extern "system" fn(
        usize,
        *const c_void,
        u32,
        *mut u32,
        *mut u32,
        *mut u8,
        *mut i32,
        *mut c_void,
        *const c_void,
    ) -> i32 = unsafe { core::mem::transmute(super::original(b"WSARecvFrom")) };
    if !overlapped.is_null() {
        add_pending(overlapped, from);
    }
    let result = unsafe { f(socket, buffers, count, received, flags, from, from_length, overlapped, routine) };
    if result == 0 {
        show_terminal(from);
    }
    static LOGGED: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
    if LOGGED.fetch_add(1, core::sync::atomic::Ordering::Relaxed) < 6 {
        let mut h = [0u8; 16];
        log(&[
            b"network: recv from result ",
            super::hex(result as u32 as u64, &mut h),
            if overlapped.is_null() { b" (blocking)" } else { b" (overlapped)" },
        ]);
        if result == 0 {
            log_address(b"network: received from ", from);
        }
    }
    result
}

/// `getaddrinfo`: the first names the game looks up are logged with their
/// callers (diagnostics).
pub unsafe extern "system" fn getaddrinfo(node: *const u8, service: *const u8, hints: *const c_void, out: *mut *mut c_void) -> i32 {
    let f: unsafe extern "system" fn(*const u8, *const u8, *const c_void, *mut *mut c_void) -> i32 =
        unsafe { core::mem::transmute(super::original(b"getaddrinfo")) };
    let result = unsafe { f(node, service, hints, out) };
    let text = |p: *const u8| -> &[u8] {
        if p.is_null() {
            return b"-";
        }
        let mut n = 0;
        while n < 200 && unsafe { *p.add(n) } != 0 {
            n += 1;
        }
        unsafe { core::slice::from_raw_parts(p, n) }
    };
    let mut h = [0u8; 16];
    static LOGGED: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
    if LOGGED.fetch_add(1, core::sync::atomic::Ordering::Relaxed) >= 8 {
        return result;
    }
    let mut c = [0u8; 160];
    log(&[
        b"network: getaddrinfo ",
        text(node),
        b" ",
        text(service),
        b" -> ",
        super::hex(result as u32 as u64, &mut h),
        b" <-",
        super::trace::callers(&mut c),
    ]);
    result
}

/// A TCP destination as the host can reach it: the emulated terminal (its
/// game server proxy) is on loopback.
fn reachable(address: *const u8, length: i32, copy: &mut [u8; 16]) -> *const u8 {
    if address.is_null() || length < 16 || unsafe { ptr::read_unaligned(address as *const u16) } != 2 {
        return address;
    }
    if first_time(&SENT_TO, address) {
        log_address(b"network: connect ", address);
    }
    unsafe { ptr::copy_nonoverlapping(address, copy.as_mut_ptr(), 16) };
    if u32::from_le_bytes(copy[4..8].try_into().unwrap()) == TERMINAL {
        copy[4..8].copy_from_slice(&LOOPBACK.to_le_bytes());
        return copy.as_ptr();
    }
    address
}

/// `connect`: see [`reachable`].
pub unsafe extern "system" fn connect(socket: usize, address: *const u8, length: i32) -> i32 {
    let f: unsafe extern "system" fn(usize, *const u8, i32) -> i32 =
        unsafe { core::mem::transmute(super::original_ordinal(4)) };
    let mut copy = [0u8; 16];
    unsafe { f(socket, reachable(address, length, &mut copy), length) }
}

type ConnectEx =
    unsafe extern "system" fn(usize, *const u8, i32, *const c_void, u32, *mut u32, *mut c_void) -> i32;
static CONNECT_EX: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
/// WSAID_CONNECTEX.
const CONNECT_EX_ID: [u8; 16] = [
    0xb9, 0x07, 0xa2, 0x25, 0xf3, 0xdd, 0x60, 0x46, 0x8e, 0xe9, 0x76, 0xe5, 0x8c, 0x74, 0x06, 0x3e,
];

/// The `ConnectEx` the game gets from `WSAIoctl`: see [`reachable`].
unsafe extern "system" fn connect_ex(
    socket: usize,
    address: *const u8,
    length: i32,
    data: *const c_void,
    data_length: u32,
    sent: *mut u32,
    overlapped: *mut c_void,
) -> i32 {
    let f: ConnectEx = unsafe { core::mem::transmute(CONNECT_EX.load(core::sync::atomic::Ordering::Relaxed)) };
    let mut copy = [0u8; 16];
    unsafe { f(socket, reachable(address, length, &mut copy), length, data, data_length, sent, overlapped) }
}

/// `WSAIoctl`: control codes are logged once each (diagnostics).
pub unsafe extern "system" fn wsa_ioctl(
    socket: usize,
    code: u32,
    input: *const u8,
    input_length: u32,
    output: *mut c_void,
    output_length: u32,
    returned: *mut u32,
    overlapped: *mut c_void,
    routine: *const c_void,
) -> i32 {
    let f: unsafe extern "system" fn(usize, u32, *const u8, u32, *mut c_void, u32, *mut u32, *mut c_void, *const c_void) -> i32 =
        unsafe { core::mem::transmute(super::original(b"WSAIoctl")) };
    static LOGGED: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
    if LOGGED.fetch_add(1, core::sync::atomic::Ordering::Relaxed) < 12 {
        let (mut a, mut b) = ([0u8; 16], [0u8; 16]);
        let first = if !input.is_null() && input_length >= 4 {
            unsafe { ptr::read_unaligned(input as *const u32) }
        } else {
            0
        };
        log(&[b"network: WSAIoctl ", super::hex(code.into(), &mut a), b" in ", super::hex(first.into(), &mut b)]);
    }
    let result = unsafe { f(socket, code, input, input_length, output, output_length, returned, overlapped, routine) };
    // SIO_GET_EXTENSION_FUNCTION_POINTER for ConnectEx: hand out the wrapper.
    if result == 0
        && code == 0xc800_0006
        && input_length >= 16
        && !input.is_null()
        && unsafe { core::slice::from_raw_parts(input, 16) } == CONNECT_EX_ID
        && !output.is_null()
        && output_length >= 8
    {
        let original = unsafe { ptr::read_unaligned(output as *const usize) };
        CONNECT_EX.store(original, core::sync::atomic::Ordering::Relaxed);
        unsafe { ptr::write_unaligned(output as *mut usize, connect_ex as *const () as usize) };
    }
    result
}
