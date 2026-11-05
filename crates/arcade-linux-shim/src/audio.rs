//! Minimal OSS playback device backed by PulseAudio / pipewire-pulse.
//! Non-audio descriptors go directly to the kernel, avoiding interposition recursion.
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
}
static DEVICE: Mutex<Device> = Mutex::new(Device {
    stream: 0,
    rate: 24000,
    channels: 2,
    fragment: 1024,
    fragments: 8,
    bytes: 0,
    logged: false,
});
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
        && std::env::var_os("ARCADE_BBH_AUDIO").is_some_and(|v| v == "1")
    {
        return open_device();
    }
    (unsafe { libc::syscall(libc::SYS_openat, libc::AT_FDCWD, path, flags, mode) }) as c_int
}

fn open_device() -> c_int {
    let mut device = DEVICE.lock().unwrap_or_else(|e| e.into_inner());
    if FD.load(Ordering::Relaxed) >= 0 {
        return fail(libc::EBUSY);
    }
    let fd = unsafe {
        libc::syscall(
            libc::SYS_openat,
            libc::AT_FDCWD,
            c"/dev/null".as_ptr(),
            libc::O_WRONLY | libc::O_CLOEXEC,
            0,
        )
    } as c_int;
    if fd >= 0 {
        *device = Device {
            stream: 0,
            rate: 24000,
            channels: 2,
            fragment: 1024,
            fragments: 8,
            bytes: 0,
            logged: false,
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
        return unsafe { libc::syscall(libc::SYS_ioctl, fd, request, arg) } as c_int;
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
        return unsafe { libc::syscall(libc::SYS_write, fd, data, bytes) } as isize;
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
                c"Arcade Foundry".as_ptr(),
                1,
                std::ptr::null(),
                c"Big Buck Hunter Pro".as_ptr(),
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
        eprintln!("[arcade audio] closed after {} PCM bytes", device.bytes);
    }
    (unsafe { libc::syscall(libc::SYS_close, fd) }) as c_int
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
