//! Win32 file API over the session view. Paths under the staged root map to
//! view paths; files opened there get a real placeholder handle (to the NUL
//! device) that indexes the view handle, so any API this shim does not cover
//! sees a harmless empty device instead of an unrelated object.
#![allow(non_snake_case, unsafe_op_in_unsafe_fn, clippy::missing_safety_doc)]
mod hook;

use crate::ini;
use banana_wire::{Client, Meta, errno, open as flags};
use std::{
    collections::HashMap,
    ffi::c_void,
    io,
    ptr,
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicUsize, Ordering::Relaxed},
    },
};
use windows_sys::Win32::{
    Foundation::*,
    Globalization::{CP_ACP, CompareStringOrdinal, MultiByteToWideChar, WideCharToMultiByte},
    Storage::FileSystem::*,
    System::{
        Console::{GetStdHandle, STD_ERROR_HANDLE},
        IO::OVERLAPPED,
        LibraryLoader::{DisableThreadLibraryCalls, GetModuleHandleW, GetProcAddress},
        Memory::{FILE_MAP_WRITE, MapViewOfFile, UnmapViewOfFile},
        SystemInformation::GetSystemTimeAsFileTime,
        SystemServices::DLL_PROCESS_ATTACH,
        Threading::SetEvent,
    },
};

type H = usize;
const INVALID: H = usize::MAX;

static VIEW: OnceLock<Client> = OnceLock::new();
/// The staged root's full path, without a trailing separator.
static ROOT: OnceLock<Vec<u16>> = OnceLock::new();
static TIME: OnceLock<FILETIME> = OnceLock::new();
/// `BANANA_VIEW_TRACE=1` logs every path the shim serves; `2` also logs
/// each read, write, seek and size query.
static TRACE: OnceLock<u8> = OnceLock::new();
fn trace(what: &str, path: &str) {
    if *TRACE.get().unwrap_or(&0) >= 1 {
        log(&format!("[banana-view] {what} {path}\n"));
    }
}
fn trace_io(h: H, what: std::fmt::Arguments) {
    if *TRACE.get().unwrap_or(&0) >= 2 {
        let path = with_file(h, |f| f.path.clone()).unwrap_or_default();
        log(&format!("[banana-view]   {path}: {what}\n"));
    }
}

struct Open {
    remote: u64,
    path: String,
    position: u64,
    append: bool,
    read: bool,
    write: bool,
    delete_on_close: bool,
    /// Requested access (`WANT_*`) and share mode, for Windows sharing rules.
    wants: u32,
    share: u32,
}
const WANT_READ: u32 = 1;
const WANT_WRITE: u32 = 2;
const WANT_DELETE: u32 = 4;
fn wants(access: u32, attributes: u32) -> u32 {
    let mut w = 0;
    if access & (GENERIC_READ | GENERIC_ALL | GENERIC_EXECUTE | FILE_READ_DATA) != 0 {
        w |= WANT_READ;
    }
    if access & (GENERIC_WRITE | GENERIC_ALL | FILE_WRITE_DATA | FILE_APPEND_DATA) != 0 {
        w |= WANT_WRITE;
    }
    if access & (DELETE | GENERIC_ALL) != 0 || attributes & FILE_FLAG_DELETE_ON_CLOSE != 0 {
        w |= WANT_DELETE;
    }
    w
}
/// Whether opening `path` with `wants`/`share` conflicts with an open handle.
fn sharing_conflict(path: &str, wants: u32, share: u32) -> bool {
    let files = FILES.lock().unwrap();
    files.as_ref().is_some_and(|files| {
        files.values().any(|o| {
            o.path.eq_ignore_ascii_case(path)
                && (wants & WANT_READ != 0 && o.share & FILE_SHARE_READ == 0
                    || wants & WANT_WRITE != 0 && o.share & FILE_SHARE_WRITE == 0
                    || wants & WANT_DELETE != 0 && o.share & FILE_SHARE_DELETE == 0
                    || o.wants & WANT_READ != 0 && share & FILE_SHARE_READ == 0
                    || o.wants & WANT_WRITE != 0 && share & FILE_SHARE_WRITE == 0
                    || o.wants & WANT_DELETE != 0 && share & FILE_SHARE_DELETE == 0)
        })
    })
}
/// DeleteFile/MoveFile semantics: delete access, everything shared.
fn in_use(path: &str) -> bool {
    sharing_conflict(path, WANT_DELETE, FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
}
static FILES: Mutex<Option<HashMap<H, Open>>> = Mutex::new(None);
struct Find {
    entries: Vec<(String, Meta)>,
    next: usize,
}
static FINDS: Mutex<Option<HashMap<H, Find>>> = Mutex::new(None);

// ---- originals -------------------------------------------------------------

/// A callable original (the trampoline the hook saved).
macro_rules! original {
    ($name:ident, fn($($a:ident: $t:ty),*) -> $r:ty) => {
        #[allow(dead_code)]
        pub(super) unsafe fn $name($($a: $t),*) -> $r {
            let f: unsafe extern "system" fn($($t),*) -> $r =
                std::mem::transmute(slot::$name.load(Relaxed));
            f($($a),*)
        }
    };
}
#[allow(non_upper_case_globals)]
mod slot {
    use std::sync::atomic::AtomicUsize;
    macro_rules! slots { ($($n:ident),*) => { $(pub static $n: AtomicUsize = AtomicUsize::new(0);)* } }
    slots!(
        CreateFileA, CreateFileW, CreateFile2, ReadFile, WriteFile, SetFilePointer,
        SetFilePointerEx, GetFileSize, GetFileSizeEx, GetFileType, CloseHandle,
        FlushFileBuffers, SetEndOfFile, GetFileInformationByHandle,
        GetFileInformationByHandleEx, GetFinalPathNameByHandleW, GetFileTime,
        GetFileAttributesA, GetFileAttributesW,
        GetFileAttributesExA, GetFileAttributesExW, FindFirstFileA, FindFirstFileW,
        FindFirstFileExA, FindFirstFileExW, FindNextFileA, FindNextFileW, FindClose,
        DeleteFileA, DeleteFileW, CreateDirectoryA, CreateDirectoryW, RemoveDirectoryA,
        RemoveDirectoryW, MoveFileA, MoveFileW, MoveFileExA, MoveFileExW,
        CreateFileMappingA, CreateFileMappingW, GetPrivateProfileStringA,
        GetPrivateProfileStringW, GetPrivateProfileIntA, GetPrivateProfileIntW,
        WritePrivateProfileStringA, WritePrivateProfileStringW
    );
}
mod o {
    use super::*;
    original!(CreateFileA, fn(a: *const u8, b: u32, c: u32, d: usize, e: u32, f: u32, g: H) -> H);
    original!(CreateFileW, fn(a: *const u16, b: u32, c: u32, d: usize, e: u32, f: u32, g: H) -> H);
    original!(CreateFile2, fn(a: *const u16, b: u32, c: u32, d: u32, e: *const CREATEFILE2_EXTENDED_PARAMETERS) -> H);
    original!(GetFileInformationByHandleEx, fn(h: H, c: i32, o: *mut c_void, n: u32) -> i32);
    original!(GetFinalPathNameByHandleW, fn(h: H, o: *mut u16, n: u32, f: u32) -> u32);
    original!(ReadFile, fn(h: H, b: *mut u8, n: u32, r: *mut u32, o: *mut OVERLAPPED) -> i32);
    original!(WriteFile, fn(h: H, b: *const u8, n: u32, r: *mut u32, o: *mut OVERLAPPED) -> i32);
    original!(SetFilePointer, fn(h: H, d: i32, hi: *mut i32, m: u32) -> u32);
    original!(SetFilePointerEx, fn(h: H, d: i64, p: *mut i64, m: u32) -> i32);
    original!(GetFileSize, fn(h: H, hi: *mut u32) -> u32);
    original!(GetFileSizeEx, fn(h: H, s: *mut i64) -> i32);
    original!(GetFileType, fn(h: H) -> u32);
    original!(CloseHandle, fn(h: H) -> i32);
    original!(FlushFileBuffers, fn(h: H) -> i32);
    original!(SetEndOfFile, fn(h: H) -> i32);
    original!(GetFileInformationByHandle, fn(h: H, i: *mut BY_HANDLE_FILE_INFORMATION) -> i32);
    original!(GetFileTime, fn(h: H, c: *mut FILETIME, a: *mut FILETIME, w: *mut FILETIME) -> i32);
    original!(GetFileAttributesA, fn(p: *const u8) -> u32);
    original!(GetFileAttributesW, fn(p: *const u16) -> u32);
    original!(GetFileAttributesExA, fn(p: *const u8, l: u32, d: *mut c_void) -> i32);
    original!(GetFileAttributesExW, fn(p: *const u16, l: u32, d: *mut c_void) -> i32);
    original!(FindFirstFileA, fn(p: *const u8, d: *mut WIN32_FIND_DATAA) -> H);
    original!(FindFirstFileW, fn(p: *const u16, d: *mut WIN32_FIND_DATAW) -> H);
    original!(FindFirstFileExA, fn(p: *const u8, l: u32, d: *mut c_void, s: u32, f: usize, x: u32) -> H);
    original!(FindFirstFileExW, fn(p: *const u16, l: u32, d: *mut c_void, s: u32, f: usize, x: u32) -> H);
    original!(FindNextFileA, fn(h: H, d: *mut WIN32_FIND_DATAA) -> i32);
    original!(FindNextFileW, fn(h: H, d: *mut WIN32_FIND_DATAW) -> i32);
    original!(FindClose, fn(h: H) -> i32);
    original!(DeleteFileA, fn(p: *const u8) -> i32);
    original!(DeleteFileW, fn(p: *const u16) -> i32);
    original!(CreateDirectoryA, fn(p: *const u8, s: usize) -> i32);
    original!(CreateDirectoryW, fn(p: *const u16, s: usize) -> i32);
    original!(RemoveDirectoryA, fn(p: *const u8) -> i32);
    original!(RemoveDirectoryW, fn(p: *const u16) -> i32);
    original!(MoveFileA, fn(a: *const u8, b: *const u8) -> i32);
    original!(MoveFileW, fn(a: *const u16, b: *const u16) -> i32);
    original!(MoveFileExA, fn(a: *const u8, b: *const u8, f: u32) -> i32);
    original!(MoveFileExW, fn(a: *const u16, b: *const u16, f: u32) -> i32);
    original!(CreateFileMappingA, fn(h: H, s: usize, p: u32, hi: u32, lo: u32, n: *const u8) -> H);
    original!(CreateFileMappingW, fn(h: H, s: usize, p: u32, hi: u32, lo: u32, n: *const u16) -> H);
    original!(GetPrivateProfileStringA, fn(a: *const u8, k: *const u8, d: *const u8, o: *mut u8, n: u32, f: *const u8) -> u32);
    original!(GetPrivateProfileStringW, fn(a: *const u16, k: *const u16, d: *const u16, o: *mut u16, n: u32, f: *const u16) -> u32);
    original!(GetPrivateProfileIntA, fn(a: *const u8, k: *const u8, d: i32, f: *const u8) -> u32);
    original!(GetPrivateProfileIntW, fn(a: *const u16, k: *const u16, d: i32, f: *const u16) -> u32);
    original!(WritePrivateProfileStringA, fn(a: *const u8, k: *const u8, v: *const u8, f: *const u8) -> i32);
    original!(WritePrivateProfileStringW, fn(a: *const u16, k: *const u16, v: *const u16, f: *const u16) -> i32);
}

// ---- entry -------------------------------------------------------------------

#[unsafe(no_mangle)]
pub extern "system" fn DllMain(module: *mut c_void, reason: u32, _: *mut c_void) -> i32 {
    if reason != DLL_PROCESS_ATTACH {
        return TRUE;
    }
    unsafe { DisableThreadLibraryCalls(module) };
    match unsafe { attach() } {
        Ok(()) => TRUE,
        Err(e) => {
            log(&format!("[banana-view] cannot serve the session view: {e}\n"));
            FALSE
        }
    }
}
/// The import the Native backend adds to staged executables.
#[unsafe(no_mangle)]
pub extern "C" fn banana_view_attach() {}

unsafe fn attach() -> Result<(), String> {
    let Ok(spec) = std::env::var("BANANA_VIEW") else {
        // Diagnostics: with BANANA_VIEW_TRACE=3 and no view, hook everything,
        // serve nothing and log every open (to study how a game reads files).
        if std::env::var("BANANA_VIEW_TRACE").is_ok_and(|v| v == "3") {
            TRACE.set(3).ok();
            return install_hooks();
        }
        return Ok(()); // Not a Banana Pepper Windows session (e.g. Wine).
    };
    let root = std::env::var("BANANA_VIEW_ROOT").map_err(|_| "BANANA_VIEW_ROOT is unset")?;
    let mut root = full_path(&root.encode_utf16().collect::<Vec<_>>()).ok_or("bad root")?;
    while root.last().is_some_and(|&c| c == b'\\' as u16) {
        root.pop();
    }
    ROOT.set(root).ok();
    let mut now = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
    GetSystemTimeAsFileTime(&mut now);
    TIME.set(now).ok();
    TRACE
        .set(std::env::var("BANANA_VIEW_TRACE").map_or(0, |v| v.parse().unwrap_or(1)))
        .ok();
    let (r, w) = spec
        .split_once(',')
        .and_then(|(r, w)| Some((r.parse::<usize>().ok()?, w.parse::<usize>().ok()?)))
        .ok_or("BANANA_VIEW expects READ,WRITE")?;
    let client = Client::new(Pipe(r), Pipe(w)).map_err(|e| e.to_string())?;
    VIEW.set(client).ok();
    install_hooks()
}

unsafe fn install_hooks() -> Result<(), String> {
    let base = GetModuleHandleW(wide("kernelbase.dll").as_ptr());
    let k32 = GetModuleHandleW(wide("kernel32.dll").as_ptr());
    let mut count = 0;
    for (name, detour, slot) in detours() {
        let symbol = format!("{name}\0");
        let target = [base, k32]
            .into_iter()
            .filter(|m| !m.is_null())
            .find_map(|m| GetProcAddress(m, symbol.as_ptr()))
            .map(|f| f as usize);
        let Some(target) = target else { continue };
        let original = hook::install(target, detour).map_err(|e| format!("{name}: {e}"))?;
        slot.store(original, Relaxed);
        count += 1;
    }
    log(&format!(
        "[banana-view] {count} file APIs served from the view under {}\n",
        ROOT.get().map_or("(observing only)".into(), |r| String::from_utf16_lossy(r))
    ));
    Ok(())
}

/// A pipe end that bypasses the hooks (the view client must never re-enter).
struct Pipe(usize);
impl io::Read for Pipe {
    fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
        let mut n = 0u32;
        let ok = unsafe {
            if slot::ReadFile.load(Relaxed) != 0 {
                o::ReadFile(self.0, b.as_mut_ptr(), b.len() as u32, &mut n, ptr::null_mut())
            } else {
                windows_sys::Win32::Storage::FileSystem::ReadFile(
                    self.0 as HANDLE,
                    b.as_mut_ptr(),
                    b.len() as u32,
                    &mut n,
                    ptr::null_mut(),
                )
            }
        };
        if ok == 0 {
            let e = io::Error::last_os_error();
            return if e.raw_os_error() == Some(ERROR_BROKEN_PIPE as i32) {
                Ok(0)
            } else {
                Err(e)
            };
        }
        Ok(n as usize)
    }
}
impl io::Write for Pipe {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        let mut n = 0u32;
        let ok = unsafe {
            if slot::WriteFile.load(Relaxed) != 0 {
                o::WriteFile(self.0, b.as_ptr(), b.len() as u32, &mut n, ptr::null_mut())
            } else {
                windows_sys::Win32::Storage::FileSystem::WriteFile(
                    self.0 as HANDLE,
                    b.as_ptr(),
                    b.len() as u32,
                    &mut n,
                    ptr::null_mut(),
                )
            }
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(n as usize)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
fn log(text: &str) {
    let mut n = 0u32;
    unsafe {
        let err = GetStdHandle(STD_ERROR_HANDLE) as usize;
        if slot::WriteFile.load(Relaxed) != 0 {
            o::WriteFile(err, text.as_ptr(), text.len() as u32, &mut n, ptr::null_mut());
        } else {
            windows_sys::Win32::Storage::FileSystem::WriteFile(
                err as HANDLE,
                text.as_ptr(),
                text.len() as u32,
                &mut n,
                ptr::null_mut(),
            );
        }
    }
}

// ---- paths -------------------------------------------------------------------

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
unsafe fn wstr(p: *const u16) -> Option<Vec<u16>> {
    if p.is_null() {
        return None;
    }
    let mut n = 0;
    while *p.add(n) != 0 {
        n += 1;
    }
    Some(std::slice::from_raw_parts(p, n).to_vec())
}
unsafe fn astr(p: *const u8) -> Option<Vec<u8>> {
    if p.is_null() {
        return None;
    }
    let mut n = 0;
    while *p.add(n) != 0 {
        n += 1;
    }
    Some(std::slice::from_raw_parts(p, n).to_vec())
}
fn a2w(a: &[u8]) -> Vec<u16> {
    if a.is_empty() {
        return Vec::new();
    }
    unsafe {
        let n = MultiByteToWideChar(CP_ACP, 0, a.as_ptr(), a.len() as i32, ptr::null_mut(), 0);
        let mut w = vec![0u16; n.max(0) as usize];
        MultiByteToWideChar(CP_ACP, 0, a.as_ptr(), a.len() as i32, w.as_mut_ptr(), n);
        w
    }
}
fn w2a(w: &[u16]) -> Vec<u8> {
    if w.is_empty() {
        return Vec::new();
    }
    unsafe {
        let n = WideCharToMultiByte(
            CP_ACP, 0, w.as_ptr(), w.len() as i32, ptr::null_mut(), 0, ptr::null(), ptr::null_mut(),
        );
        let mut a = vec![0u8; n.max(0) as usize];
        WideCharToMultiByte(
            CP_ACP, 0, w.as_ptr(), w.len() as i32, a.as_mut_ptr(), n, ptr::null(), ptr::null_mut(),
        );
        a
    }
}
fn full_path(path: &[u16]) -> Option<Vec<u16>> {
    let input: Vec<u16> = path.iter().copied().chain(Some(0)).collect();
    let mut buffer = vec![0u16; 1024];
    loop {
        let n = unsafe {
            GetFullPathNameW(input.as_ptr(), buffer.len() as u32, buffer.as_mut_ptr(), ptr::null_mut())
        } as usize;
        if n == 0 {
            return None;
        }
        if n < buffer.len() {
            buffer.truncate(n);
            return Some(buffer);
        }
        buffer.resize(n + 1, 0);
    }
}
/// The view path for a host path under the staged root.
fn view_of(path: &[u16]) -> Option<String> {
    let root = ROOT.get()?;
    let mut full = full_path(path)?;
    for prefix in [r"\\?\", r"\??\"] {
        let p: Vec<u16> = prefix.encode_utf16().collect();
        if full.starts_with(&p) {
            full.drain(..p.len());
        }
    }
    if full.len() < root.len()
        || unsafe {
            CompareStringOrdinal(
                full.as_ptr(),
                root.len() as i32,
                root.as_ptr(),
                root.len() as i32,
                TRUE,
            )
        } != 2
    {
        return None;
    }
    let rest = &full[root.len()..];
    if !rest.is_empty() && rest[0] != b'\\' as u16 && rest[0] != b'/' as u16 {
        return None;
    }
    let text = String::from_utf16(rest).ok()?;
    Some(
        text.split(['\\', '/'])
            .filter(|p| !p.is_empty())
            .collect::<Vec<_>>()
            .join("/"),
    )
}
unsafe fn view_w(p: *const u16) -> Option<String> {
    if VIEW.get().is_none() {
        return None;
    }
    view_of(&wstr(p)?)
}
unsafe fn view_a(p: *const u8) -> Option<String> {
    if VIEW.get().is_none() {
        return None;
    }
    view_of(&a2w(&astr(p)?))
}
fn view() -> &'static Client {
    VIEW.get().expect("view connected")
}
fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |p| p.0)
}

// ---- errors ------------------------------------------------------------------

fn fail<T>(code: u32, value: T) -> T {
    unsafe { SetLastError(code) };
    value
}
/// Win32 error for a view error on `path` (missing parents are distinct).
fn win32(e: &io::Error, path: &str) -> u32 {
    match banana_wire::errno_of(e) {
        errno::ENOENT => {
            if parent(path).is_empty() || view().stat(parent(path)).is_ok_and(|m| m.directory) {
                ERROR_FILE_NOT_FOUND
            } else {
                ERROR_PATH_NOT_FOUND
            }
        }
        errno::EEXIST => ERROR_FILE_EXISTS,
        errno::EACCES | errno::EISDIR => ERROR_ACCESS_DENIED,
        errno::ENOTDIR => ERROR_PATH_NOT_FOUND,
        errno::ENOTEMPTY => ERROR_DIR_NOT_EMPTY,
        errno::EBADF => ERROR_INVALID_HANDLE,
        errno::EINVAL => ERROR_INVALID_PARAMETER,
        errno::EMFILE => ERROR_TOO_MANY_OPEN_FILES,
        _ => ERROR_GEN_FAILURE,
    }
}

// ---- handles -----------------------------------------------------------------

/// A real handle that stands for a view file or search.
unsafe fn placeholder() -> H {
    o::CreateFileW(
        wide("NUL").as_ptr(),
        GENERIC_READ | GENERIC_WRITE,
        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
        0,
        OPEN_EXISTING,
        0,
        0,
    )
}
fn with_file<T>(h: H, f: impl FnOnce(&mut Open) -> T) -> Option<T> {
    FILES.lock().unwrap().as_mut()?.get_mut(&h).map(f)
}
fn ours(h: H) -> bool {
    h != 0 && h != INVALID && with_file(h, |_| ()).is_some()
}

unsafe fn create(
    path: String,
    access: u32,
    share: u32,
    disposition: u32,
    attributes: u32,
) -> Result<H, u32> {
    trace(&format!("open {access:#x}/{disposition}"), &path);
    let existing = match view().stat(&path) {
        Ok(m) => Some(m),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(win32(&e, &path)),
    };
    if existing.is_some_and(|m| m.directory) {
        return Err(ERROR_ACCESS_DENIED);
    }
    let write = access & (GENERIC_WRITE | GENERIC_ALL | FILE_WRITE_DATA | FILE_APPEND_DATA) != 0;
    let mut f = flags::READ;
    if write {
        f |= flags::WRITE;
    }
    match disposition {
        CREATE_NEW => f |= flags::CREATE | flags::EXCLUSIVE,
        CREATE_ALWAYS => f |= flags::CREATE | flags::TRUNCATE | flags::WRITE,
        OPEN_EXISTING => {}
        OPEN_ALWAYS => f |= flags::CREATE,
        TRUNCATE_EXISTING => f |= flags::TRUNCATE | flags::WRITE,
        _ => return Err(ERROR_INVALID_PARAMETER),
    }
    if existing.is_none() && f & flags::CREATE == 0 {
        return Err(win32(&io::ErrorKind::NotFound.into(), &path));
    }
    let wanted = wants(access, attributes);
    if sharing_conflict(&path, wanted, share) {
        return Err(ERROR_SHARING_VIOLATION);
    }
    let (remote, _) = view().open(&path, f).map_err(|e| win32(&e, &path))?;
    let h = placeholder();
    if h == INVALID {
        let _ = view().close(remote);
        return Err(ERROR_TOO_MANY_OPEN_FILES);
    }
    FILES.lock().unwrap().get_or_insert_with(HashMap::new).insert(
        h,
        Open {
            remote,
            path,
            position: 0,
            append: access & FILE_APPEND_DATA != 0 && access & FILE_WRITE_DATA == 0,
            read: true,
            write: f & flags::WRITE != 0,
            delete_on_close: attributes & FILE_FLAG_DELETE_ON_CLOSE != 0,
            wants: wanted,
            share,
        },
    );
    let existed = existing.is_some() && matches!(disposition, CREATE_ALWAYS | OPEN_ALWAYS);
    SetLastError(if existed { ERROR_ALREADY_EXISTS } else { 0 });
    Ok(h)
}

unsafe extern "system" fn CreateFileW(
    p: *const u16, access: u32, share: u32, sa: usize, disposition: u32, attrs: u32, template: H,
) -> H {
    if let Some(path) = view_w(p) {
        // Directories exist on disk in the staged tree.
        if !view().stat(&path).is_ok_and(|m| m.directory) {
            return create(path, access, share, disposition, attrs)
                .unwrap_or_else(|e| fail(e, INVALID));
        }
    }
    trace_host("CreateFileW", p);
    let h = o::CreateFileW(p, access, share, sa, disposition, attrs, template);
    observe_open(h, p);
    h
}
/// CreateFile2 (used by newer system components) over the same view logic.
unsafe extern "system" fn CreateFile2(
    p: *const u16, access: u32, share: u32, disposition: u32, params: *const CREATEFILE2_EXTENDED_PARAMETERS,
) -> H {
    if let Some(path) = view_w(p) {
        if !view().stat(&path).is_ok_and(|m| m.directory) {
            let flags = params.as_ref().map_or(0, |x| x.dwFileFlags);
            return create(path, access, share, disposition, flags)
                .unwrap_or_else(|e| fail(e, INVALID));
        }
    }
    trace_host("CreateFile2", p);
    o::CreateFile2(p, access, share, disposition, params)
}
/// Level 3 trace: paths the shim leaves to Windows.
unsafe fn trace_host(api: &str, p: *const u16) {
    if *TRACE.get().unwrap_or(&0) >= 3 {
        if let Some(w) = wstr(p) {
            log(&format!("[banana-view] host {api} {}\n", String::from_utf16_lossy(&w)));
        }
    }
}
/// Observe mode: host handles to game files (relative paths), by handle.
static OBSERVED: Mutex<Option<HashMap<H, String>>> = Mutex::new(None);
unsafe fn observe_open(h: H, p: *const u16) {
    if *TRACE.get().unwrap_or(&0) < 3 || VIEW.get().is_some() || h == INVALID {
        return;
    }
    let Some(w) = wstr(p) else { return };
    let path = String::from_utf16_lossy(&w);
    if path.len() > 2 && (path.as_bytes()[1] == b':' || path.starts_with('\\')) {
        return; // absolute: system files and devices
    }
    OBSERVED.lock().unwrap().get_or_insert_with(HashMap::new).insert(h, path);
}
fn observed(h: H, what: std::fmt::Arguments) {
    if *TRACE.get().unwrap_or(&0) < 3 {
        return;
    }
    let path = OBSERVED.lock().unwrap().as_ref().and_then(|o| o.get(&h).cloned());
    if let Some(path) = path {
        log(&format!("[banana-view]   host {path}: {what}\n"));
    }
}
unsafe extern "system" fn CreateFileA(
    p: *const u8, access: u32, share: u32, sa: usize, disposition: u32, attrs: u32, template: H,
) -> H {
    if let Some(path) = view_a(p) {
        if !view().stat(&path).is_ok_and(|m| m.directory) {
            return create(path, access, share, disposition, attrs)
                .unwrap_or_else(|e| fail(e, INVALID));
        }
    }
    if *TRACE.get().unwrap_or(&0) >= 3 {
        if let Some(a) = astr(p) {
            log(&format!("[banana-view] host CreateFileA {}\n", String::from_utf8_lossy(&a)));
        }
    }
    o::CreateFileA(p, access, share, sa, disposition, attrs, template)
}

/// (remote, offset) for an I/O call; `None` when `h` is not ours.
fn io_target(h: H, overlapped: *mut OVERLAPPED, write: bool) -> Option<Result<(u64, u64, bool), u32>> {
    with_file(h, |f| {
        if write && !f.write || !write && !f.read {
            return Err(ERROR_ACCESS_DENIED);
        }
        if !overlapped.is_null() {
            let o = unsafe { &*overlapped };
            let at = unsafe { (o.Anonymous.Anonymous.OffsetHigh as u64) << 32 | o.Anonymous.Anonymous.Offset as u64 };
            return Ok((f.remote, at, at == u64::MAX && write));
        }
        Ok((f.remote, f.position, f.append && write))
    })
}
unsafe fn complete(overlapped: *mut OVERLAPPED, n: u32) {
    if let Some(o) = overlapped.as_mut() {
        o.Internal = 0;
        o.InternalHigh = n as usize;
        if !o.hEvent.is_null() {
            SetEvent((o.hEvent as usize & !1) as HANDLE);
        }
    }
}
unsafe extern "system" fn ReadFile(h: H, b: *mut u8, n: u32, read: *mut u32, ov: *mut OVERLAPPED) -> i32 {
    let Some(target) = io_target(h, ov, false) else {
        let r = o::ReadFile(h, b, n, read, ov);
        observed(h, format_args!("read {n} -> {} {} (overlapped {})", r, read.as_ref().copied().unwrap_or(0), !ov.is_null()));
        return r;
    };
    let (remote, at, _) = match target {
        Ok(t) => t,
        Err(e) => return fail(e, FALSE),
    };
    let buffer = std::slice::from_raw_parts_mut(b, n as usize);
    let r = view().read(remote, at, buffer);
    trace_io(h, format_args!("read {n} at {at} -> {:?} (overlapped {})", r.as_ref().ok(), !ov.is_null()));
    match r {
        Ok(got) => {
            if ov.is_null() {
                with_file(h, |f| f.position = at + got as u64);
            }
            if let Some(r) = read.as_mut() {
                *r = got as u32;
            }
            complete(ov, got as u32);
            if !ov.is_null() && got == 0 && n > 0 {
                return fail(ERROR_HANDLE_EOF, FALSE);
            }
            TRUE
        }
        Err(e) => fail(win32(&e, ""), FALSE),
    }
}
unsafe extern "system" fn WriteFile(h: H, b: *const u8, n: u32, written: *mut u32, ov: *mut OVERLAPPED) -> i32 {
    let Some(target) = io_target(h, ov, true) else {
        return o::WriteFile(h, b, n, written, ov);
    };
    let (remote, mut at, append) = match target {
        Ok(t) => t,
        Err(e) => return fail(e, FALSE),
    };
    if append {
        match view().fstat(remote) {
            Ok(m) => at = m.len,
            Err(e) => return fail(win32(&e, ""), FALSE),
        }
    }
    trace_io(h, format_args!("write {n} at {at}"));
    match view().write(remote, at, std::slice::from_raw_parts(b, n as usize)) {
        Ok(put) => {
            if ov.is_null() {
                with_file(h, |f| f.position = at + put as u64);
            }
            if let Some(w) = written.as_mut() {
                *w = put as u32;
            }
            complete(ov, put as u32);
            TRUE
        }
        Err(e) => fail(win32(&e, ""), FALSE),
    }
}
fn seek(h: H, distance: i64, method: u32) -> Option<Result<u64, u32>> {
    let (remote, position) = with_file(h, |f| (f.remote, f.position))?;
    let base = match method {
        FILE_BEGIN => 0,
        FILE_CURRENT => position as i64,
        FILE_END => match view().fstat(remote) {
            Ok(m) => m.len as i64,
            Err(e) => return Some(Err(win32(&e, ""))),
        },
        _ => return Some(Err(ERROR_INVALID_PARAMETER)),
    };
    let target = base.checked_add(distance).filter(|&t| t >= 0);
    let Some(target) = target else {
        return Some(Err(ERROR_NEGATIVE_SEEK));
    };
    with_file(h, |f| f.position = target as u64);
    Some(Ok(target as u64))
}
unsafe extern "system" fn SetFilePointer(h: H, low: i32, high: *mut i32, method: u32) -> u32 {
    if !ours(h) {
        let r = o::SetFilePointer(h, low, high, method);
        observed(h, format_args!("seek {low} from {method} -> {r}"));
        return r;
    }
    let distance = match high.as_ref() {
        Some(&hi) => ((hi as i64) << 32) | low as u32 as i64,
        None => low as i64,
    };
    let r = seek(h, distance, method).unwrap();
    trace_io(h, format_args!("seek {distance} from {method} (high {}) -> {r:?}", !high.is_null()));
    match r {
        Ok(p) => {
            if let Some(hi) = high.as_mut() {
                *hi = (p >> 32) as i32;
            }
            SetLastError(0);
            p as u32
        }
        Err(e) => fail(e, INVALID_SET_FILE_POINTER),
    }
}
unsafe extern "system" fn SetFilePointerEx(h: H, distance: i64, new: *mut i64, method: u32) -> i32 {
    if !ours(h) {
        let r = o::SetFilePointerEx(h, distance, new, method);
        observed(h, format_args!("seek64 {distance} from {method} -> {}", new.as_ref().copied().unwrap_or(-1)));
        return r;
    }
    let r = seek(h, distance, method).unwrap();
    trace_io(h, format_args!("seek64 {distance} from {method} -> {r:?}"));
    match r {
        Ok(p) => {
            if let Some(n) = new.as_mut() {
                *n = p as i64;
            }
            TRUE
        }
        Err(e) => fail(e, FALSE),
    }
}
fn size(h: H) -> Option<Result<u64, u32>> {
    let remote = with_file(h, |f| f.remote)?;
    Some(view().fstat(remote).map(|m| m.len).map_err(|e| win32(&e, "")))
}
unsafe extern "system" fn GetFileSize(h: H, high: *mut u32) -> u32 {
    let r = size(h);
    if r.is_some() {
        trace_io(h, format_args!("size -> {r:?}"));
    }
    match r {
        None => {
            let r = o::GetFileSize(h, high);
            observed(h, format_args!("size -> {r}"));
            r
        }
        Some(Ok(n)) => {
            if let Some(hi) = high.as_mut() {
                *hi = (n >> 32) as u32;
            }
            SetLastError(0);
            n as u32
        }
        Some(Err(e)) => fail(e, INVALID_FILE_SIZE),
    }
}
unsafe extern "system" fn GetFileSizeEx(h: H, out: *mut i64) -> i32 {
    let r = size(h);
    if r.is_some() {
        trace_io(h, format_args!("size64 -> {r:?}"));
    }
    match r {
        None => {
            let r = o::GetFileSizeEx(h, out);
            observed(h, format_args!("size64 -> {}", *out));
            r
        }
        Some(Ok(n)) => {
            *out = n as i64;
            TRUE
        }
        Some(Err(e)) => fail(e, FALSE),
    }
}
unsafe extern "system" fn GetFileType(h: H) -> u32 {
    if ours(h) {
        trace_io(h, format_args!("type"));
        FILE_TYPE_DISK
    } else {
        o::GetFileType(h)
    }
}
unsafe extern "system" fn FlushFileBuffers(h: H) -> i32 {
    if ours(h) { TRUE } else { o::FlushFileBuffers(h) }
}
unsafe extern "system" fn SetEndOfFile(h: H) -> i32 {
    let Some((remote, position, write)) = with_file(h, |f| (f.remote, f.position, f.write)) else {
        return o::SetEndOfFile(h);
    };
    if !write {
        return fail(ERROR_ACCESS_DENIED, FALSE);
    }
    match view().truncate(remote, position) {
        Ok(()) => TRUE,
        Err(e) => fail(win32(&e, ""), FALSE),
    }
}
fn time() -> FILETIME {
    *TIME.get().unwrap()
}
fn file_index(path: &str) -> u64 {
    path.bytes()
        .fold(0xcbf29ce484222325u64, |h, b| (h ^ b.to_ascii_lowercase() as u64).wrapping_mul(0x100000001b3))
}
fn attributes(m: &Meta) -> u32 {
    if m.directory { FILE_ATTRIBUTE_DIRECTORY } else { FILE_ATTRIBUTE_ARCHIVE }
}
unsafe extern "system" fn GetFileInformationByHandle(h: H, info: *mut BY_HANDLE_FILE_INFORMATION) -> i32 {
    let Some((remote, path)) = with_file(h, |f| (f.remote, f.path.clone())) else {
        observed(h, format_args!("information"));
        return o::GetFileInformationByHandle(h, info);
    };
    trace_io(h, format_args!("information"));
    let m = match view().fstat(remote) {
        Ok(m) => m,
        Err(e) => return fail(win32(&e, ""), FALSE),
    };
    let index = file_index(&path);
    *info = BY_HANDLE_FILE_INFORMATION {
        dwFileAttributes: attributes(&m),
        ftCreationTime: time(),
        ftLastAccessTime: time(),
        ftLastWriteTime: time(),
        dwVolumeSerialNumber: 0xba7a_7a00,
        nFileSizeHigh: (m.len >> 32) as u32,
        nFileSizeLow: m.len as u32,
        nNumberOfLinks: 1,
        nFileIndexHigh: (index >> 32) as u32,
        nFileIndexLow: index as u32,
    };
    TRUE
}
/// The commonly queried classes: basic (0), standard (1), attribute tag (9).
unsafe extern "system" fn GetFileInformationByHandleEx(h: H, class: i32, out: *mut c_void, n: u32) -> i32 {
    let Some((remote, path)) = with_file(h, |f| (f.remote, f.path.clone())) else {
        observed(h, format_args!("information class {class}"));
        return o::GetFileInformationByHandleEx(h, class, out, n);
    };
    trace_io(h, format_args!("information class {class}"));
    let m = match view().fstat(remote) {
        Ok(m) => m,
        Err(e) => return fail(win32(&e, &path), FALSE),
    };
    let t = time();
    let ticks = (t.dwHighDateTime as i64) << 32 | t.dwLowDateTime as i64;
    match class {
        0 if n as usize >= size_of::<FILE_BASIC_INFO>() => {
            *(out as *mut FILE_BASIC_INFO) = FILE_BASIC_INFO {
                CreationTime: ticks,
                LastAccessTime: ticks,
                LastWriteTime: ticks,
                ChangeTime: ticks,
                FileAttributes: attributes(&m),
            };
            TRUE
        }
        1 if n as usize >= size_of::<FILE_STANDARD_INFO>() => {
            *(out as *mut FILE_STANDARD_INFO) = FILE_STANDARD_INFO {
                AllocationSize: m.len.next_multiple_of(4096) as i64,
                EndOfFile: m.len as i64,
                NumberOfLinks: 1,
                DeletePending: false,
                Directory: m.directory,
            };
            TRUE
        }
        9 if n as usize >= size_of::<FILE_ATTRIBUTE_TAG_INFO>() => {
            *(out as *mut FILE_ATTRIBUTE_TAG_INFO) = FILE_ATTRIBUTE_TAG_INFO {
                FileAttributes: attributes(&m),
                ReparseTag: 0,
            };
            TRUE
        }
        0 | 1 | 9 => fail(ERROR_BAD_LENGTH, FALSE),
        _ => {
            trace_io(h, format_args!("unsupported information class {class}"));
            fail(ERROR_INVALID_PARAMETER, FALSE)
        }
    }
}
/// The staged host path of a view file (`\\?\` form unless asked otherwise).
unsafe extern "system" fn GetFinalPathNameByHandleW(h: H, out: *mut u16, n: u32, flags: u32) -> u32 {
    let Some(path) = with_file(h, |f| f.path.clone()) else {
        return o::GetFinalPathNameByHandleW(h, out, n, flags);
    };
    let mut full: Vec<u16> = r"\\?\".encode_utf16().collect();
    full.extend_from_slice(ROOT.get().unwrap());
    for part in path.split('/') {
        full.push(b'\\' as u16);
        full.extend(part.encode_utf16());
    }
    if n as usize <= full.len() {
        return full.len() as u32 + 1;
    }
    std::ptr::copy_nonoverlapping(full.as_ptr(), out, full.len());
    *out.add(full.len()) = 0;
    full.len() as u32
}
unsafe extern "system" fn GetFileTime(h: H, c: *mut FILETIME, a: *mut FILETIME, w: *mut FILETIME) -> i32 {
    if !ours(h) {
        return o::GetFileTime(h, c, a, w);
    }
    trace_io(h, format_args!("times"));
    for t in [c, a, w] {
        if let Some(t) = t.as_mut() {
            *t = time();
        }
    }
    TRUE
}
unsafe extern "system" fn CloseHandle(h: H) -> i32 {
    if *TRACE.get().unwrap_or(&0) >= 3 {
        observed(h, format_args!("close"));
        if let Some(o) = OBSERVED.lock().unwrap().as_mut() {
            o.remove(&h);
        }
    }
    let open = FILES.lock().unwrap().as_mut().and_then(|f| f.remove(&h));
    if let Some(open) = open {
        trace("close", &open.path);
        let _ = view().close(open.remote);
        if open.delete_on_close {
            let _ = view().remove(&open.path);
        }
    }
    o::CloseHandle(h)
}

// ---- attributes and directories --------------------------------------------

unsafe fn attributes_of(path: &str) -> u32 {
    trace("attributes", path);
    match view().stat(path) {
        Ok(m) => attributes(&m),
        Err(e) => fail(win32(&e, path), INVALID_FILE_ATTRIBUTES),
    }
}
unsafe extern "system" fn GetFileAttributesW(p: *const u16) -> u32 {
    match view_w(p) {
        Some(path) => attributes_of(&path),
        None => o::GetFileAttributesW(p),
    }
}
unsafe extern "system" fn GetFileAttributesA(p: *const u8) -> u32 {
    match view_a(p) {
        Some(path) => attributes_of(&path),
        None => o::GetFileAttributesA(p),
    }
}
unsafe fn attribute_data(path: &str, out: *mut c_void) -> i32 {
    match view().stat(path) {
        Ok(m) => {
            *(out as *mut WIN32_FILE_ATTRIBUTE_DATA) = WIN32_FILE_ATTRIBUTE_DATA {
                dwFileAttributes: attributes(&m),
                ftCreationTime: time(),
                ftLastAccessTime: time(),
                ftLastWriteTime: time(),
                nFileSizeHigh: (m.len >> 32) as u32,
                nFileSizeLow: m.len as u32,
            };
            TRUE
        }
        Err(e) => fail(win32(&e, path), FALSE),
    }
}
unsafe extern "system" fn GetFileAttributesExW(p: *const u16, level: u32, out: *mut c_void) -> i32 {
    match view_w(p) {
        Some(path) if level == 0 => attribute_data(&path, out),
        _ => o::GetFileAttributesExW(p, level, out),
    }
}
unsafe extern "system" fn GetFileAttributesExA(p: *const u8, level: u32, out: *mut c_void) -> i32 {
    match view_a(p) {
        Some(path) if level == 0 => attribute_data(&path, out),
        _ => o::GetFileAttributesExA(p, level, out),
    }
}

/// Windows wildcard match (`*`, `?`; `*.*` is everything; case-insensitive).
fn matches(pattern: &str, name: &str) -> bool {
    let pattern = if pattern == "*.*" { "*" } else { pattern };
    if let Some(stem) = pattern.strip_suffix('.') {
        if !name.contains('.') {
            return matches(stem, name);
        }
    }
    let (p, n): (Vec<char>, Vec<char>) = (
        pattern.to_lowercase().chars().collect(),
        name.to_lowercase().chars().collect(),
    );
    let (mut pi, mut ni, mut star, mut mark) = (0, 0, None, 0);
    while ni < n.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == n[ni]) {
            pi += 1;
            ni += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ni;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ni = mark;
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|&c| c == '*')
}
/// Entries matching a search path; `None` when it is not under the root.
fn search(full: &[u16], directories_only: bool) -> Option<Result<Vec<(String, Meta)>, u32>> {
    let text = String::from_utf16(&full_path(full)?).ok()?;
    let (directory, pattern) = text.rsplit_once('\\')?;
    let path = view_of(&directory.encode_utf16().collect::<Vec<_>>())?;
    trace(&format!("find {pattern} in"), &path);
    let listing = match view().read_dir(&path) {
        Ok(l) => l,
        Err(e) => {
            let code = if e.kind() == io::ErrorKind::NotFound { ERROR_PATH_NOT_FOUND } else { win32(&e, &path) };
            return Some(Err(code));
        }
    };
    let dot = Meta { directory: true, ..Meta::default() };
    let entries: Vec<_> = [(".".to_owned(), dot), ("..".to_owned(), dot)]
        .into_iter()
        .chain(listing)
        .filter(|(n, m)| matches(pattern, n) && (!directories_only || m.directory))
        .collect();
    Some(if entries.is_empty() { Err(ERROR_FILE_NOT_FOUND) } else { Ok(entries) })
}
unsafe fn fill_w(out: *mut WIN32_FIND_DATAW, (name, m): &(String, Meta)) {
    let mut d: WIN32_FIND_DATAW = std::mem::zeroed();
    d.dwFileAttributes = attributes(m);
    d.ftCreationTime = time();
    d.ftLastAccessTime = time();
    d.ftLastWriteTime = time();
    d.nFileSizeHigh = (m.len >> 32) as u32;
    d.nFileSizeLow = m.len as u32;
    for (i, c) in name.encode_utf16().take(259).enumerate() {
        d.cFileName[i] = c;
    }
    *out = d;
}
unsafe fn fill_a(out: *mut WIN32_FIND_DATAA, (name, m): &(String, Meta)) {
    let mut d: WIN32_FIND_DATAA = std::mem::zeroed();
    d.dwFileAttributes = attributes(m);
    d.ftCreationTime = time();
    d.ftLastAccessTime = time();
    d.ftLastWriteTime = time();
    d.nFileSizeHigh = (m.len >> 32) as u32;
    d.nFileSizeLow = m.len as u32;
    for (i, c) in w2a(&name.encode_utf16().collect::<Vec<_>>()).into_iter().take(259).enumerate() {
        d.cFileName[i] = c as _;
    }
    *out = d;
}
unsafe fn start_find(entries: Vec<(String, Meta)>) -> H {
    let h = placeholder();
    if h != INVALID {
        FINDS.lock().unwrap().get_or_insert_with(HashMap::new).insert(h, Find { entries, next: 1 });
    }
    h
}
unsafe fn first(full: Option<Vec<u16>>, directories: bool, fill: impl FnOnce(&(String, Meta))) -> Option<H> {
    if VIEW.get().is_none() {
        return None;
    }
    match search(&full?, directories)? {
        Ok(entries) => {
            fill(&entries[0]);
            Some(start_find(entries))
        }
        Err(e) => Some(fail(e, INVALID)),
    }
}
unsafe extern "system" fn FindFirstFileW(p: *const u16, out: *mut WIN32_FIND_DATAW) -> H {
    first(wstr(p), false, |e| fill_w(out, e)).unwrap_or_else(|| o::FindFirstFileW(p, out))
}
unsafe extern "system" fn FindFirstFileA(p: *const u8, out: *mut WIN32_FIND_DATAA) -> H {
    first(astr(p).map(|a| a2w(&a)), false, |e| fill_a(out, e)).unwrap_or_else(|| o::FindFirstFileA(p, out))
}
unsafe extern "system" fn FindFirstFileExW(p: *const u16, l: u32, out: *mut c_void, s: u32, f: usize, x: u32) -> H {
    first(wstr(p), s == 1, |e| fill_w(out.cast(), e)).unwrap_or_else(|| o::FindFirstFileExW(p, l, out, s, f, x))
}
unsafe extern "system" fn FindFirstFileExA(p: *const u8, l: u32, out: *mut c_void, s: u32, f: usize, x: u32) -> H {
    first(astr(p).map(|a| a2w(&a)), s == 1, |e| fill_a(out.cast(), e))
        .unwrap_or_else(|| o::FindFirstFileExA(p, l, out, s, f, x))
}
fn next(h: H) -> Option<Option<(String, Meta)>> {
    let mut finds = FINDS.lock().unwrap();
    let find = finds.as_mut()?.get_mut(&h)?;
    let entry = find.entries.get(find.next).cloned();
    find.next += 1;
    Some(entry)
}
unsafe extern "system" fn FindNextFileW(h: H, out: *mut WIN32_FIND_DATAW) -> i32 {
    match next(h) {
        None => o::FindNextFileW(h, out),
        Some(Some(e)) => {
            fill_w(out, &e);
            TRUE
        }
        Some(None) => fail(ERROR_NO_MORE_FILES, FALSE),
    }
}
unsafe extern "system" fn FindNextFileA(h: H, out: *mut WIN32_FIND_DATAA) -> i32 {
    match next(h) {
        None => o::FindNextFileA(h, out),
        Some(Some(e)) => {
            fill_a(out, &e);
            TRUE
        }
        Some(None) => fail(ERROR_NO_MORE_FILES, FALSE),
    }
}
unsafe extern "system" fn FindClose(h: H) -> i32 {
    if FINDS.lock().unwrap().as_mut().and_then(|f| f.remove(&h)).is_some() {
        return o::CloseHandle(h);
    }
    o::FindClose(h)
}

unsafe fn result(r: io::Result<()>, path: &str) -> i32 {
    trace(if r.is_ok() { "changed" } else { "failed to change" }, path);
    match r {
        Ok(()) => TRUE,
        Err(e) => fail(win32(&e, path), FALSE),
    }
}
unsafe fn delete(path: &str) -> i32 {
    if view().stat(path).is_ok_and(|m| m.directory) {
        return fail(ERROR_ACCESS_DENIED, FALSE);
    }
    if in_use(path) {
        trace("in use, not deleted:", path);
        return fail(ERROR_SHARING_VIOLATION, FALSE);
    }
    result(view().remove(path), path)
}
unsafe extern "system" fn DeleteFileW(p: *const u16) -> i32 {
    match view_w(p) {
        Some(path) => delete(&path),
        None => o::DeleteFileW(p),
    }
}
unsafe extern "system" fn DeleteFileA(p: *const u8) -> i32 {
    match view_a(p) {
        Some(path) => delete(&path),
        None => o::DeleteFileA(p),
    }
}
unsafe fn make_directory(path: &str) -> i32 {
    match view().mkdir(path) {
        Ok(()) => TRUE,
        Err(e) if banana_wire::errno_of(&e) == errno::EEXIST => fail(ERROR_ALREADY_EXISTS, FALSE),
        Err(e) => fail(win32(&e, path), FALSE),
    }
}
unsafe extern "system" fn CreateDirectoryW(p: *const u16, sa: usize) -> i32 {
    let Some(path) = view_w(p) else { return o::CreateDirectoryW(p, sa) };
    let made = make_directory(&path);
    if made == TRUE {
        // Mirror it on disk so the game can make it its current directory.
        o::CreateDirectoryW(p, sa);
        SetLastError(0);
    }
    made
}
unsafe extern "system" fn CreateDirectoryA(p: *const u8, sa: usize) -> i32 {
    let Some(path) = view_a(p) else { return o::CreateDirectoryA(p, sa) };
    let made = make_directory(&path);
    if made == TRUE {
        o::CreateDirectoryA(p, sa);
        SetLastError(0);
    }
    made
}
unsafe extern "system" fn RemoveDirectoryW(p: *const u16) -> i32 {
    let Some(path) = view_w(p) else { return o::RemoveDirectoryW(p) };
    let removed = result(view().remove(&path), &path);
    if removed == TRUE {
        o::RemoveDirectoryW(p);
        SetLastError(0);
    }
    removed
}
unsafe extern "system" fn RemoveDirectoryA(p: *const u8) -> i32 {
    let Some(path) = view_a(p) else { return o::RemoveDirectoryA(p) };
    let removed = result(view().remove(&path), &path);
    if removed == TRUE {
        o::RemoveDirectoryA(p);
        SetLastError(0);
    }
    removed
}
/// `None` when neither path is in the view.
unsafe fn move_file(from: Option<String>, to: Option<String>, replace: bool) -> Option<i32> {
    match (from, to) {
        (None, None) => None,
        (Some(from), Some(to)) => {
            if in_use(&from) || in_use(&to) {
                return Some(fail(ERROR_SHARING_VIOLATION, FALSE));
            }
            if !replace && view().stat(&to).is_ok() {
                return Some(fail(ERROR_ALREADY_EXISTS, FALSE));
            }
            Some(result(view().rename(&from, &to), &from))
        }
        _ => Some(fail(ERROR_NOT_SAME_DEVICE, FALSE)),
    }
}
unsafe extern "system" fn MoveFileW(a: *const u16, b: *const u16) -> i32 {
    move_file(view_w(a), view_w(b), false).unwrap_or_else(|| o::MoveFileW(a, b))
}
unsafe extern "system" fn MoveFileA(a: *const u8, b: *const u8) -> i32 {
    move_file(view_a(a), view_a(b), false).unwrap_or_else(|| o::MoveFileA(a, b))
}
unsafe extern "system" fn MoveFileExW(a: *const u16, b: *const u16, f: u32) -> i32 {
    move_file(view_w(a), view_w(b), f & MOVEFILE_REPLACE_EXISTING != 0).unwrap_or_else(|| o::MoveFileExW(a, b, f))
}
unsafe extern "system" fn MoveFileExA(a: *const u8, b: *const u8, f: u32) -> i32 {
    move_file(view_a(a), view_a(b), f & MOVEFILE_REPLACE_EXISTING != 0).unwrap_or_else(|| o::MoveFileExA(a, b, f))
}

// ---- mappings ----------------------------------------------------------------

/// A pagefile-backed section holding a copy of the file: reads see the view;
/// writes through the mapping are not persisted.
unsafe fn mapping(h: H, sa: usize, protect: u32, high: u32, low: u32, name: *const u16) -> Option<H> {
    let remote = with_file(h, |f| f.remote)?;
    trace("map", &with_file(h, |f| f.path.clone()).unwrap_or_default());
    if protect & SEC_IMAGE != 0 {
        return Some(fail(ERROR_ACCESS_DENIED, 0));
    }
    let len = match view().fstat(remote) {
        Ok(m) => m.len.max((high as u64) << 32 | low as u64),
        Err(e) => return Some(fail(win32(&e, ""), 0)),
    };
    if len == 0 {
        return Some(fail(ERROR_FILE_INVALID, 0));
    }
    let execute = protect & 0xf0 != 0;
    let page = if execute { PAGE_EXECUTE_READWRITE } else { PAGE_READWRITE };
    let section = o::CreateFileMappingW(INVALID, sa, page, (len >> 32) as u32, len as u32, name);
    if section == 0 {
        return Some(0);
    }
    let address = MapViewOfFile(section as HANDLE, FILE_MAP_WRITE, 0, 0, len as usize);
    if address.Value.is_null() {
        o::CloseHandle(section);
        return Some(0);
    }
    let bytes = std::slice::from_raw_parts_mut(address.Value as *mut u8, len as usize);
    let ok = view().read(remote, 0, bytes).is_ok();
    UnmapViewOfFile(address);
    if !ok {
        o::CloseHandle(section);
        return Some(fail(ERROR_READ_FAULT, 0));
    }
    Some(section)
}
unsafe extern "system" fn CreateFileMappingW(h: H, sa: usize, p: u32, hi: u32, lo: u32, name: *const u16) -> H {
    mapping(h, sa, p, hi, lo, name).unwrap_or_else(|| {
        observed(h, format_args!("mapping {p:#x}"));
        o::CreateFileMappingW(h, sa, p, hi, lo, name)
    })
}
unsafe extern "system" fn CreateFileMappingA(h: H, sa: usize, p: u32, hi: u32, lo: u32, name: *const u8) -> H {
    if !ours(h) {
        return o::CreateFileMappingA(h, sa, p, hi, lo, name);
    }
    let name = astr(name).map(|n| a2w(&n).into_iter().chain(Some(0)).collect::<Vec<_>>());
    mapping(h, sa, p, hi, lo, name.as_ref().map_or(ptr::null(), |n| n.as_ptr())).unwrap_or(0)
}
use windows_sys::Win32::System::Memory::{PAGE_EXECUTE_READWRITE, PAGE_READWRITE, SEC_IMAGE};

// ---- profiles ----------------------------------------------------------------

/// Profile file names without a directory (`win.ini`, `system.ini`) live in
/// the Windows directory, never the current one: those stay with Windows.
fn bare_name(name: &[u16]) -> bool {
    !name.iter().any(|&c| c == b'\\' as u16 || c == b'/' as u16 || c == b':' as u16)
}
unsafe fn profile_w(f: *const u16) -> Option<String> {
    if wstr(f).is_none_or(|n| bare_name(&n)) {
        return None;
    }
    view_w(f)
}
unsafe fn profile_a(f: *const u8) -> Option<String> {
    if astr(f).is_none_or(|n| bare_name(&a2w(&n))) {
        return None;
    }
    view_a(f)
}
fn profile_text(path: &str) -> Vec<u8> {
    trace("profile", path);
    view().read_all(path, 16 * 1024 * 1024).unwrap_or_default()
}
fn profile_write(path: &str, text: &[u8]) -> io::Result<()> {
    let (h, _) = view().open(path, flags::READ | flags::WRITE | flags::CREATE | flags::TRUNCATE)?;
    let r = view().write(h, 0, text).map(|_| ());
    let _ = view().close(h);
    r
}
/// The profile value or name list, as bytes in the file's encoding.
fn profile_get(path: &str, app: Option<&[u8]>, key: Option<&[u8]>, default: &[u8], out: &mut [u8]) -> usize {
    let text = profile_text(path);
    match (app, key) {
        (None, _) => ini::copy_list(&ini::names(&text, None), out),
        (Some(app), None) => ini::copy_list(&ini::names(&text, Some(app)), out),
        (Some(app), Some(key)) => {
            let value = ini::value(&text, app, key).unwrap_or_else(|| {
                let d = default;
                let end = d.iter().rposition(|c| *c != b' ').map_or(0, |i| i + 1);
                d[..end].to_vec()
            });
            ini::copy_string(&value, out)
        }
    }
}
unsafe extern "system" fn GetPrivateProfileStringA(
    a: *const u8, k: *const u8, d: *const u8, out: *mut u8, n: u32, f: *const u8,
) -> u32 {
    let Some(path) = profile_a(f) else { return o::GetPrivateProfileStringA(a, k, d, out, n, f) };
    if out.is_null() {
        return 0;
    }
    let out = std::slice::from_raw_parts_mut(out, n as usize);
    profile_get(&path, astr(a).as_deref(), astr(k).as_deref(), &astr(d).unwrap_or_default(), out) as u32
}
unsafe extern "system" fn GetPrivateProfileStringW(
    a: *const u16, k: *const u16, d: *const u16, out: *mut u16, n: u32, f: *const u16,
) -> u32 {
    let Some(path) = profile_w(f) else { return o::GetPrivateProfileStringW(a, k, d, out, n, f) };
    if out.is_null() || n == 0 {
        return 0;
    }
    let conv = |p: *const u16| wstr(p).map(|w| w2a(&w));
    let mut bytes = vec![0u8; n as usize * 2];
    let count = profile_get(&path, conv(a).as_deref(), conv(k).as_deref(), &conv(d).unwrap_or_default(), &mut bytes);
    // Convert the result (NUL-separated names or one string) back to UTF-16.
    let end = if a.is_null() || k.is_null() { count + 2 } else { count + 1 };
    let wide = a2w(&bytes[..end.min(bytes.len())]);
    let out = std::slice::from_raw_parts_mut(out, n as usize);
    let m = wide.len().min(out.len());
    out[..m].copy_from_slice(&wide[..m]);
    let full = m == out.len();
    if let (true, Some(last)) = (full, out.last_mut()) {
        *last = 0;
    }
    wide.len().saturating_sub(end - count).min(n as usize - 1) as u32
}
unsafe extern "system" fn GetPrivateProfileIntA(a: *const u8, k: *const u8, d: i32, f: *const u8) -> u32 {
    let Some(path) = profile_a(f) else { return o::GetPrivateProfileIntA(a, k, d, f) };
    let (Some(app), Some(key)) = (astr(a), astr(k)) else { return d as u32 };
    ini::value(&profile_text(&path), &app, &key).map_or(d as u32, |v| ini::int(&v, d))
}
unsafe extern "system" fn GetPrivateProfileIntW(a: *const u16, k: *const u16, d: i32, f: *const u16) -> u32 {
    let Some(path) = profile_w(f) else { return o::GetPrivateProfileIntW(a, k, d, f) };
    let (Some(app), Some(key)) = (wstr(a), wstr(k)) else { return d as u32 };
    ini::value(&profile_text(&path), &w2a(&app), &w2a(&key)).map_or(d as u32, |v| ini::int(&v, d))
}
unsafe fn profile_set(path: &str, app: Option<Vec<u8>>, key: Option<Vec<u8>>, value: Option<Vec<u8>>) -> i32 {
    let Some(app) = app else { return TRUE }; // Flush request.
    let text = ini::set(&profile_text(path), &app, key.as_deref(), value.as_deref());
    result(profile_write(path, &text), path)
}
unsafe extern "system" fn WritePrivateProfileStringA(a: *const u8, k: *const u8, v: *const u8, f: *const u8) -> i32 {
    match profile_a(f) {
        Some(path) => profile_set(&path, astr(a), astr(k), astr(v)),
        None => o::WritePrivateProfileStringA(a, k, v, f),
    }
}
unsafe extern "system" fn WritePrivateProfileStringW(a: *const u16, k: *const u16, v: *const u16, f: *const u16) -> i32 {
    match profile_w(f) {
        Some(path) => {
            let conv = |p: *const u16| wstr(p).map(|w| w2a(&w));
            profile_set(&path, conv(a), conv(k), conv(v))
        }
        None => o::WritePrivateProfileStringW(a, k, v, f),
    }
}

// ---- table -------------------------------------------------------------------

fn detours() -> Vec<(&'static str, usize, &'static AtomicUsize)> {
    macro_rules! t { ($($n:ident),*) => { vec![$((stringify!($n), $n as *const () as usize, &slot::$n)),*] } }
    t!(
        CreateFileA, CreateFileW, CreateFile2, ReadFile, WriteFile, SetFilePointer,
        SetFilePointerEx, GetFileSize, GetFileSizeEx, GetFileType, CloseHandle,
        FlushFileBuffers, SetEndOfFile, GetFileInformationByHandle,
        GetFileInformationByHandleEx, GetFinalPathNameByHandleW, GetFileTime,
        GetFileAttributesA, GetFileAttributesW,
        GetFileAttributesExA, GetFileAttributesExW, FindFirstFileA, FindFirstFileW,
        FindFirstFileExA, FindFirstFileExW, FindNextFileA, FindNextFileW, FindClose,
        DeleteFileA, DeleteFileW, CreateDirectoryA, CreateDirectoryW, RemoveDirectoryA,
        RemoveDirectoryW, MoveFileA, MoveFileW, MoveFileExA, MoveFileExW,
        CreateFileMappingA, CreateFileMappingW, GetPrivateProfileStringA,
        GetPrivateProfileStringW, GetPrivateProfileIntA, GetPrivateProfileIntW,
        WritePrivateProfileStringA, WritePrivateProfileStringW
    )
}
