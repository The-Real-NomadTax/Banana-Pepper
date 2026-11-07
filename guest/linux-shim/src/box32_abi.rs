//! glibc entrypoints missing from box32's wrapped symbol table.
//! These functions retain libc semantics and can also run in an ordinary guest.
use std::ffi::{c_char, c_int, c_void};

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sysv_signal(
    signal: c_int,
    handler: libc::sighandler_t,
) -> libc::sighandler_t {
    let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
    let mut previous: libc::sigaction = unsafe { std::mem::zeroed() };
    action.sa_sigaction = handler;
    action.sa_flags = libc::SA_RESETHAND | libc::SA_NODEFER;
    unsafe { libc::sigemptyset(&mut action.sa_mask) };
    if unsafe { libc::sigaction(signal, &action, &mut previous) } < 0 {
        libc::SIG_ERR
    } else {
        previous.sa_sigaction
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ttyname_r(fd: c_int, buffer: *mut c_char, length: usize) -> c_int {
    let original = unsafe { libc::dlsym(libc::RTLD_NEXT, c"ttyname_r".as_ptr()) };
    if !original.is_null() && original != ttyname_r as *const () as *mut c_void {
        let original: unsafe extern "C" fn(c_int, *mut c_char, usize) -> c_int =
            unsafe { std::mem::transmute(original) };
        return unsafe { original(fd, buffer, length) };
    }
    if unsafe { libc::isatty(fd) } == 0 {
        return libc::ENOTTY;
    }
    let ttyname = unsafe { libc::dlsym(libc::RTLD_NEXT, c"ttyname".as_ptr()) };
    if ttyname.is_null() {
        return libc::ENOTTY;
    }
    let ttyname: unsafe extern "C" fn(c_int) -> *mut c_char =
        unsafe { std::mem::transmute(ttyname) };
    let name = unsafe { ttyname(fd) };
    if name.is_null() {
        return unsafe { *libc::__errno_location() };
    }
    let size = unsafe { libc::strlen(name) } + 1;
    if size > length {
        return libc::ERANGE;
    }
    unsafe { std::ptr::copy_nonoverlapping(name, buffer, size) };
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn getutent_r(buffer: *mut c_void, result: *mut *mut c_void) -> c_int {
    let original = unsafe { libc::dlsym(libc::RTLD_NEXT, c"getutent_r".as_ptr()) };
    if !original.is_null() && original != getutent_r as *const () as *mut c_void {
        let original: unsafe extern "C" fn(*mut c_void, *mut *mut c_void) -> c_int =
            unsafe { std::mem::transmute(original) };
        return unsafe { original(buffer, result) };
    }
    // Android's private Linux rootfs has no host login records.
    if !result.is_null() {
        unsafe { *result = std::ptr::null_mut() };
    }
    unsafe {
        *libc::__errno_location() = libc::ENOENT;
    }
    -1
}

pub fn resolve(name: &std::ffi::CStr) -> *mut c_void {
    match name.to_bytes() {
        b"__fwprintf_chk" => __fwprintf_chk as *const () as *mut c_void,
        b"wcstoq" => unsafe { libc::dlsym(libc::RTLD_DEFAULT, c"wcstoll".as_ptr()) },
        b"wcstouq" => unsafe { libc::dlsym(libc::RTLD_DEFAULT, c"wcstoull".as_ptr()) },
        b"settimeofday" => denied_settimeofday as *const () as *mut c_void,
        b"SDL_ReadBE16" => read_be16 as *const () as *mut c_void,
        b"SDL_ReadBE32" => read_be32 as *const () as *mut c_void,
        b"SDL_LoadWAV_RW" => load_wav_rw as *const () as *mut c_void,
        b"sysv_signal" => sysv_signal as *const () as *mut c_void,
        b"__stpcpy" => unsafe { libc::dlsym(libc::RTLD_DEFAULT, c"stpcpy".as_ptr()) },
        b"__strcasecmp" => unsafe { libc::dlsym(libc::RTLD_DEFAULT, c"strcasecmp".as_ptr()) },
        _ => unsafe { libc::dlsym(libc::RTLD_DEFAULT, name.as_ptr()) },
    }
}

unsafe extern "C" {
    fn vfwprintf(
        stream: *mut c_void,
        format: *const libc::wchar_t,
        arguments: *mut c_void,
    ) -> c_int;
}
#[inline(never)]
unsafe extern "C" fn forward_vfwprintf(
    stream: *mut c_void,
    format: *const libc::wchar_t,
    arguments: *mut c_void,
) -> c_int {
    unsafe { vfwprintf(stream, format, arguments) }
}
// i386 va_list is the address of the first stack argument. Forward the original
// arguments to vfwprintf without the extra fortify flag; keep the caller's stack.
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __fwprintf_chk(
    _stream: *mut c_void,
    _flag: c_int,
    _format: *const libc::wchar_t,
) -> c_int {
    std::arch::naked_asm!(
        "lea eax, [esp + 16]", "push eax", "push dword ptr [esp + 16]",
        "push dword ptr [esp + 12]", "call {target}", "add esp, 12", "ret",
        target = sym forward_vfwprintf,
    );
}

#[repr(C)]
struct RwOps {
    seek: usize,
    read: unsafe extern "C" fn(*mut RwOps, *mut c_void, c_int, c_int) -> c_int,
    write: usize,
    close: unsafe extern "C" fn(*mut RwOps) -> c_int,
}
unsafe fn read_rw(rw: *mut RwOps, buffer: &mut [u8]) -> bool {
    !rw.is_null()
        && unsafe { ((*rw).read)(rw, buffer.as_mut_ptr().cast(), 1, buffer.len() as c_int) }
            == buffer.len() as c_int
}
unsafe extern "C" fn read_be16(rw: *mut RwOps) -> u16 {
    let mut data = [0; 2];
    if unsafe { read_rw(rw, &mut data) } {
        u16::from_be_bytes(data)
    } else {
        0
    }
}
unsafe extern "C" fn read_be32(rw: *mut RwOps) -> u32 {
    let mut data = [0; 4];
    if unsafe { read_rw(rw, &mut data) } {
        u32::from_be_bytes(data)
    } else {
        0
    }
}
#[repr(C)]
struct AudioSpec {
    frequency: c_int,
    format: u16,
    channels: u8,
    silence: u8,
    samples: u16,
    padding: u16,
    size: u32,
    callback: usize,
    userdata: usize,
}
unsafe extern "C" fn load_wav_rw(
    rw: *mut RwOps,
    free_source: c_int,
    spec: *mut AudioSpec,
    audio: *mut *mut u8,
    length: *mut u32,
) -> *mut AudioSpec {
    let result = (|| -> Option<*mut AudioSpec> {
        if rw.is_null() || spec.is_null() || audio.is_null() || length.is_null() {
            return None;
        }
        unsafe {
            *audio = std::ptr::null_mut();
            *length = 0;
        }
        let mut header = [0; 12];
        if !unsafe { read_rw(rw, &mut header) }
            || &header[..4] != b"RIFF"
            || &header[8..] != b"WAVE"
        {
            return None;
        }
        let total = u32::from_le_bytes(header[4..8].try_into().ok()?) as usize;
        if !(4..=64 * 1024 * 1024).contains(&total) {
            return None;
        }
        let mut body = vec![0; total - 4];
        if !unsafe { read_rw(rw, &mut body) } {
            return None;
        }
        let mut cursor = 0;
        let mut format = None;
        let mut pcm = None;
        while cursor + 8 <= body.len() {
            let size = u32::from_le_bytes(body[cursor + 4..cursor + 8].try_into().ok()?) as usize;
            let chunk = body.get(cursor + 8..cursor + 8 + size)?;
            match &body[cursor..cursor + 4] {
                b"fmt " => format = Some(chunk),
                b"data" => pcm = Some(chunk),
                _ => {}
            }
            cursor = cursor.checked_add(8 + size + (size % 2))?;
        }
        let format = format?;
        let pcm = pcm?;
        if format.len() < 16 {
            return None;
        }
        let encoding = u16::from_le_bytes(format[0..2].try_into().ok()?);
        let channels = u16::from_le_bytes(format[2..4].try_into().ok()?);
        let rate = u32::from_le_bytes(format[4..8].try_into().ok()?);
        let bits = u16::from_le_bytes(format[14..16].try_into().ok()?);
        if encoding != 1
            || !(1..=2).contains(&channels)
            || !matches!(bits, 8 | 16)
            || !(8000..=192000).contains(&rate)
        {
            return None;
        }
        let buffer = unsafe { libc::malloc(pcm.len()) }.cast::<u8>();
        if buffer.is_null() {
            return None;
        }
        unsafe {
            std::ptr::copy_nonoverlapping(pcm.as_ptr(), buffer, pcm.len());
            *audio = buffer;
            *length = pcm.len() as u32;
            *spec = AudioSpec {
                frequency: rate as c_int,
                format: if bits == 8 { 0x0008 } else { 0x8010 },
                channels: channels as u8,
                silence: if bits == 8 { 128 } else { 0 },
                samples: 4096,
                padding: 0,
                size: pcm.len() as u32,
                callback: 0,
                userdata: 0,
            };
        }
        Some(spec)
    })();
    if free_source != 0 && !rw.is_null() {
        unsafe { ((*rw).close)(rw) };
    }
    result.unwrap_or(std::ptr::null_mut())
}
unsafe extern "C" fn denied_settimeofday(_time: *const c_void, _zone: *const c_void) -> c_int {
    unsafe { *libc::__errno_location() = libc::EPERM };
    -1
}

#[cfg(test)]
mod tests {
    use super::*;
    #[repr(C)]
    struct Memory {
        ops: RwOps,
        bytes: Vec<u8>,
        cursor: usize,
    }
    unsafe extern "C" fn read(
        rw: *mut RwOps,
        out: *mut c_void,
        size: c_int,
        count: c_int,
    ) -> c_int {
        let m = unsafe { &mut *rw.cast::<Memory>() };
        let n = (size as usize * count as usize).min(m.bytes.len() - m.cursor);
        unsafe { std::ptr::copy_nonoverlapping(m.bytes[m.cursor..].as_ptr(), out.cast(), n) };
        m.cursor += n;
        if size == 0 { 0 } else { n as c_int / size }
    }
    unsafe extern "C" fn close(_rw: *mut RwOps) -> c_int {
        0
    }
    fn memory(bytes: Vec<u8>) -> Memory {
        Memory {
            ops: RwOps {
                seek: 0,
                read,
                write: 0,
                close,
            },
            bytes,
            cursor: 0,
        }
    }
    #[test]
    fn reads_big_endian_from_guest_rwops() {
        let mut m = memory(vec![0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc]);
        unsafe {
            assert_eq!(read_be16(&mut m.ops), 0x1234);
            assert_eq!(read_be32(&mut m.ops), 0x56789abc);
        }
    }
    #[test]
    fn loads_pcm_wave_without_changing_samples() {
        let mut bytes = b"RIFF".to_vec();
        bytes.extend(40u32.to_le_bytes());
        bytes.extend(b"WAVEfmt ");
        bytes.extend(16u32.to_le_bytes());
        bytes.extend(1u16.to_le_bytes());
        bytes.extend(1u16.to_le_bytes());
        bytes.extend(24000u32.to_le_bytes());
        bytes.extend(48000u32.to_le_bytes());
        bytes.extend(2u16.to_le_bytes());
        bytes.extend(16u16.to_le_bytes());
        bytes.extend(b"data");
        bytes.extend(4u32.to_le_bytes());
        bytes.extend([1, 2, 3, 4]);
        let mut m = memory(bytes);
        let mut spec: AudioSpec = unsafe { std::mem::zeroed() };
        let mut audio = std::ptr::null_mut();
        let mut length = 0;
        unsafe {
            assert!(!load_wav_rw(&mut m.ops, 0, &mut spec, &mut audio, &mut length).is_null());
            assert_eq!(spec.frequency, 24000);
            assert_eq!(spec.format, 0x8010);
            assert_eq!(length, 4);
            assert_eq!(
                std::slice::from_raw_parts(audio, length as usize),
                [1, 2, 3, 4]
            );
            libc::free(audio.cast());
        }
    }
    #[test]
    fn rejects_truncated_wave() {
        let mut m = memory(b"RIFF\xff\xff\xff\xffWAVE".to_vec());
        let mut spec: AudioSpec = unsafe { std::mem::zeroed() };
        let mut audio = std::ptr::null_mut();
        let mut length = 0;
        unsafe {
            assert!(load_wav_rw(&mut m.ops, 0, &mut spec, &mut audio, &mut length).is_null());
        }
    }
}
