//! The Wangan terminal's HTTPS endpoint.
//!
//! Drive cabinets send their game server traffic to the terminal (TCP, the
//! link port 50765) over TLS; the terminal answers or forwards it. This
//! terminal answers it itself (`crate::game_server`), on loopback only: the
//! hook routes the cabinet's connections to the terminal's address here
//! (`network.rs`). TLS is `crate::tls` with the certificate from
//! `crate::pki`, which the plan's CA file makes trusted.
use super::log;
use core::{ffi::c_void, ptr};

/// The terminal's port (TCP).
pub const PORT: u16 = 50765;
/// The game server's URI as the auth service reports it: cabinets accept
/// only an `https` URI, and send its traffic through their terminal.
pub const URI: &[u8] = b"https://127.0.0.1/\0";

#[link(name = "ws2_32", kind = "raw-dylib")]
unsafe extern "system" {
    fn WSAStartup(version: u16, data: *mut u8) -> i32;
    fn socket(family: i32, kind: i32, protocol: i32) -> usize;
    fn bind(socket: usize, address: *const u8, length: i32) -> i32;
    fn listen(socket: usize, backlog: i32) -> i32;
    fn accept(socket: usize, address: *mut u8, length: *mut i32) -> usize;
    fn recv(socket: usize, buffer: *mut u8, length: i32, flags: i32) -> i32;
    fn send(socket: usize, buffer: *const u8, length: i32, flags: i32) -> i32;
    fn closesocket(socket: usize) -> i32;
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
    fn CloseHandle(handle: isize) -> i32;
}
#[link(name = "bcrypt", kind = "raw-dylib")]
unsafe extern "system" {
    fn BCryptGenRandom(algorithm: *mut c_void, buffer: *mut u8, length: u32, flags: u32) -> i32;
}

/// Stack for each connection: TLS records and HTTP messages live on it.
const STACK: usize = 2 << 20;

pub fn start() {
    let thread = unsafe { CreateThread(ptr::null_mut(), 0, serve, ptr::null_mut(), 0, ptr::null_mut()) };
    unsafe { CloseHandle(thread) };
}

unsafe extern "system" fn serve(_: *mut c_void) -> u32 {
    let mut data = [0u8; 512];
    if unsafe { WSAStartup(0x0202, data.as_mut_ptr()) } != 0 {
        log(&[b"terminal server: no Winsock"]);
        return 0;
    }
    let listener = unsafe { socket(2, 1, 6) };
    let mut address = [0u8; 16];
    address[0] = 2;
    address[2..4].copy_from_slice(&PORT.to_be_bytes());
    address[4..8].copy_from_slice(&[127, 0, 0, 1]);
    if listener == usize::MAX
        || unsafe { bind(listener, address.as_ptr(), 16) } != 0
        || unsafe { listen(listener, 16) } != 0
    {
        log(&[b"terminal server: can't listen"]);
        return 0;
    }
    loop {
        let client = unsafe { accept(listener, ptr::null_mut(), ptr::null_mut()) };
        if client == usize::MAX {
            continue;
        }
        let thread =
            unsafe { CreateThread(ptr::null_mut(), STACK, connection, client as *mut c_void, 0, ptr::null_mut()) };
        if thread == 0 {
            unsafe { closesocket(client) };
        } else {
            unsafe { CloseHandle(thread) };
        }
    }
}

struct Socket(usize);

impl crate::tls::Transport for Socket {
    fn read(&mut self, buffer: &mut [u8]) -> Option<usize> {
        let n = unsafe { recv(self.0, buffer.as_mut_ptr(), buffer.len().min(1 << 30) as i32, 0) };
        (n > 0).then_some(n as usize)
    }
    fn write(&mut self, mut data: &[u8]) -> bool {
        while !data.is_empty() {
            let n = unsafe { send(self.0, data.as_ptr(), data.len().min(1 << 30) as i32, 0) };
            if n <= 0 {
                return false;
            }
            data = &data[n as usize..];
        }
        true
    }
}

fn random() -> [u8; 32] {
    let mut bytes = [0u8; 32];
    // BCRYPT_USE_SYSTEM_PREFERRED_RNG
    unsafe { BCryptGenRandom(ptr::null_mut(), bytes.as_mut_ptr(), 32, 2) };
    bytes
}

unsafe extern "system" fn connection(socket: *mut c_void) -> u32 {
    let mut io = Socket(socket as usize);
    serve_connection(&mut io);
    unsafe { closesocket(io.0) };
    0
}

/// The largest request the terminal takes (headers and body).
const REQUEST: usize = 512 * 1024;

fn serve_connection(io: &mut Socket) {
    let (mut leaf, mut root) = ([0u8; 800], [0u8; 800]);
    let l = crate::pki::terminal_der(&mut leaf);
    let r = crate::pki::ca_der(&mut root);
    let ephemeral = loop {
        // A valid P-256 scalar (nearly all 32 random bytes are).
        let key = random();
        if p256::SecretKey::from_bytes(&key.into()).is_ok() {
            break key;
        }
    };
    let mut session = match crate::tls::Session::accept(
        io,
        &random(),
        &ephemeral,
        &[&leaf[..l], &root[..r]],
        &crate::pki::terminal_key(),
    ) {
        Ok(session) => session,
        Err(e) => {
            log_error(b"terminal server: handshake failed: ", &e);
            return;
        }
    };
    // Requests are read into a shared buffer, one connection at a time.
    static mut REQUESTS: [u8; REQUEST] = [0; REQUEST];
    static BUSY: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
    let mut buffer: &mut [u8] = &mut [];
    let mut len = 0;
    'requests: loop {
        // Headers.
        let header_end = loop {
            if let Some(end) = find(&buffer[..len], b"\r\n\r\n") {
                break end + 4;
            }
            if buffer.is_empty() {
                // Waits for the buffer only once data arrives.
                let mut first = [0u8; 16 * 1024 + 256];
                let n = match session.read(io, &mut first) {
                    Ok(0) | Err(_) => break 'requests,
                    Ok(n) => n,
                };
                while BUSY.swap(true, core::sync::atomic::Ordering::Acquire) {
                    core::hint::spin_loop();
                }
                buffer = unsafe { &mut *ptr::addr_of_mut!(REQUESTS) };
                buffer[..n].copy_from_slice(&first[..n]);
                len = n;
                continue;
            }
            if len == buffer.len() {
                break 'requests;
            }
            match session.read(io, &mut buffer[len..]) {
                Ok(0) | Err(_) => break 'requests,
                Ok(n) => len += n,
            }
        };
        let content_length = header(&buffer[..header_end], b"content-length")
            .and_then(|v| core::str::from_utf8(v).ok()?.trim().parse::<usize>().ok())
            .unwrap_or(0);
        if header_end + content_length > buffer.len() {
            log(&[b"terminal server: request too large"]);
            break;
        }
        while len < header_end + content_length {
            match session.read(io, &mut buffer[len..]) {
                Ok(0) | Err(_) => break 'requests,
                Ok(n) => len += n,
            }
        }
        let line_end = find(&buffer[..header_end], b"\r\n").unwrap_or(0);
        let path = buffer[..line_end].split(|&b| b == b' ').nth(1).unwrap_or(b"");
        if !answer(&mut session, io, path, &buffer[header_end..header_end + content_length]) {
            break;
        }
        buffer.copy_within(header_end + content_length..len, 0);
        len -= header_end + content_length;
        if len == 0 {
            // Idle connections leave the buffer to others.
            buffer = &mut [];
            BUSY.store(false, core::sync::atomic::Ordering::Release);
        }
    }
    if !buffer.is_empty() {
        BUSY.store(false, core::sync::atomic::Ordering::Release);
    }
    session.close(io);
}

/// Answers one request; `false` if the connection failed.
fn answer(session: &mut crate::tls::Session, io: &mut Socket, path: &[u8], body: &[u8]) -> bool {
    let mut response = [0u8; 16 * 1024];
    let request = crate::game_server::request(path);
    let result = request.and_then(|r| crate::game_server::answer(&r, body, &mut response));
    let mut h = [0u8; 16];
    log(&[
        b"terminal server: ",
        path,
        b" (",
        super::hex(body.len() as u64, &mut h),
        b" bytes) ",
        if result.is_some() { b"answered" } else { b"not handled" },
    ]);
    if result.is_none() {
        log_hex(b"terminal server: request ", body);
    }
    let mut head = [0u8; 256];
    let mut n = 0;
    let mut put = |bytes: &[u8]| {
        head[n..n + bytes.len()].copy_from_slice(bytes);
        n += bytes.len();
    };
    let length = result.unwrap_or(0);
    match result {
        Some(_) => put(b"HTTP/1.1 200 OK\r\nContent-Type: application/x-protobuf\r\n"),
        None => put(b"HTTP/1.1 404 Not Found\r\n"),
    }
    put(b"Content-Length: ");
    let mut digits = [0u8; 20];
    put(decimal(length, &mut digits));
    put(b"\r\nConnection: keep-alive\r\n\r\n");
    session.write(io, &head[..n]).is_ok() && session.write(io, &response[..length]).is_ok()
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// A header's value (name compared without case).
fn header<'a>(head: &'a [u8], name: &[u8]) -> Option<&'a [u8]> {
    head.split(|&b| b == b'\n').find_map(|line| {
        let colon = line.iter().position(|&b| b == b':')?;
        let value = &line[colon + 1..];
        line[..colon].eq_ignore_ascii_case(name).then(|| value.strip_suffix(b"\r").unwrap_or(value))
    })
}

fn decimal(mut value: usize, out: &mut [u8; 20]) -> &[u8] {
    let mut n = out.len();
    loop {
        n -= 1;
        out[n] = b'0' + (value % 10) as u8;
        value /= 10;
        if value == 0 {
            return &out[n..];
        }
    }
}

fn log_error(what: &[u8], error: &crate::tls::Error) {
    let (mut a, mut b) = ([0u8; 16], [0u8; 16]);
    match error {
        crate::tls::Error::Closed => log(&[what, b"connection closed"]),
        crate::tls::Error::Protocol(reason) => log(&[what, reason.as_bytes()]),
        crate::tls::Error::Alert(level, code) => log(&[
            what,
            b"alert ",
            super::hex((*level).into(), &mut a),
            b"/",
            super::hex((*code).into(), &mut b),
        ]),
    }
}

fn log_hex(what: &[u8], data: &[u8]) {
    let mut line = [0u8; 3 * 96];
    let mut n = 0;
    for &b in data.iter().take(96) {
        for d in [b >> 4, b & 15] {
            line[n] = b"0123456789abcdef"[usize::from(d)];
            n += 1;
        }
        line[n] = b' ';
        n += 1;
    }
    log(&[what, &line[..n]]);
}
