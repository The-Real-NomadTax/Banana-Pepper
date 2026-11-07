//! ELF32 API translation: the program's imports are bound to Water's own
//! Win32 implementations of the Linux libraries it uses (C library, files,
//! GLUT/OpenGL, OSS audio), optionally overridden by a preload adapter.
//! Guest instructions run directly in a separate 32-bit Windows process.
#![allow(unsafe_op_in_unsafe_fn)]
mod crt;
mod display;
mod files;
mod linker;
mod sound;
use crate::extension::{ADAPTER_VERSION, AdapterV2, HostServicesV2};
use goblin::elf::{Elf, program_header::PT_LOAD};
use std::{
    ffi::{CStr, CString, c_void},
    path::{Path, PathBuf},
    ptr,
    sync::OnceLock,
};
use windows_sys::Win32::System::{
    Diagnostics::Debug::*, LibraryLoader::*, Memory::*, Threading::*,
};

pub static WORK: OnceLock<PathBuf> = OnceLock::new();
/// The program's command line, `argv[0]` being its guest path.
static ARGV: OnceLock<Vec<String>> = OnceLock::new();
static TRACES: std::sync::Mutex<Vec<(String, bool)>> = std::sync::Mutex::new(Vec::new());

unsafe extern "C" fn trace(index: usize) {
    let mut calls = TRACES.lock().unwrap();
    if let Some((name, seen)) = calls.get_mut(index) {
        if !*seen {
            *seen = true;
            eprintln!("[water] import {name}");
        }
    }
}
unsafe fn traced(name: &str, target: usize) -> Result<usize, String> {
    if target == 0 || std::env::var_os("ARCADE_NATIVE_TRACE").is_none() {
        return Ok(target);
    }
    let mut calls = TRACES.lock().unwrap();
    let index = calls.len();
    calls.push((name.into(), false));
    drop(calls);
    let mut code = vec![0x9c, 0x60, 0x68];
    code.extend_from_slice(&(index as u32).to_le_bytes());
    code.push(0xb8);
    code.extend_from_slice(&(trace as *const () as u32).to_le_bytes());
    code.extend_from_slice(&[0xff, 0xd0, 0x83, 0xc4, 0x04, 0x61, 0x9d, 0x68]);
    code.extend_from_slice(&(target as u32).to_le_bytes());
    code.push(0xc3);
    executable(&code)
}

static EXTENSION: OnceLock<&'static AdapterV2> = OnceLock::new();
static SERVICES: HostServicesV2 = HostServicesV2 {
    size: std::mem::size_of::<HostServicesV2>() as u32,
    next: next_service,
    capture: display::capture_service,
    window: display::window_service,
    message_hook: display::message_hook_service,
};
unsafe extern "C" fn next_service(name: *const i8) -> usize {
    if name.is_null() {
        return 0;
    }
    builtin(text(name)).ok().flatten().unwrap_or(0)
}

/// Run `image`, with the optional preload `adapter` loaded first.
pub fn launch(
    image: &[u8],
    argv: Vec<String>,
    state: &Path,
    adapter: Option<&Path>,
) -> Result<(), String> {
    WORK.set(state.to_path_buf())
        .map_err(|_| "State already set")?;
    ARGV.set(argv).map_err(|_| "Arguments already set")?;
    if let Some(adapter) = adapter {
        unsafe { load_adapter(adapter, state)? };
    }
    unsafe { load(image) }
}

unsafe fn load_adapter(adapter: &Path, state: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    let dll: Vec<u16> = adapter.as_os_str().encode_wide().chain(Some(0)).collect();
    let module = LoadLibraryW(dll.as_ptr());
    if module.is_null() {
        return Err(std::io::Error::last_os_error().to_string());
    }
    let entry = CString::new(crate::extension::ADAPTER_ENTRY).unwrap();
    let factory = GetProcAddress(module, entry.as_ptr().cast())
        .ok_or("The Water adapter has no banana_water_adapter_v2 entry point")?;
    let factory: unsafe extern "C" fn() -> *const AdapterV2 = std::mem::transmute(factory);
    let extension = factory().as_ref().ok_or("Null Water adapter")?;
    if extension.version != ADAPTER_VERSION
        || extension.size != std::mem::size_of::<AdapterV2>() as u32
    {
        return Err("Unsupported Water adapter ABI".into());
    }
    // Adapters see the guest's root path, not a host directory.
    let root = CString::new(std::env::var("BANANA_GUEST_ROOT").unwrap_or_else(|_| "/game".into()))
        .map_err(|e| e.to_string())?;
    let state = cpath(state);
    if (extension.initialize)(root.as_ptr(), state.as_ptr(), &SERVICES) != 0 {
        return Err("Water adapter initialization failed".into());
    }
    EXTENSION
        .set(extension)
        .map_err(|_| "Adapter already installed")?;
    Ok(())
}

pub unsafe fn proc(module: *mut c_void, name: &str) -> usize {
    let name = CString::new(name).unwrap();
    GetProcAddress(module, name.as_ptr().cast())
        .map(|f| f as usize)
        .unwrap_or(0)
}

/// Adapt an i386 cdecl call to a Windows stdcall API without changing guest args.
pub unsafe fn stdcall(target: usize, words: usize) -> Result<usize, String> {
    if target == 0 || words > 32 {
        return Err("Invalid stdcall translation".into());
    }
    let mut code = Vec::new();
    for _ in 0..words {
        code.extend_from_slice(&[0xff, 0xb4, 0x24]);
        code.extend_from_slice(&((words * 4) as u32).to_le_bytes());
    }
    code.push(0xb8);
    code.extend_from_slice(&(target as u32).to_le_bytes());
    code.extend_from_slice(&[0xff, 0xd0, 0xc3]);
    executable(&code)
}

pub unsafe fn executable(code: &[u8]) -> Result<usize, String> {
    let memory = VirtualAlloc(
        ptr::null(),
        code.len(),
        MEM_RESERVE | MEM_COMMIT,
        PAGE_READWRITE,
    );
    if memory.is_null() {
        return Err(std::io::Error::last_os_error().to_string());
    }
    ptr::copy_nonoverlapping(code.as_ptr(), memory.cast(), code.len());
    let mut old = 0;
    if VirtualProtect(memory, code.len(), PAGE_EXECUTE_READ, &mut old) == 0
        || FlushInstructionCache(GetCurrentProcess(), memory, code.len()) == 0
    {
        VirtualFree(memory, 0, MEM_RELEASE);
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok(memory as usize)
}

unsafe extern "system" fn fault(info: *mut EXCEPTION_POINTERS) -> i32 {
    if !info.is_null() && !(*info).ContextRecord.is_null() && !(*info).ExceptionRecord.is_null() {
        let code = (*(*info).ExceptionRecord).ExceptionCode as u32;
        eprintln!(
            "[water] Windows exception {code:#x} at {:#x}",
            (*(*info).ContextRecord).Eip
        );
        if matches!(code, 0xc0000005 | 0xc000001d | 0xc0000096 | 0xc00000fd) {
            let c = &*(*info).ContextRecord;
            eprintln!(
                "[water] fault {code:#x} EIP={:#x} ESP={:#x} EAX={:#x} EBX={:#x} ECX={:#x} EDX={:#x} ESI={:#x} EDI={:#x}",
                c.Eip, c.Esp, c.Eax, c.Ebx, c.Ecx, c.Edx, c.Esi, c.Edi
            );
            let mut region: MEMORY_BASIC_INFORMATION = std::mem::zeroed();
            if VirtualQuery(
                c.Esp as *const c_void,
                &mut region,
                std::mem::size_of::<MEMORY_BASIC_INFORMATION>(),
            ) != 0
                && region.State == MEM_COMMIT
                && c.Esp as usize + 48 <= region.BaseAddress as usize + region.RegionSize
            {
                eprintln!(
                    "[water] guest stack: {:x?}",
                    std::slice::from_raw_parts(c.Esp as *const u32, 12)
                );
            }
            ExitProcess(125);
        }
    }
    EXCEPTION_CONTINUE_SEARCH
}

unsafe fn load(bytes: &[u8]) -> Result<(), String> {
    let program = ARGV.get().and_then(|a| a.first()).cloned().unwrap_or_default();
    linker::ARGV_PROGRAM.set(program.clone()).ok();
    let (entry, start, end) = linker::load_program(bytes, &program)?;
    if let Some(adapter) = EXTENSION.get() {
        if (adapter.install)() != 0 {
            return Err("Water adapter patch installation failed".into());
        }
    }
    FlushInstructionCache(GetCurrentProcess(), ptr::null(), 0);
    AddVectoredExceptionHandler(1, Some(fault));
    eprintln!("[water] mapped ELF32 program {start:#x}..{end:#x}; Windows APIs only");
    if ConvertThreadToFiber(ptr::null()).is_null() {
        return Err("Cannot create host fiber".into());
    }
    let guest = CreateFiberEx(
        8 * 1024 * 1024,
        8 * 1024 * 1024,
        0,
        Some(fiber),
        entry as *mut c_void,
    );
    if guest.is_null() {
        return Err("Cannot allocate guest fiber".into());
    }
    SwitchToFiber(guest);
    Err("Guest fiber unexpectedly returned".into())
}

unsafe extern "system" fn fiber(entry: *mut c_void) {
    // Shipped libraries initialize first, dependencies before dependents.
    linker::initialize_libraries();
    let current: usize;
    core::arch::asm!("mov {},esp",out(reg)current,options(nostack,readonly));
    // argc, argv[], NULL, envp[] (empty: the C library reads Windows'
    // environment), NULL, then the auxiliary vector (AT_PAGESZ, AT_NULL).
    let argv = ARGV.get().map(Vec::as_slice).unwrap_or(&[]);
    let mut initial = vec![argv.len() as u32];
    for arg in argv {
        initial.push(CString::new(arg.as_str()).unwrap_or_default().into_raw() as u32);
    }
    initial.extend([0, 0, 6, 4096, 0, 0]);
    let sp = (current - 65536) & !15;
    ptr::copy_nonoverlapping(initial.as_ptr(), sp as *mut u32, initial.len());
    enter(entry as u32, sp as u32)
}

#[unsafe(naked)]
unsafe extern "C" fn enter(_entry: u32, _stack: u32) -> ! {
    core::arch::naked_asm!(
        "mov eax, [esp+4]",
        "mov ecx, [esp+8]",
        "mov esp, ecx",
        "xor edx, edx",
        "xor ebp, ebp",
        "jmp eax"
    );
}

/// Water's own implementation of a Linux library function.
unsafe fn builtin(name: &str) -> Result<Option<usize>, String> {
    if let Some(address) = files::resolve(name) {
        return Ok(Some(address));
    }
    if let Some(address) = crt::resolve(name) {
        return Ok(Some(address));
    }
    if let Some(address) = linker::resolve(name) {
        return Ok(Some(address));
    }
    if let Some(address) = display::resolve(name)? {
        return Ok(Some(address));
    }
    if let Some(address) = sound::resolve(name) {
        return Ok(Some(address));
    }
    if name == "__gmon_start__" || name == "_Jv_RegisterClasses" {
        return Ok(None);
    }
    let library = LoadLibraryA(c"msvcrt.dll".as_ptr().cast());
    Ok(Some(proc(library, name)).filter(|&a| a != 0))
}

pub unsafe fn text<'a>(p: *const i8) -> &'a str {
    CStr::from_ptr(p).to_str().unwrap_or("")
}

pub fn cpath(path: &Path) -> CString {
    // The legacy CRT cannot open extended-path prefixes. Game paths on this
    // runtime are local, bounded filenames, resolved before entering the guest.
    CString::new(path.to_string_lossy().trim_start_matches(r"\\?\")).unwrap()
}
