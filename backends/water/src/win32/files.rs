//! Guest files for ELF32 programs, backed by the session view: POSIX file
//! descriptors and C `FILE` streams. Streams and descriptors this module did
//! not create (the host CRT's standard streams and fds 0-2) pass through to
//! the host CRT unchanged.
use super::*;
use crate::vfs::{self, File, Options};
use std::{
    collections::{HashMap, HashSet},
    io::SeekFrom,
    sync::Mutex,
};

/// View descriptors start above any the host CRT hands out.
const FIRST_FD: i32 = 0x4000;
static FDS: Mutex<Option<HashMap<i32, File>>> = Mutex::new(None);
static NEXT_FD: Mutex<i32> = Mutex::new(FIRST_FD);

unsafe fn host(name: &str) -> usize {
    crt::host(name)
}
fn fail(e: &std::io::Error) -> i32 {
    unsafe { crt::errno(vfs::errno(e)) }
}

/// Open a view file as a descriptor (Linux `open` flags).
pub fn open_fd(name: &str, flags: i32) -> i32 {
    match File::open(name, Options::linux(flags as u32 as u64)) {
        Ok(file) => {
            let mut next = NEXT_FD.lock().unwrap();
            let fd = *next;
            *next += 1;
            FDS.lock()
                .unwrap()
                .get_or_insert_with(HashMap::new)
                .insert(fd, file);
            fd
        }
        Err(e) => fail(&e),
    }
}
/// Run `f` on a view descriptor; `None` when `fd` is not one.
pub fn with_fd<T>(fd: i32, f: impl FnOnce(&mut File) -> T) -> Option<T> {
    FDS.lock()
        .unwrap()
        .as_mut()
        .and_then(|fds| fds.get_mut(&fd))
        .map(f)
}
pub fn close_fd(fd: i32) -> Option<i32> {
    FDS.lock()
        .unwrap()
        .as_mut()
        .and_then(|fds| fds.remove(&fd))
        .map(|_| 0)
}
pub unsafe fn read_fd(fd: i32, data: *mut u8, length: u32) -> Option<i32> {
    with_fd(fd, |f| {
        let b = std::slice::from_raw_parts_mut(data, length as usize);
        match f.read(b) {
            Ok(n) => n as i32,
            Err(e) => fail(&e),
        }
    })
}
pub unsafe fn write_fd(fd: i32, data: *const u8, length: u32) -> Option<i32> {
    with_fd(fd, |f| {
        let b = std::slice::from_raw_parts(data, length as usize);
        match f.write(b) {
            Ok(n) => n as i32,
            Err(e) => fail(&e),
        }
    })
}
unsafe extern "C" fn lseek(fd: i32, offset: i32, whence: i32) -> i32 {
    let to = match whence {
        0 if offset >= 0 => SeekFrom::Start(offset as u64),
        1 => SeekFrom::Current(offset as i64),
        2 => SeekFrom::End(offset as i64),
        _ => return crt::errno(22),
    };
    if let Some(r) = with_fd(fd, |f| f.seek(to)) {
        return match r {
            Ok(p) if p <= i32::MAX as u64 => p as i32,
            Ok(_) => crt::errno(75), // EOVERFLOW
            Err(e) => fail(&e),
        };
    }
    let f: unsafe extern "C" fn(i32, i32, i32) -> i32 = std::mem::transmute(host("_lseek"));
    f(fd, offset, whence)
}

const BUFFER: usize = 64 * 1024;
/// A C stream over a view file. Reads are buffered; writes go through.
struct Stream {
    file: File,
    buffer: Vec<u8>,
    at: usize,
    unread: Option<u8>,
    eof: bool,
    error: bool,
}
impl Stream {
    /// Logical position: the file position minus buffered, unread bytes.
    fn tell(&self) -> u64 {
        self.file.position() - (self.buffer.len() - self.at) as u64 - u64::from(self.unread.is_some())
    }
    /// Drop read-ahead so the file position is the logical position.
    fn settle(&mut self) -> std::io::Result<()> {
        let at = self.tell();
        self.buffer.clear();
        self.at = 0;
        self.unread = None;
        self.file.seek(SeekFrom::Start(at)).map(|_| ())
    }
    /// Refill the read-ahead buffer; false at end of file or on error.
    fn fill(&mut self) -> bool {
        self.buffer.resize(BUFFER, 0);
        self.at = 0;
        match self.file.read(&mut self.buffer) {
            Ok(0) => {
                self.buffer.clear();
                self.eof = true;
                false
            }
            Ok(n) => {
                self.buffer.truncate(n);
                true
            }
            Err(_) => {
                self.buffer.clear();
                self.error = true;
                false
            }
        }
    }
    fn byte(&mut self) -> Option<u8> {
        if let Some(b) = self.unread.take() {
            return Some(b);
        }
        if self.at == self.buffer.len() && !self.fill() {
            return None;
        }
        self.at += 1;
        Some(self.buffer[self.at - 1])
    }
    fn read(&mut self, out: &mut [u8]) -> usize {
        let mut n = 0;
        if let (Some(b), Some(first)) = (self.unread.take(), out.first_mut()) {
            *first = b;
            n = 1;
        }
        while n < out.len() {
            if self.at < self.buffer.len() {
                let take = (self.buffer.len() - self.at).min(out.len() - n);
                out[n..n + take].copy_from_slice(&self.buffer[self.at..self.at + take]);
                self.at += take;
                n += take;
            } else if out.len() - n >= BUFFER {
                // Large reads go straight to the view.
                match self.file.read(&mut out[n..]) {
                    Ok(0) => {
                        self.eof = true;
                        break;
                    }
                    Ok(got) => n += got,
                    Err(_) => {
                        self.error = true;
                        break;
                    }
                }
            } else if !self.fill() {
                break;
            }
        }
        n
    }
    fn write(&mut self, data: &[u8]) -> usize {
        if self.settle().is_err() {
            self.error = true;
            return 0;
        }
        match self.file.write(data) {
            Ok(n) => n,
            Err(_) => {
                self.error = true;
                0
            }
        }
    }
}

/// Streams this module created, by address.
static STREAMS: Mutex<Option<HashSet<usize>>> = Mutex::new(None);
fn ours(f: usize) -> bool {
    STREAMS.lock().unwrap().as_ref().is_some_and(|s| s.contains(&f))
}
unsafe fn stream<'a>(f: usize) -> &'a mut Stream {
    &mut *(f as *mut Stream)
}

pub unsafe extern "C" fn fopen(name: *const i8, mode: *const i8) -> usize {
    let Some(options) = Options::stdio(text(mode)) else {
        crt::errno(22);
        return 0;
    };
    match File::open(text(name), options) {
        Ok(file) => {
            let f = Box::into_raw(Box::new(Stream {
                file,
                buffer: Vec::new(),
                at: 0,
                unread: None,
                eof: false,
                error: false,
            })) as usize;
            STREAMS.lock().unwrap().get_or_insert_with(HashSet::new).insert(f);
            f
        }
        Err(e) => {
            fail(&e);
            0
        }
    }
}
unsafe extern "C" fn fclose(f: usize) -> i32 {
    let mine = STREAMS.lock().unwrap().as_mut().is_some_and(|s| s.remove(&f));
    if mine {
        drop(Box::from_raw(f as *mut Stream));
        return 0;
    }
    let host: unsafe extern "C" fn(usize) -> i32 = std::mem::transmute(host("fclose"));
    host(f)
}
unsafe extern "C" fn fread(data: *mut u8, size: u32, count: u32, f: usize) -> u32 {
    if !ours(f) {
        let host: unsafe extern "C" fn(*mut u8, u32, u32, usize) -> u32 =
            std::mem::transmute(host("fread"));
        return host(data, size, count, f);
    }
    let Some(total) = size.checked_mul(count).filter(|&t| t > 0) else {
        return 0;
    };
    let out = std::slice::from_raw_parts_mut(data, total as usize);
    stream(f).read(out) as u32 / size
}
unsafe extern "C" fn fwrite(data: *const u8, size: u32, count: u32, f: usize) -> u32 {
    if !ours(f) {
        let host: unsafe extern "C" fn(*const u8, u32, u32, usize) -> u32 =
            std::mem::transmute(host("fwrite"));
        return host(data, size, count, f);
    }
    let Some(total) = size.checked_mul(count).filter(|&t| t > 0) else {
        return 0;
    };
    stream(f).write(std::slice::from_raw_parts(data, total as usize)) as u32 / size
}
unsafe extern "C" fn fseek(f: usize, offset: i32, whence: i32) -> i32 {
    if !ours(f) {
        let host: unsafe extern "C" fn(usize, i32, i32) -> i32 = std::mem::transmute(host("fseek"));
        return host(f, offset, whence);
    }
    let s = stream(f);
    let to = match whence {
        0 if offset >= 0 => SeekFrom::Start(offset as u64),
        1 => SeekFrom::Start(match (s.tell() as i64).checked_add(offset as i64) {
            Some(p) if p >= 0 => p as u64,
            _ => return crt::errno(22),
        }),
        2 => SeekFrom::End(offset as i64),
        _ => return crt::errno(22),
    };
    if s.settle().and_then(|()| s.file.seek(to)).is_err() {
        return crt::errno(22);
    }
    s.eof = false;
    0
}
unsafe extern "C" fn ftell(f: usize) -> i32 {
    if !ours(f) {
        let host: unsafe extern "C" fn(usize) -> i32 = std::mem::transmute(host("ftell"));
        return host(f);
    }
    stream(f).tell().min(i32::MAX as u64) as i32
}
unsafe extern "C" fn rewind(f: usize) {
    if !ours(f) {
        let host: unsafe extern "C" fn(usize) = std::mem::transmute(host("rewind"));
        return host(f);
    }
    fseek(f, 0, 0);
    stream(f).error = false;
}
unsafe extern "C" fn feof(f: usize) -> i32 {
    if !ours(f) {
        let host: unsafe extern "C" fn(usize) -> i32 = std::mem::transmute(host("feof"));
        return host(f);
    }
    i32::from(stream(f).eof)
}
unsafe extern "C" fn ferror(f: usize) -> i32 {
    if !ours(f) {
        let host: unsafe extern "C" fn(usize) -> i32 = std::mem::transmute(host("ferror"));
        return host(f);
    }
    i32::from(stream(f).error)
}
unsafe extern "C" fn clearerr(f: usize) {
    if !ours(f) {
        let host: unsafe extern "C" fn(usize) = std::mem::transmute(host("clearerr"));
        return host(f);
    }
    let s = stream(f);
    s.eof = false;
    s.error = false;
}
unsafe extern "C" fn fflush(f: usize) -> i32 {
    if f != 0 && ours(f) {
        // Writes are not buffered; settling keeps the position consistent.
        return if stream(f).settle().is_ok() { 0 } else { -1 };
    }
    let host: unsafe extern "C" fn(usize) -> i32 = std::mem::transmute(host("fflush"));
    host(f)
}
unsafe extern "C" fn fgetc(f: usize) -> i32 {
    if !ours(f) {
        let host: unsafe extern "C" fn(usize) -> i32 = std::mem::transmute(host("fgetc"));
        return host(f);
    }
    stream(f).byte().map_or(-1, i32::from)
}
unsafe extern "C" fn ungetc(c: i32, f: usize) -> i32 {
    if !ours(f) {
        let host: unsafe extern "C" fn(i32, usize) -> i32 = std::mem::transmute(host("ungetc"));
        return host(c, f);
    }
    let s = stream(f);
    if c == -1 || s.unread.is_some() {
        return -1;
    }
    s.unread = Some(c as u8);
    s.eof = false;
    c & 0xff
}
unsafe extern "C" fn fgets(out: *mut u8, n: i32, f: usize) -> *mut u8 {
    if !ours(f) {
        let host: unsafe extern "C" fn(*mut u8, i32, usize) -> *mut u8 =
            std::mem::transmute(host("fgets"));
        return host(out, n, f);
    }
    if n <= 0 || out.is_null() {
        return ptr::null_mut();
    }
    let s = stream(f);
    let mut i = 0;
    while i + 1 < n as usize {
        let Some(b) = s.byte() else { break };
        *out.add(i) = b;
        i += 1;
        if b == b'\n' {
            break;
        }
    }
    if i == 0 {
        return ptr::null_mut();
    }
    *out.add(i) = 0;
    out
}
unsafe extern "C" fn fputc(c: i32, f: usize) -> i32 {
    if !ours(f) {
        let host: unsafe extern "C" fn(i32, usize) -> i32 = std::mem::transmute(host("fputc"));
        return host(c, f);
    }
    if stream(f).write(&[c as u8]) == 1 { c & 0xff } else { -1 }
}
unsafe extern "C" fn fputs(text: *const i8, f: usize) -> i32 {
    if !ours(f) {
        let host: unsafe extern "C" fn(*const i8, usize) -> i32 = std::mem::transmute(host("fputs"));
        return host(text, f);
    }
    let bytes = CStr::from_ptr(text).to_bytes();
    if stream(f).write(bytes) == bytes.len() { 0 } else { -1 }
}
unsafe extern "C" fn setvbuf(f: usize, buffer: *mut u8, mode: i32, size: u32) -> i32 {
    if !ours(f) {
        let host: unsafe extern "C" fn(usize, *mut u8, i32, u32) -> i32 =
            std::mem::transmute(host("setvbuf"));
        return host(f, buffer, mode, size);
    }
    0
}
/// `vfprintf` with an i386 `va_list` (a pointer to the stacked arguments).
unsafe extern "C" fn vfprintf(f: usize, format: *const i8, args: usize) -> i32 {
    if !ours(f) {
        let host: unsafe extern "C" fn(usize, *const i8, usize) -> i32 =
            std::mem::transmute(host("vfprintf"));
        return host(f, format, args);
    }
    let count: unsafe extern "C" fn(*const i8, usize) -> i32 =
        std::mem::transmute(host("_vscprintf"));
    let length = count(format, args);
    if length < 0 {
        return -1;
    }
    let mut text = vec![0u8; length as usize + 1];
    let print: unsafe extern "C" fn(*mut u8, u32, *const i8, usize) -> i32 =
        std::mem::transmute(host("_vsnprintf"));
    let written = print(text.as_mut_ptr(), text.len() as u32, format, args);
    if written < 0 {
        return -1;
    }
    let text = &text[..written as usize];
    if stream(f).write(text) == text.len() { written } else { -1 }
}
/// `fprintf(FILE*, format, ...)`: forward the stacked arguments as a va_list.
#[unsafe(naked)]
unsafe extern "C" fn fprintf() {
    core::arch::naked_asm!(
        "lea eax, [esp+12]",
        "push eax",
        "push dword ptr [esp+12]",
        "push dword ptr [esp+12]",
        "call {vfprintf}",
        "add esp, 12",
        "ret",
        vfprintf = sym vfprintf,
    );
}

unsafe extern "C" fn rename(from: *const i8, to: *const i8) -> i32 {
    match vfs::rename(text(from), text(to)) {
        Ok(()) => 0,
        Err(e) => fail(&e),
    }
}
unsafe extern "C" fn remove(name: *const i8) -> i32 {
    match vfs::remove(text(name)) {
        Ok(()) => 0,
        Err(e) => fail(&e),
    }
}

pub unsafe fn resolve(name: &str) -> Option<usize> {
    Some(match name {
        "fopen" | "fopen64" => fopen as *const () as usize,
        "fclose" => fclose as *const () as usize,
        "fread" => fread as *const () as usize,
        "fwrite" => fwrite as *const () as usize,
        "fseek" => fseek as *const () as usize,
        "ftell" => ftell as *const () as usize,
        "rewind" => rewind as *const () as usize,
        "feof" => feof as *const () as usize,
        "ferror" => ferror as *const () as usize,
        "clearerr" => clearerr as *const () as usize,
        "fflush" => fflush as *const () as usize,
        "fgetc" | "getc" | "_IO_getc" => fgetc as *const () as usize,
        "ungetc" => ungetc as *const () as usize,
        "fgets" => fgets as *const () as usize,
        "fputc" | "putc" | "_IO_putc" => fputc as *const () as usize,
        "fputs" => fputs as *const () as usize,
        "setvbuf" => setvbuf as *const () as usize,
        "vfprintf" => vfprintf as *const () as usize,
        "fprintf" => fprintf as *const () as usize,
        "lseek" => lseek as *const () as usize,
        "rename" => rename as *const () as usize,
        "remove" | "unlink" => remove as *const () as usize,
        _ => return None,
    })
}
