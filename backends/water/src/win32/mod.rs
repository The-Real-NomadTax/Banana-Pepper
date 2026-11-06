//! Measured ELF32 API translation for the fingerprinted Big Buck build.
//! Guest instructions run directly in a separate 32-bit Windows process.
#![allow(unsafe_op_in_unsafe_fn)]
mod crt;
mod display;
mod jamma;
mod rawinput;
mod sound;
/// XInput and DirectInput controllers (shared with the Super Cars and Ford
/// adapter DLLs).
#[path = "../../../../guest/win32/xinput.rs"]
mod xinput;
#[path = "../../../../guest/win32/dinput.rs"]
mod dinput;
#[path = "../../../../guest/win32/controllers.rs"]
#[allow(dead_code)] // Water reads every player; the hook DLLs read player one.
mod controllers;
use banana_input::pad;

use goblin::elf::{Elf, program_header::PT_LOAD};
use std::{
    ffi::{CStr, CString, c_void},
    fs,
    path::{Path, PathBuf},
    ptr,
    sync::OnceLock,
};
use windows_sys::Win32::System::{
    Diagnostics::Debug::*, LibraryLoader::*, Memory::*, Threading::*,
};

pub(super) static GAME: OnceLock<PathBuf> = OnceLock::new();
pub(super) static WORK: OnceLock<PathBuf> = OnceLock::new();
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

pub fn launch() -> Result<(), String> {
    let mut args = std::env::args_os().skip(1);
    let source = PathBuf::from(
        args.next()
            .ok_or("Usage: water-buck GAME_FOLDER [--work PATH] [--timeout SECONDS] [--worker]")?,
    );
    let mut work = std::env::current_dir()
        .map_err(|e| e.to_string())?
        .join("runtime/big-buck-native");
    let mut timeout = None;
    let mut worker = false;
    while let Some(arg) = args.next() {
        if arg == "--worker" {
            worker = true;
        } else if arg == "--work" {
            work = PathBuf::from(args.next().ok_or("--work requires a path")?);
        } else if arg == "--timeout" {
            let value = args
                .next()
                .ok_or("--timeout requires seconds")?
                .to_string_lossy()
                .parse::<u64>()
                .map_err(|e| e.to_string())?;
            if !(1..=3600).contains(&value) {
                return Err("Timeout must be 1..3600 seconds".into());
            }
            timeout = Some(value);
        } else {
            return Err(format!("Unknown argument {}", arg.to_string_lossy()));
        }
    }
    let source = source.canonicalize().map_err(|e| e.to_string())?;
    let source = if source.is_file() {
        source.parent().ok_or("No game directory")?.to_path_buf()
    } else {
        source
    };
    fs::create_dir_all(&work).map_err(|e| e.to_string())?;
    let work = work.canonicalize().map_err(|e| e.to_string())?;
    if !worker {
        use std::{
            process::{Command, Stdio},
            time::{Duration, Instant},
        };
        let lock = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(work.join("session.lock"))
            .map_err(|e| e.to_string())?;
        lock.try_lock()
            .map_err(|e| format!("Another native session is using this save directory: {e}"))?;
        let log = fs::File::create(work.join("runner.log")).map_err(|e| e.to_string())?;
        let mut child = Command::new(std::env::current_exe().map_err(|e| e.to_string())?)
            .arg(&source)
            .arg("--work")
            .arg(&work)
            .arg("--worker")
            .stdout(Stdio::from(log.try_clone().map_err(|e| e.to_string())?))
            .stderr(Stdio::from(log))
            .spawn()
            .map_err(|e| e.to_string())?;
        let started = Instant::now();
        loop {
            if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
                return if status.success() {
                    Ok(())
                } else {
                    Err(format!(
                        "Native guest exited with {status}; see {}",
                        work.join("runner.log").display()
                    ))
                };
            }
            if timeout.is_some_and(|limit| started.elapsed() >= Duration::from_secs(limit)) {
                child.kill().map_err(|e| e.to_string())?;
                child.wait().map_err(|e| e.to_string())?;
                eprintln!(
                    "[water] bounded session ended after {} seconds",
                    timeout.unwrap()
                );
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    // The launcher stages the verified image and saves in `work`; Water
    // itself applies no game-specific preparation.
    if !work.join("game").is_file() {
        return Err(format!("No staged guest image in {}", work.display()));
    }
    GAME.set(source).map_err(|_| "Game already set")?;
    WORK.set(work.clone()).map_err(|_| "Work already set")?;
    let bytes = fs::read(work.join("game")).map_err(|e| e.to_string())?;
    unsafe { load(&bytes) }
}

pub(super) unsafe fn proc(module: *mut c_void, name: &str) -> usize {
    let name = CString::new(name).unwrap();
    GetProcAddress(module, name.as_ptr().cast())
        .map(|f| f as usize)
        .unwrap_or(0)
}

/// Adapt an i386 cdecl call to a Windows stdcall API without changing guest args.
pub(super) unsafe fn stdcall(target: usize, words: usize) -> Result<usize, String> {
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

pub(super) unsafe fn executable(code: &[u8]) -> Result<usize, String> {
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
    let elf = Elf::parse(bytes).map_err(|e| e.to_string())?;
    if elf.is_64 || elf.header.e_machine != 3 || elf.header.e_type != 2 {
        return Err("Expected fingerprinted fixed-address ELF32/i386".into());
    }
    let loads: Vec<_> = elf
        .program_headers
        .iter()
        .filter(|p| p.p_type == PT_LOAD)
        .collect();
    let start = loads
        .iter()
        .map(|p| p.p_vaddr & !65535)
        .min()
        .ok_or("No LOAD segments")?;
    let end = loads
        .iter()
        .map(|p| p.p_vaddr.checked_add(p.p_memsz).ok_or("LOAD overflow"))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .max()
        .unwrap();
    if end - start > 256 * 1024 * 1024 || end > 0x80000000 {
        return Err("Unsupported ELF memory extent".into());
    }
    let memory = VirtualAlloc(
        start as *const c_void,
        (end - start) as usize,
        MEM_COMMIT | MEM_RESERVE,
        PAGE_READWRITE,
    );
    if memory as usize != start as usize {
        return Err(format!(
            "Cannot map ELF at {start:#x}: {}",
            std::io::Error::last_os_error()
        ));
    }
    for p in &loads {
        let data = bytes
            .get(p.p_offset as usize..(p.p_offset + p.p_filesz) as usize)
            .ok_or("LOAD exceeds file")?;
        if p.p_filesz > p.p_memsz {
            return Err("Invalid LOAD sizes".into());
        }
        ptr::copy_nonoverlapping(data.as_ptr(), p.p_vaddr as *mut u8, data.len());
    }
    let mut unresolved = Vec::new();
    let mut count = 0;
    for reloc in elf.dynrels.iter().chain(elf.pltrelocs.iter()) {
        let symbol = elf
            .dynsyms
            .get(reloc.r_sym)
            .ok_or("Bad relocation symbol")?;
        let name = elf
            .dynstrtab
            .get_at(symbol.st_name)
            .ok_or("Bad symbol name")?;
        let address = if symbol.st_shndx != 0 {
            symbol.st_value as usize
        } else {
            let target = resolve(name)?;
            traced(name, target)?
        };
        if address == 0 && symbol.st_bind() != 2 {
            unresolved.push(name.to_owned());
            continue;
        }
        if !loads
            .iter()
            .any(|p| reloc.r_offset >= p.p_vaddr && reloc.r_offset + 4 <= p.p_vaddr + p.p_memsz)
        {
            return Err("Relocation outside guest mapping".into());
        }
        let at = reloc.r_offset as *mut u32;
        match reloc.r_type {
            1 => *at = (*at).wrapping_add(address as u32),
            2 => {
                *at = (*at)
                    .wrapping_add(address as u32)
                    .wrapping_sub(reloc.r_offset as u32)
            }
            6 | 7 => *at = address as u32,
            8 => {} // Fixed ET_EXEC has zero load bias.
            other => return Err(format!("Unsupported ELF32 relocation {other} for {name}")),
        }
        count += 1;
    }
    if !unresolved.is_empty() {
        return Err(format!("Unimplemented imports: {}", unresolved.join(", ")));
    }
    jamma::install()?;
    // This verified legacy build has writable executable segments. The worker
    // is explicitly a compatibility boundary, not an untrusted-code sandbox.
    let mut old = 0;
    if VirtualProtect(
        memory,
        (end - start) as usize,
        PAGE_EXECUTE_READWRITE,
        &mut old,
    ) == 0
    {
        return Err(std::io::Error::last_os_error().to_string());
    }
    FlushInstructionCache(GetCurrentProcess(), memory, (end - start) as usize);
    AddVectoredExceptionHandler(1, Some(fault));
    eprintln!(
        "[water] mapped ELF32 {start:#x}..{end:#x}; resolved {count} relocations; Windows APIs only"
    );
    if ConvertThreadToFiber(ptr::null()).is_null() {
        return Err("Cannot create host fiber".into());
    }
    let guest = CreateFiberEx(
        8 * 1024 * 1024,
        8 * 1024 * 1024,
        0,
        Some(fiber),
        elf.entry as *mut c_void,
    );
    if guest.is_null() {
        return Err("Cannot allocate guest fiber".into());
    }
    SwitchToFiber(guest);
    Err("Guest fiber unexpectedly returned".into())
}

unsafe extern "system" fn fiber(entry: *mut c_void) {
    let current: usize;
    core::arch::asm!("mov {},esp",out(reg)current,options(nostack,readonly));
    let argv0 = CString::new("/game/game").unwrap().into_raw() as u32;
    let initial = [1u32, argv0, 0, 0, 6, 4096, 0, 0];
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

unsafe fn resolve(name: &str) -> Result<usize, String> {
    if let Some(address) = crt::resolve(name) {
        return Ok(address);
    }
    if let Some(address) = display::resolve(name)? {
        return Ok(address);
    }
    if let Some(address) = sound::resolve(name) {
        return Ok(address);
    }
    if name == "__gmon_start__" || name == "_Jv_RegisterClasses" {
        return Ok(0);
    }
    let library = LoadLibraryA(c"msvcrt.dll".as_ptr().cast());
    Ok(proc(library, name))
}

pub(super) unsafe fn text<'a>(p: *const i8) -> &'a str {
    CStr::from_ptr(p).to_str().unwrap_or("")
}
pub(super) fn path(name: &str, write: bool) -> Result<PathBuf, String> {
    if name == "/proc/cpuinfo" && !write {
        let file = WORK.get().unwrap().join("cpuinfo");
        static CPU_MHZ:OnceLock<f64>=OnceLock::new();
        let mhz=CPU_MHZ.get_or_init(||{
            let started = std::time::Instant::now();
            let ticks = unsafe { std::arch::x86::_rdtsc() };
            std::thread::sleep(std::time::Duration::from_millis(50));
            (unsafe { std::arch::x86::_rdtsc() } - ticks) as f64
                / started.elapsed().as_secs_f64()
                / 1_000_000.0
        });
            fs::write(&file,format!("processor\t: 0\nvendor_id\t: BananaPepper\nmodel name\t: Windows x86 host\ncpu MHz\t\t: {mhz:.3}\nbogomips\t: {:.3}\nflags\t\t: fpu tsc mmx sse sse2\n",mhz*2.0)).map_err(|e|e.to_string())?;
        return Ok(file);
    }
    let name = name.replace('\\', "/");
    if name.contains(':') || name.split('/').any(|s| s == "..") {
        return Err(format!("Invalid guest path {name}"));
    }
    let name = name
        .strip_prefix("/game/")
        .unwrap_or(&name)
        .trim_start_matches('/');
    if let Some(rest) = name.strip_prefix("bbh3user/") {
        return Ok(WORK.get().unwrap().join("bbh3user").join(rest));
    }
    if name == "bbh3user" {
        return Ok(WORK.get().unwrap().join(name));
    }
    if write {
        return Err(format!("Read-only game asset: {name}"));
    }
    Ok(GAME.get().unwrap().join(name))
}

pub(super) fn cpath(path: &Path) -> CString {
    // The legacy CRT cannot open extended-path prefixes. Game paths on this
    // runtime are local, bounded filenames, resolved before entering the guest.
    CString::new(path.to_string_lossy().trim_start_matches(r"\\?\")).unwrap()
}
