use super::*;
use std::{collections::VecDeque, sync::Mutex};
use windows_sys::Win32::{Media::Audio::*, System::Threading::Sleep};
const DSP: i32 = 0x7000;
const MIXER: i32 = 0x7001;
struct Buffer {
    header: Box<WAVEHDR>,
    _data: Vec<u8>,
}
struct Device {
    handle: usize,
    rate: u32,
    channels: u16,
    fragment: i32,
    buffers: VecDeque<Buffer>,
    bytes: u64,
    nonzero: bool,
}
// Buffers and the wave handle are only accessed under DEVICE's lock.
unsafe impl Send for Device {}
static DEVICE: Mutex<Device> = Mutex::new(Device {
    handle: 0,
    rate: 24000,
    channels: 2,
    fragment: 1024,
    buffers: VecDeque::new(),
    bytes: 0,
    nonzero: false,
});
unsafe fn host(name: &str) -> usize {
    proc(LoadLibraryA(c"msvcrt.dll".as_ptr().cast()), name)
}
unsafe extern "C" fn open(name: *const i8, flags: i32, mode: i32) -> i32 {
    let name = text(name);
    if name == "/dev/dsp" {
        return DSP;
    }
    if name == "/dev/mixer" || name == "/dev/mixer0" {
        return MIXER;
    }
    if name.starts_with("/dev/") {
        return crt::errno(2);
    }
    let write = flags & 3 != 0 || flags & (0x40 | 0x200 | 0x400) != 0;
    let path = match path(name, write) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[water] open: {e}");
            return crt::errno(13);
        }
    };
    let native_flags = (flags & 3)
        | 0x8000
        | if flags & 0x40 != 0 { 0x100 } else { 0 }
        | if flags & 0x200 != 0 { 0x200 } else { 0 }
        | if flags & 0x400 != 0 { 8 } else { 0 }
        | if flags & 0x80 != 0 { 0x400 } else { 0 };
    let f: unsafe extern "C" fn(*const i8, i32, i32) -> i32 = std::mem::transmute(host("_open"));
    let fd = f(cpath(&path).as_ptr(), native_flags, mode);
    eprintln!("[water] open {name} -> {fd}");
    fd
}
unsafe extern "C" fn ioctl(fd: i32, request: u32, arg: *mut i32) -> i32 {
    if fd == MIXER {
        return 0;
    }
    if fd != DSP {
        return crt::errno(25);
    }
    if request & 0xff00 == 0x4d00 {
        return 0;
    } // Legacy cabinet mixer ioctls on the DSP fd.
    let mut d = DEVICE.lock().unwrap();
    if request == 0x5000 || request == 0x5001 {
        if d.handle != 0 {
            waveOutReset(d.handle as HWAVEOUT);
        }
        return 0;
    }
    if arg.is_null() {
        return crt::errno(14);
    }
    match request {
        0x8004500b => *arg = 0x10,
        0xc0045005 => {
            if *arg != 0x10 {
                return crt::errno(22);
            }
        }
        0xc0045002 => {
            if !(8000..=96000).contains(&*arg) {
                return crt::errno(22);
            }
            d.rate = *arg as u32;
        }
        0xc0045003 => {
            d.channels = if *arg == 0 { 1 } else { 2 };
        }
        0xc0045006 => {
            if *arg != 1 && *arg != 2 {
                return crt::errno(22);
            }
            d.channels = *arg as u16;
        }
        0xc004500a => {
            let power = (*arg & 65535).clamp(8, 16);
            d.fragment = 1 << power;
        }
        0xc0045004 => *arg = d.fragment,
        0x8010500c => {
            *arg = 8;
            *arg.add(1) = 8;
            *arg.add(2) = d.fragment;
            *arg.add(3) = 8 * d.fragment;
        }
        0x8004500f | 0x80045017 => *arg = 0,
        _ => {
            eprintln!("[water] unsupported OSS ioctl {request:#x}");
            return crt::errno(25);
        }
    }
    0
}
unsafe fn reap(d: &mut Device, wait: bool) {
    while let Some(front) = d.buffers.front() {
        while std::ptr::read_volatile(ptr::addr_of!(front.header.dwFlags)) & WHDR_DONE == 0 {
            if !wait {
                return;
            }
            Sleep(1);
        }
        let mut buffer = d.buffers.pop_front().unwrap();
        waveOutUnprepareHeader(
            d.handle as HWAVEOUT,
            &mut *buffer.header,
            std::mem::size_of::<WAVEHDR>() as u32,
        );
        if wait {
            return;
        }
    }
}
unsafe extern "C" fn write(fd: i32, data: *const u8, length: u32) -> i32 {
    if fd != DSP {
        let f: unsafe extern "C" fn(i32, *const u8, u32) -> i32 =
            std::mem::transmute(host("_write"));
        return f(fd, data, length);
    }
    if length == 0 {
        return 0;
    }
    if data.is_null() || length > 16 * 1024 * 1024 {
        return crt::errno(22);
    }
    let mut d = DEVICE.lock().unwrap();
    if std::env::var("ARCADE_AUDIO").is_ok_and(|s| s == "0") || std::env::var_os("ARCADE_NATIVE_PROBE").is_some() {
        let millis = length as u64 * 1000 / (d.rate as u64 * d.channels as u64 * 2);
        Sleep(millis as u32);
        return length as i32;
    }
    if d.handle == 0 {
        let mut handle: HWAVEOUT = ptr::null_mut();
        let format = WAVEFORMATEX {
            wFormatTag: 1,
            nChannels: d.channels,
            nSamplesPerSec: d.rate,
            nAvgBytesPerSec: d.rate * d.channels as u32 * 2,
            nBlockAlign: d.channels * 2,
            wBitsPerSample: 16,
            cbSize: 0,
        };
        let result = waveOutOpen(&mut handle, WAVE_MAPPER, &format, 0, 0, 0);
        if result != 0 {
            eprintln!("[water] waveOutOpen failed {result}");
            return crt::errno(5);
        }
        d.handle = handle as usize;
        eprintln!(
            "[water] Windows PCM playback: {} Hz, {} channels",
            d.rate, d.channels
        );
    }
    reap(&mut d, false);
    if d.buffers.len() >= 4 {
        reap(&mut d, true);
    }
    let samples = std::slice::from_raw_parts(data, length as usize).to_vec();
    let mut header: Box<WAVEHDR> = Box::new(std::mem::zeroed());
    header.lpData = samples.as_ptr() as *mut u8;
    header.dwBufferLength = length;
    if waveOutPrepareHeader(
        d.handle as HWAVEOUT,
        &mut *header,
        std::mem::size_of::<WAVEHDR>() as u32,
    ) != 0
    {
        return crt::errno(5);
    }
    if waveOutWrite(
        d.handle as HWAVEOUT,
        &mut *header,
        std::mem::size_of::<WAVEHDR>() as u32,
    ) != 0
    {
        waveOutUnprepareHeader(
            d.handle as HWAVEOUT,
            &mut *header,
            std::mem::size_of::<WAVEHDR>() as u32,
        );
        return crt::errno(5);
    }
    if !d.nonzero && samples.iter().any(|&v| v != 0) {
        d.nonzero = true;
        eprintln!("[water] non-silent PCM submitted to Windows audio");
    }
    d.bytes += length as u64;
    d.buffers.push_back(Buffer {
        header,
        _data: samples,
    });
    length as i32
}
unsafe extern "C" fn close(fd: i32) -> i32 {
    if fd == MIXER {
        return 0;
    }
    if fd == DSP {
        let mut d = DEVICE.lock().unwrap();
        if d.handle != 0 {
            waveOutReset(d.handle as HWAVEOUT);
            reap(&mut d, false);
            waveOutClose(d.handle as HWAVEOUT);
            d.handle = 0;
        }
        eprintln!("[water] audio closed: {} bytes", d.bytes);
        return 0;
    }
    let f: unsafe extern "C" fn(i32) -> i32 = std::mem::transmute(host("_close"));
    f(fd)
}
unsafe extern "C" fn read(fd: i32, data: *mut u8, length: u32) -> i32 {
    if fd == DSP || fd == MIXER {
        return crt::errno(9);
    }
    let f: unsafe extern "C" fn(i32, *mut u8, u32) -> i32 = std::mem::transmute(host("_read"));
    f(fd, data, length)
}
pub unsafe fn resolve(name: &str) -> Option<usize> {
    Some(match name {
        "open" => open as *const () as usize,
        "ioctl" => ioctl as *const () as usize,
        "write" => write as *const () as usize,
        "read" => read as *const () as usize,
        "close" => close as *const () as usize,
        _ => return None,
    })
}
