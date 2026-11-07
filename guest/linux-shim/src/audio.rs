//! Minimal OSS playback device backed by PulseAudio / pipewire-pulse.
//! Non-audio descriptors go directly to the kernel, avoiding interposition recursion.
//! Under x86 translators without raw-syscall support for a call (box32 returns
//! ENOSYS for openat/ioctl), the next libc definition is used instead.
use std::{
    ffi::{CStr, c_char, c_int, c_void},
    sync::{
        Mutex,
        atomic::{AtomicI32, Ordering},
    },
};

#[repr(C)]
struct SampleSpec {
    format: c_int,
    rate: u32,
    channels: u8,
}
#[repr(C)]
struct BufferAttr {
    maxlength: u32,
    tlength: u32,
    prebuf: u32,
    minreq: u32,
    fragsize: u32,
}
#[link(name = "pulse-simple")]
unsafe extern "C" {
    fn pa_simple_new(
        server: *const c_char,
        name: *const c_char,
        dir: c_int,
        dev: *const c_char,
        stream: *const c_char,
        spec: *const SampleSpec,
        map: *const c_void,
        attr: *const BufferAttr,
        error: *mut c_int,
    ) -> *mut c_void;
    fn pa_simple_write(
        stream: *mut c_void,
        data: *const c_void,
        bytes: usize,
        error: *mut c_int,
    ) -> c_int;
    fn pa_simple_drain(stream: *mut c_void, error: *mut c_int) -> c_int;
    fn pa_simple_flush(stream: *mut c_void, error: *mut c_int) -> c_int;
    fn pa_simple_free(stream: *mut c_void);
}
static FD: AtomicI32 = AtomicI32::new(-1);
struct Device {
    stream: usize,
    rate: u32,
    channels: u8,
    fragment: i32,
    fragments: i32,
    bytes: u64,
    logged: bool,
    /// Android: connected PCM socket to the app (ARCADE_AUDIO_SOCKET), or -1.
    socket: c_int,
}
static DEVICE: Mutex<Device> = Mutex::new(Device {
    stream: 0,
    rate: 24000,
    channels: 2,
    fragment: 1024,
    fragments: 8,
    bytes: 0,
    logged: false,
    socket: -1,
});
/// Raw syscall first; on ENOSYS (translated guests) the next libc symbol.
fn passthrough(
    raw: impl FnOnce() -> libc::c_long,
    name: &CStr,
    call: impl FnOnce(usize) -> libc::c_long,
) -> libc::c_long {
    use std::sync::atomic::AtomicBool;
    static USE_LIBC: AtomicBool = AtomicBool::new(false);
    if !USE_LIBC.load(Ordering::Relaxed) {
        let result = raw();
        if result != -1 || unsafe { *libc::__errno_location() } != libc::ENOSYS {
            return result;
        }
        USE_LIBC.store(true, Ordering::Relaxed);
        eprintln!("[arcade] raw syscalls unavailable; using libc I/O");
    }
    let next = unsafe { libc::dlsym(libc::RTLD_NEXT, name.as_ptr()) };
    if next.is_null() {
        return fail(libc::ENOSYS) as libc::c_long;
    }
    call(next as usize)
}

fn sys_openat(path: *const c_char, flags: c_int, mode: usize) -> c_int {
    passthrough(
        || unsafe { libc::syscall(libc::SYS_openat, libc::AT_FDCWD, path, flags, mode) },
        // libc open, not openat: translators special-case paths such as
        // /proc/cpuinfo (x86 CPU description) in their open wrapper.
        c"open",
        |f| unsafe {
            let f: unsafe extern "C" fn(*const c_char, c_int, ...) -> c_int =
                std::mem::transmute(f);
            f(path, flags, mode as libc::c_uint) as libc::c_long
        },
    ) as c_int
}

fn sys_ioctl(fd: c_int, request: u32, arg: *mut i32) -> c_int {
    passthrough(
        || unsafe { libc::syscall(libc::SYS_ioctl, fd, request, arg) },
        c"ioctl",
        |f| unsafe {
            let f: unsafe extern "C" fn(c_int, libc::c_ulong, ...) -> c_int =
                std::mem::transmute(f);
            f(fd, request as libc::c_ulong, arg) as libc::c_long
        },
    ) as c_int
}

fn sys_write(fd: c_int, data: *const c_void, bytes: usize) -> isize {
    passthrough(
        || unsafe { libc::syscall(libc::SYS_write, fd, data, bytes) },
        c"write",
        |f| unsafe {
            let f: unsafe extern "C" fn(c_int, *const c_void, usize) -> isize =
                std::mem::transmute(f);
            f(fd, data, bytes) as libc::c_long
        },
    ) as isize
}

fn sys_close(fd: c_int) -> c_int {
    passthrough(
        || unsafe { libc::syscall(libc::SYS_close, fd) },
        c"close",
        |f| unsafe {
            let f: unsafe extern "C" fn(c_int) -> c_int = std::mem::transmute(f);
            f(fd) as libc::c_long
        },
    ) as c_int
}

fn fail(errno: c_int) -> c_int {
    unsafe {
        *libc::__errno_location() = errno;
    }
    -1
}

/// # Safety
/// C open() arguments, including mode when O_CREAT/O_TMPFILE requires it.
#[unsafe(no_mangle)]
#[unsafe(naked)]
// The naked entry preserves the full optional third argument on the i386
// caller's stack. Rust cannot spell a defined C variadic function on stable.
#[allow(invalid_runtime_symbol_definitions)]
pub unsafe extern "C" fn open(_path: *const c_char, _flags: c_int) -> c_int {
    core::arch::naked_asm!("jmp arcade_oss_open_bridge");
}
/// # Safety
/// C ioctl() arguments; its optional argument is decoded from the original stack.
#[unsafe(no_mangle)]
#[unsafe(naked)]
pub unsafe extern "C" fn ioctl(_fd: c_int, _request: u32) -> c_int {
    core::arch::naked_asm!("jmp arcade_oss_ioctl_bridge");
}
core::arch::global_asm!(
    r#"
.hidden arcade_oss_open
.hidden arcade_oss_ioctl
.hidden arcade_oss_open_bridge
.hidden arcade_oss_ioctl_bridge
arcade_oss_open_bridge:
    push ebp
    mov ebp,esp
    and esp,-16
    sub esp,12
    lea eax,[ebp+8]
    push eax
    call arcade_oss_open
    mov esp,ebp
    pop ebp
    ret
arcade_oss_ioctl_bridge:
    push ebp
    mov ebp,esp
    and esp,-16
    sub esp,12
    lea eax,[ebp+8]
    push eax
    call arcade_oss_ioctl
    mov esp,ebp
    pop ebp
    ret
"#
);

#[unsafe(no_mangle)]
unsafe extern "C" fn arcade_oss_open(args: *const usize) -> c_int {
    let path = unsafe { *args } as *const c_char;
    let flags = unsafe { *args.add(1) } as c_int;
    let mode = if flags & libc::O_CREAT != 0 || flags & libc::O_TMPFILE == libc::O_TMPFILE {
        unsafe { *args.add(2) }
    } else {
        0
    };
    if !path.is_null()
        && unsafe { CStr::from_ptr(path) }.to_bytes() == b"/dev/dsp"
        && std::env::var_os("ARCADE_AUDIO").is_some_and(|v| v == "1")
    {
        return open_device();
    }
    sys_openat(path, flags, mode)
}

fn open_device() -> c_int {
    let mut device = DEVICE.lock().unwrap_or_else(|e| e.into_inner());
    if FD.load(Ordering::Relaxed) >= 0 {
        return fail(libc::EBUSY);
    }
    let fd = sys_openat(c"/dev/null".as_ptr(), libc::O_WRONLY | libc::O_CLOEXEC, 0);
    if fd >= 0 {
        *device = Device {
            stream: 0,
            rate: 24000,
            channels: 2,
            fragment: 1024,
            fragments: 8,
            bytes: 0,
            logged: false,
            socket: -1,
        };
        FD.store(fd, Ordering::Relaxed);
        eprintln!("[arcade audio] OSS playback device opened (fd {fd})");
    }
    fd
}

#[unsafe(no_mangle)]
unsafe extern "C" fn arcade_oss_ioctl(args: *const usize) -> c_int {
    let fd = unsafe { *args } as c_int;
    let request = unsafe { *args.add(1) } as u32;
    let arg = unsafe { *args.add(2) } as *mut i32;
    if fd < 0 || fd != FD.load(Ordering::Relaxed) {
        return sys_ioctl(fd, request, arg);
    }
    let mut device = DEVICE.lock().unwrap_or_else(|e| e.into_inner());
    if request >> 8 & 255 == b'M' as u32 {
        // Cabinet mixer writes are device-local; do not change the host master volume.
        return if arg.is_null() { fail(libc::EFAULT) } else { 0 };
    }
    if request != 0x5000
        && request != 0x5001
        && request != 0x5008
        && request != 0x500e
        && arg.is_null()
    {
        return fail(libc::EFAULT);
    }
    match request {
        0x5000 => {
            if device.stream != 0 {
                unsafe {
                    pa_simple_flush(device.stream as *mut c_void, std::ptr::null_mut());
                }
            }
            device.bytes = 0;
            0
        }
        0x5001 => {
            if device.stream == 0 {
                0
            } else {
                unsafe { pa_simple_drain(device.stream as *mut c_void, std::ptr::null_mut()) }
            }
        }
        0x5008 | 0x500e => 0,
        0x8004500b => {
            unsafe {
                *arg = 0x10;
            }
            0
        } // AFMT_S16_LE only.
        0xc0045005 => {
            if unsafe { *arg } != 0 && unsafe { *arg } != 0x10 {
                return fail(libc::EINVAL);
            }
            unsafe {
                *arg = 0x10;
            }
            0
        }
        0xc0045006 | 0xc0045003 => {
            let channels = if request == 0xc0045003 {
                (unsafe { *arg }) + 1
            } else {
                unsafe { *arg }
            };
            if !(1..=2).contains(&channels) || device.stream != 0 {
                return fail(libc::EINVAL);
            }
            device.channels = channels as u8;
            0
        }
        0xc0045002 => {
            let rate = unsafe { *arg };
            if !(8000..=192000).contains(&rate) || device.stream != 0 {
                return fail(libc::EINVAL);
            }
            device.rate = rate as u32;
            eprintln!(
                "[arcade audio] negotiated {} Hz / {} channels / S16LE",
                device.rate, device.channels
            );
            0
        }
        0xc004500a => {
            let value = unsafe { *arg } as u32;
            device.fragment = 1i32 << ((value & 65535).clamp(8, 14));
            device.fragments = ((value >> 16).clamp(2, 32)) as i32;
            0
        }
        0xc0045004 => {
            unsafe {
                *arg = device.fragment;
            }
            0
        }
        0x8010500c => {
            unsafe {
                *arg = device.fragments;
                *arg.add(1) = device.fragments;
                *arg.add(2) = device.fragment;
                *arg.add(3) = device.fragments * device.fragment;
            }
            0
        }
        0x8004500f => {
            unsafe {
                *arg = 0;
            }
            0
        } // No mmap, duplex or trigger claims.
        0x80045017 => {
            unsafe {
                *arg = 0;
            }
            0
        }
        _ => {
            eprintln!("[arcade audio] unsupported OSS ioctl {request:#x}");
            fail(libc::ENOTTY)
        }
    }
}

/// # Safety
/// Same buffer contract as POSIX write; the game must provide `bytes` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn write(fd: c_int, data: *const c_void, bytes: usize) -> isize {
    if fd < 0 || fd != FD.load(Ordering::Relaxed) {
        return sys_write(fd, data, bytes);
    }
    if bytes == 0 {
        return 0;
    }
    if data.is_null() {
        return fail(libc::EFAULT) as isize;
    }
    let mut device = DEVICE.lock().unwrap_or_else(|e| e.into_inner());
    if !bytes.is_multiple_of(device.channels as usize * 2) {
        return fail(libc::EINVAL) as isize;
    }
    if let Some(name) = std::env::var_os("ARCADE_AUDIO_SOCKET") {
        let pcm = unsafe { std::slice::from_raw_parts(data.cast::<u8>(), bytes) };
        socket_write(&mut device, name.as_encoded_bytes(), pcm);
        device.bytes += bytes as u64;
        return bytes as isize;
    }
    let mut error = 0;
    if device.stream == 0 {
        let spec = SampleSpec {
            format: 3,
            rate: device.rate,
            channels: device.channels,
        };
        let buffer = BufferAttr {
            maxlength: u32::MAX,
            tlength: device.rate * u32::from(device.channels) * 2 / 10,
            prebuf: u32::MAX,
            minreq: device.fragment as u32,
            fragsize: u32::MAX,
        };
        let stream = unsafe {
            pa_simple_new(
                std::ptr::null(),
                c"Banana pepper".as_ptr(),
                1,
                std::ptr::null(),
                c"Game audio".as_ptr(),
                &spec,
                std::ptr::null(),
                &buffer,
                &mut error,
            )
        };
        if stream.is_null() {
            eprintln!("[arcade audio] Pulse connection failed ({error})");
            return fail(libc::EIO) as isize;
        }
        device.stream = stream as usize;
        eprintln!("[arcade audio] connected to desktop PulseAudio/PipeWire playback");
    }
    if unsafe { pa_simple_write(device.stream as *mut c_void, data, bytes, &mut error) } < 0 {
        eprintln!("[arcade audio] playback failed ({error})");
        return fail(libc::EIO) as isize;
    }
    device.bytes += bytes as u64;
    if !device.logged
        && unsafe { std::slice::from_raw_parts(data.cast::<u8>(), bytes) }
            .iter()
            .any(|&v| v != 0)
    {
        device.logged = true;
        eprintln!("[arcade audio] non-silent PCM submitted to desktop audio");
    }
    bytes as isize
}

/// Android app audio: raw S16LE PCM over an abstract Unix stream socket.
/// The app's blocking AudioTrack paces playback; a small send buffer keeps
/// latency low. Header: b"AFA1", rate (u32 LE), channels (u32 LE).
/// Without a listener, audio is dropped but writes are still paced in real time.
fn socket_write(device: &mut Device, name: &[u8], mut pcm: &[u8]) {
    let frame_bytes = u64::from(device.channels) * 2;
    if device.socket < 0 {
        device.socket = connect_audio(name, device.rate, device.channels);
        if device.socket >= 0 {
            eprintln!("[arcade audio] streaming PCM to the app");
        }
    }
    while device.socket >= 0 && !pcm.is_empty() {
        let sent = unsafe {
            libc::send(
                device.socket,
                pcm.as_ptr().cast(),
                pcm.len(),
                libc::MSG_NOSIGNAL,
            )
        };
        if sent < 0 {
            if unsafe { *libc::__errno_location() } == libc::EINTR {
                continue;
            }
            eprintln!("[arcade audio] app audio socket closed");
            sys_close(device.socket);
            device.socket = -1;
            break;
        }
        pcm = &pcm[sent as usize..];
    }
    if device.socket < 0 && !pcm.is_empty() {
        let micros = pcm.len() as u64 / frame_bytes * 1_000_000 / u64::from(device.rate.max(1));
        std::thread::sleep(std::time::Duration::from_micros(micros));
    }
}

fn connect_audio(name: &[u8], rate: u32, channels: u8) -> c_int {
    let fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM | libc::SOCK_CLOEXEC, 0) };
    if fd < 0 {
        return -1;
    }
    let buffer: c_int = 8192;
    unsafe {
        libc::setsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_SNDBUF,
            (&buffer as *const c_int).cast(),
            std::mem::size_of::<c_int>() as libc::socklen_t,
        );
    }
    let mut addr: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    addr.sun_family = libc::AF_UNIX as libc::sa_family_t;
    let len = name.len().min(addr.sun_path.len() - 1);
    for (slot, &byte) in addr.sun_path[1..=len].iter_mut().zip(name) {
        *slot = byte as c_char; // sun_path[0] == 0: abstract namespace
    }
    let addr_len = (std::mem::size_of::<libc::sa_family_t>() + 1 + len) as libc::socklen_t;
    let mut header = [0u8; 12];
    header[..4].copy_from_slice(b"AFA1");
    header[4..8].copy_from_slice(&rate.to_le_bytes());
    header[8..].copy_from_slice(&u32::from(channels).to_le_bytes());
    let ok = unsafe { libc::connect(fd, (&addr as *const libc::sockaddr_un).cast(), addr_len) }
        == 0
        && unsafe { libc::send(fd, header.as_ptr().cast(), header.len(), libc::MSG_NOSIGNAL) }
            == header.len() as isize;
    if !ok {
        sys_close(fd);
        return -1;
    }
    fd
}

#[unsafe(no_mangle)]
pub extern "C" fn close(fd: c_int) -> c_int {
    if fd >= 0
        && FD
            .compare_exchange(fd, -1, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
    {
        let mut device = DEVICE.lock().unwrap_or_else(|e| e.into_inner());
        if device.stream != 0 {
            unsafe {
                pa_simple_free(device.stream as *mut c_void);
            }
            device.stream = 0;
        }
        if device.socket >= 0 {
            sys_close(device.socket);
            device.socket = -1;
        }
        eprintln!("[arcade audio] closed after {} PCM bytes", device.bytes);
    }
    sys_close(fd)
}

#[cfg(test)]
mod tests {
    use super::*;
    unsafe extern "C" {
        #[link_name = "ioctl"]
        fn abi_ioctl(fd: c_int, request: u32, ...) -> c_int;
        #[link_name = "open"]
        fn abi_open(path: *const c_char, flags: c_int, ...) -> c_int;
    }
    #[test]
    fn oss_negotiation_and_normal_file_calls_preserve_the_c_abi() {
        let fd = open_device();
        assert!(fd >= 0);
        let mut value = 0i32;
        assert_eq!(unsafe { abi_ioctl(fd, 0x8004500b, &mut value) }, 0);
        assert_eq!(value, 0x10);
        value = 2;
        assert_eq!(unsafe { abi_ioctl(fd, 0xc0045006, &mut value) }, 0);
        value = 24000;
        assert_eq!(unsafe { abi_ioctl(fd, 0xc0045002, &mut value) }, 0);
        value = 0x20;
        assert_eq!(unsafe { abi_ioctl(fd, 0xc0045005, &mut value) }, -1);
        assert_eq!(unsafe { *libc::__errno_location() }, libc::EINVAL);
        let mut info = [0i32; 4];
        assert_eq!(unsafe { abi_ioctl(fd, 0x8010500c, info.as_mut_ptr()) }, 0);
        assert_eq!(info, [8, 8, 1024, 8192]);
        assert_eq!(
            unsafe { abi_ioctl(fd, 0x8004500b, std::ptr::null_mut::<i32>()) },
            -1
        );
        assert_eq!(unsafe { *libc::__errno_location() }, libc::EFAULT);
        assert_eq!(unsafe { abi_ioctl(fd, 0x1234, 0usize) }, -1);
        assert_eq!(close(fd), 0);
        // Ordinary file I/O must continue working through the exported hooks.
        let path = std::ffi::CString::new(format!("/tmp/arcade-audio-abi-{}", std::process::id()))
            .unwrap();
        let normal = unsafe {
            abi_open(
                path.as_ptr(),
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
                0o600u32,
            )
        };
        assert!(normal >= 0);
        assert_eq!(unsafe { write(normal, b"test".as_ptr().cast(), 4) }, 4);
        assert_eq!(close(normal), 0);
        assert_eq!(std::fs::read(path.to_str().unwrap()).unwrap(), b"test");
        std::fs::remove_file(path.to_str().unwrap()).unwrap();
    }
}
