//! Water's dynamic linker, its `ld.so` for ELF32 programs.
//!
//! The program is mapped at its fixed address; every shared library it
//! needs is either one Water implements itself (a *builtin*, like Wine's
//! builtin DLLs) or an ELF32 shared object the game ships, which is mapped
//! from the session view at a free address. Symbols bind in `ld.so` order:
//! the preload adapter, the program, the shipped libraries in load order,
//! then Water's builtins. Libraries' initializers run before the program's,
//! dependencies first. `dlopen`/`dlsym` use the same machinery.
use super::*;
use goblin::elf::dynamic::{DT_INIT, DT_INIT_ARRAY, DT_INIT_ARRAYSZ, DT_RPATH, DT_RUNPATH};
use std::{collections::HashMap, sync::Mutex};

/// Libraries Water implements itself. A program's `NEEDED` entry for one of
/// these is satisfied by Water even if the game folder holds a copy.
const BUILTIN: &[&str] = &[
    "ld-linux.so.2",
    "libc.so.6",
    "libm.so.6",
    "libpthread.so.0",
    "libdl.so.2",
    "librt.so.1",
    "libGL.so.1",
    "libGLU.so.1",
    "libglut.so.3",
];
fn builtin_library(name: &str) -> bool {
    let name = name.rsplit('/').next().unwrap_or(name);
    BUILTIN.contains(&name)
}

/// One mapped ELF object.
struct Object {
    /// Soname or file name, for messages and `dlopen` reuse.
    name: String,
    /// Guest path it was loaded from.
    path: String,
    /// Mapped address range.
    start: usize,
    end: usize,
    /// Exported (global and weak) definitions.
    exports: HashMap<String, usize>,
    /// Where its own dependencies (and, for the program, dlopen) are searched.
    origin: Origin,
    /// Initializers (DT_INIT, then DT_INIT_ARRAY), run once.
    init: Vec<usize>,
    initialized: bool,
}

/// Every object in global-scope order: the program first, then libraries
/// in load order.
static OBJECTS: Mutex<Vec<Object>> = Mutex::new(Vec::new());
static ERROR: Mutex<Option<std::ffi::CString>> = Mutex::new(None);

/// Where an object came from, for searching its dependencies.
#[derive(Clone)]
struct Origin {
    directory: String,
    search: Vec<String>,
}

/// Map the program and every library it needs, bind all relocations and
/// return the program's entry point and mapped range.
pub unsafe fn load_program(bytes: &[u8], path: &str) -> Result<(usize, usize, usize), String> {
    let elf = Elf::parse(bytes).map_err(|e| e.to_string())?;
    if elf.is_64 || elf.header.e_machine != 3 || elf.header.e_type != 2 {
        return Err("Expected a fixed-address ELF32/i386 program".into());
    }
    let mut pending = Vec::new();
    let (start, end) = map(&elf, bytes, 0, path, &mut pending)?;
    let entry = elf.entry as usize;
    load_needed(pending)?;
    relocate_all()?;
    Ok((entry, start, end))
}

/// Map one object; queue its `NEEDED` libraries.
unsafe fn map(
    elf: &Elf,
    bytes: &[u8],
    fixed: usize,
    path: &str,
    pending: &mut Vec<(String, Origin)>,
) -> Result<(usize, usize), String> {
    let loads: Vec<_> = elf
        .program_headers
        .iter()
        .filter(|p| p.p_type == PT_LOAD && p.p_memsz > 0)
        .collect();
    // Windows reserves at 64 KiB granularity.
    let low = loads
        .iter()
        .map(|p| p.p_vaddr & !0xffff)
        .min()
        .ok_or("No LOAD segments")? as usize;
    let high = loads
        .iter()
        .map(|p| p.p_vaddr.checked_add(p.p_memsz).ok_or("LOAD overflow"))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .max()
        .unwrap() as usize;
    let span = (high - low + 0xfff) & !0xfff;
    if span > 256 * 1024 * 1024 || (elf.header.e_type == 2 && high > 0x8000_0000) {
        return Err(format!("{path}: unsupported ELF memory extent"));
    }
    // The program at its own address (fixed = 0 bias); libraries anywhere.
    let request = if elf.header.e_type == 2 {
        low as *const c_void
    } else {
        ptr::null()
    };
    let memory = VirtualAlloc(request, span, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE);
    if memory.is_null() || (elf.header.e_type == 2 && memory as usize != low) {
        return Err(format!(
            "Cannot map {path} at {low:#x}: {}",
            std::io::Error::last_os_error()
        ));
    }
    let base = if elf.header.e_type == 2 {
        fixed
    } else {
        memory as usize - low
    };
    for p in &loads {
        if p.p_filesz > p.p_memsz {
            return Err(format!("{path}: invalid LOAD sizes"));
        }
        let data = bytes
            .get(p.p_offset as usize..(p.p_offset + p.p_filesz) as usize)
            .ok_or_else(|| format!("{path}: LOAD exceeds the file"))?;
        ptr::copy_nonoverlapping(
            data.as_ptr(),
            (base + p.p_vaddr as usize) as *mut u8,
            data.len(),
        );
    }
    // Guest code runs in place; legacy builds have writable code segments.
    // The worker is a compatibility boundary, not an untrusted-code sandbox.
    let mut old = 0;
    VirtualProtect(memory, span, PAGE_EXECUTE_READWRITE, &mut old);
    let mut exports = HashMap::new();
    for symbol in elf.dynsyms.iter() {
        if symbol.st_shndx == 0 || !matches!(symbol.st_bind(), 1 | 2) || symbol.st_type() == 3 {
            continue;
        }
        if let Some(name) = elf.dynstrtab.get_at(symbol.st_name) {
            exports
                .entry(name.to_owned())
                .or_insert(base + symbol.st_value as usize);
        }
    }
    let mut init = Vec::new();
    let mut search = Vec::new();
    if let Some(dynamic) = &elf.dynamic {
        let mut array = (0, 0);
        for d in &dynamic.dyns {
            match d.d_tag {
                DT_INIT => init.push(base + d.d_val as usize),
                DT_INIT_ARRAY => array.0 = base + d.d_val as usize,
                DT_INIT_ARRAYSZ => array.1 = d.d_val as usize / 4,
                DT_RPATH | DT_RUNPATH => {
                    if let Some(text) = elf.dynstrtab.get_at(d.d_val as usize) {
                        search.extend(text.split(':').map(str::to_owned));
                    }
                }
                _ => {}
            }
        }
        // INIT_ARRAY entries are read once relocated (see `initializers`).
        if array.0 != 0 {
            init.push(usize::MAX - array.1);
            init.push(array.0);
        }
    }
    let directory = path.rsplit_once('/').map_or("", |(d, _)| d).to_owned();
    for needed in &elf.libraries {
        pending.push((
            needed.to_string(),
            Origin {
                directory: directory.clone(),
                search: search.clone(),
            },
        ));
    }
    let name = elf
        .soname
        .map(str::to_owned)
        .unwrap_or_else(|| path.rsplit('/').next().unwrap_or(path).to_owned());
    OBJECTS.lock().unwrap().push(Object {
        name,
        path: path.to_owned(),
        start: memory as usize,
        end: memory as usize + span,
        exports,
        origin: Origin {
            directory: directory.clone(),
            search: search.clone(),
        },
        init,
        initialized: false,
    });
    RELOCATE
        .lock()
        .unwrap()
        .push((bytes.to_vec(), base, path.to_owned()));
    Ok((memory as usize, memory as usize + span))
}
/// Objects mapped but not yet relocated: (file bytes, bias, path).
static RELOCATE: Mutex<Vec<(Vec<u8>, usize, String)>> = Mutex::new(Vec::new());

/// Load every queued library (breadth first, like `ld.so`), skipping
/// builtins and libraries already loaded.
unsafe fn load_needed(mut pending: Vec<(String, Origin)>) -> Result<(), String> {
    let mut at = 0;
    while at < pending.len() {
        let (name, origin) = (pending[at].0.clone(), &pending[at].1);
        at += 1;
        if builtin_library(&name) || loaded(&name).is_some() {
            continue;
        }
        let Some((path, bytes)) = find(&name, origin) else {
            // Not shipped with the game: Water's own implementation of its
            // functions is used, and any it lacks is reported by name.
            eprintln!("[water] {name} is not in the game; using Water's builtins");
            continue;
        };
        let elf = Elf::parse(&bytes).map_err(|e| format!("{path}: {e}"))?;
        if elf.is_64 || elf.header.e_machine != 3 || elf.header.e_type != 3 {
            return Err(format!("{path} is not an ELF32/i386 shared library"));
        }
        let mut more = Vec::new();
        let (start, _) = map(&elf, &bytes, 0, &path, &mut more)?;
        eprintln!("[water] loaded {path} at {start:#x}");
        pending.extend(more);
    }
    Ok(())
}

fn loaded(name: &str) -> Option<usize> {
    let file = name.rsplit('/').next().unwrap_or(name);
    OBJECTS
        .lock()
        .unwrap()
        .iter()
        .position(|o| o.name == file || o.path == name || o.path.ends_with(&format!("/{file}")))
}

/// Find a library in the view: a path as given, else `LD_LIBRARY_PATH`,
/// the referencing object's RPATH/RUNPATH (`$ORIGIN` expanded), its own
/// directory and the program's directory.
fn find(name: &str, origin: &Origin) -> Option<(String, Vec<u8>)> {
    let view = crate::vfs::view().ok()?;
    let read = |guest: &str| -> Option<(String, Vec<u8>)> {
        let path = crate::vfs::resolve(guest).ok()?;
        let bytes = view.read_all(&path, 256 * 1024 * 1024).ok()?;
        (bytes.get(..4) == Some(b"\x7fELF")).then(|| (guest.to_owned(), bytes))
    };
    if name.contains('/') {
        return read(name);
    }
    let program = ARGV_PROGRAM
        .get()
        .and_then(|p| p.rsplit_once('/').map(|(d, _)| d.to_owned()))
        .unwrap_or_default();
    let mut directories: Vec<String> = std::env::var("LD_LIBRARY_PATH")
        .unwrap_or_default()
        .split(':')
        .filter(|d| !d.is_empty())
        .map(str::to_owned)
        .collect();
    directories.extend(origin.search.iter().map(|d| {
        d.replace("$ORIGIN", &origin.directory)
            .replace("${ORIGIN}", &origin.directory)
    }));
    directories.push(origin.directory.clone());
    directories.push(program);
    directories
        .iter()
        .find_map(|d| read(&format!("{}/{name}", d.trim_end_matches('/'))))
}
/// The program's guest path (`argv[0]`), for library searches.
pub static ARGV_PROGRAM: OnceLock<String> = OnceLock::new();

/// Bind a symbol in global scope: the preload adapter, every object in
/// load order, then Water's builtins.
unsafe fn lookup(name: &str) -> Result<usize, String> {
    if let Some(adapter) = EXTENSION.get() {
        let symbol = CString::new(name).map_err(|e| e.to_string())?;
        let address = (adapter.resolve)(symbol.as_ptr());
        if address != 0 {
            return Ok(address);
        }
    }
    if let Some(address) = OBJECTS
        .lock()
        .unwrap()
        .iter()
        .find_map(|o| o.exports.get(name).copied())
    {
        return Ok(address);
    }
    Ok(builtin(name)?.unwrap_or(0))
}

/// Relocate every mapped object not yet relocated: dependencies before
/// dependents (reverse load order), the program last, as `ld.so` does.
unsafe fn relocate_all() -> Result<(), String> {
    let objects = std::mem::take(&mut *RELOCATE.lock().unwrap());
    let mut unresolved = Vec::new();
    let mut count = 0;
    for (bytes, base, path) in objects.iter().rev() {
        let elf = Elf::parse(bytes).map_err(|e| e.to_string())?;
        count += relocate(&elf, *base, path, &mut unresolved)?;
    }
    if !unresolved.is_empty() {
        unresolved.sort();
        unresolved.dedup();
        return Err(format!(
            "Water does not implement these imports yet: {}",
            unresolved.join(", ")
        ));
    }
    eprintln!("[water] resolved {count} relocations");
    Ok(())
}

unsafe fn relocate(
    elf: &Elf,
    base: usize,
    path: &str,
    unresolved: &mut Vec<String>,
) -> Result<usize, String> {
    let (start, end) = {
        let objects = OBJECTS.lock().unwrap();
        let o = objects
            .iter()
            .find(|o| o.path == path)
            .ok_or("Lost object")?;
        (o.start, o.end)
    };
    let mut count = 0;
    for reloc in elf.dynrels.iter().chain(elf.pltrelocs.iter()) {
        let at = base + reloc.r_offset as usize;
        if at < start || at + 4 > end {
            return Err(format!("{path}: relocation outside its mapping"));
        }
        let at = at as *mut u32;
        if reloc.r_type == 8 {
            // R_386_RELATIVE
            *at = (*at).wrapping_add(base as u32);
            count += 1;
            continue;
        }
        let symbol = elf
            .dynsyms
            .get(reloc.r_sym)
            .ok_or("Bad relocation symbol")?;
        let name = elf
            .dynstrtab
            .get_at(symbol.st_name)
            .ok_or("Bad symbol name")?;
        let address = if reloc.r_sym == 0 {
            0
        } else if symbol.st_bind() == 0 && symbol.st_shndx != 0 {
            base + symbol.st_value as usize // local
        } else if reloc.r_type == 5 {
            // R_386_COPY: the definition in a library, never the program.
            copy_source(name)
        } else {
            traced(name, lookup(name)?)?
        };
        if address == 0 && reloc.r_sym != 0 && symbol.st_bind() != 2 {
            unresolved.push(name.to_owned());
            continue;
        }
        match reloc.r_type {
            1 => *at = (*at).wrapping_add(address as u32), // R_386_32
            2 => *at = (*at).wrapping_add(address as u32).wrapping_sub(at as u32), // R_386_PC32
            5 => {
                if address != 0 {
                    ptr::copy_nonoverlapping(
                        address as *const u8,
                        at.cast::<u8>(),
                        symbol.st_size as usize,
                    );
                }
            }
            6 | 7 => *at = address as u32, // GLOB_DAT, JMP_SLOT
            14..=17 | 35..=43 => {
                return Err(format!(
                    "{path}: thread-local storage ({name}) is not implemented by Water yet"
                ));
            }
            other => {
                return Err(format!(
                    "{path}: unsupported ELF32 relocation {other} for {name}"
                ));
            }
        }
        count += 1;
    }
    Ok(count)
}

/// A copy relocation's source: the first library (not the program) defining it.
fn copy_source(name: &str) -> usize {
    OBJECTS
        .lock()
        .unwrap()
        .iter()
        .skip(1)
        .find_map(|o| o.exports.get(name).copied())
        .unwrap_or(0)
}

/// Initializers of every library not yet initialized, dependencies first.
/// The program's own run from `__libc_start_main`.
pub unsafe fn initialize_libraries() {
    run_initializers(1);
}
unsafe fn run_initializers(first: usize) {
    let functions: Vec<usize> = {
        let mut objects = OBJECTS.lock().unwrap();
        let mut functions = Vec::new();
        for o in objects.iter_mut().skip(first).rev() {
            if !o.initialized {
                o.initialized = true;
                functions.extend(initializers(&o.init));
            }
        }
        functions
    };
    // Run without the lock: initializers may call dlopen.
    for f in functions {
        let f: unsafe extern "C" fn() = std::mem::transmute(f);
        f();
    }
}
/// The program's DT_INIT and INIT_ARRAY (when its C library start-up
/// does not run them itself).
pub unsafe fn program_initializers() -> Vec<usize> {
    let mut objects = OBJECTS.lock().unwrap();
    match objects.first_mut() {
        Some(o) if !o.initialized => {
            o.initialized = true;
            initializers(&o.init)
        }
        _ => Vec::new(),
    }
}
/// Expand an object's initializer list; INIT_ARRAY is encoded as
/// (usize::MAX - count, address) and read now that it is relocated.
unsafe fn initializers(list: &[usize]) -> Vec<usize> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < list.len() {
        if list[i] > usize::MAX - 0x10000 && i + 1 < list.len() {
            let count = usize::MAX - list[i];
            let array = list[i + 1] as *const u32;
            for n in 0..count {
                let f = *array.add(n) as usize;
                if f != 0 && f != usize::MAX && f != 0xffff_ffff {
                    out.push(f);
                }
            }
            i += 2;
        } else {
            out.push(list[i]);
            i += 1;
        }
    }
    out
}

// dlopen family -----------------------------------------------------------

/// `dlopen(NULL)`: the global scope.
const GLOBAL: usize = 0x6c6f_6200;
/// Handle for a library Water implements itself.
const BUILTINS: usize = 0x6c6f_6201;
const HANDLES: usize = 0x6c6f_6300;

fn fail(message: String) -> usize {
    *ERROR.lock().unwrap() = std::ffi::CString::new(message).ok();
    0
}

unsafe extern "C" fn dlopen(name: *const i8, _flags: i32) -> usize {
    if name.is_null() {
        return GLOBAL;
    }
    let name = text(name).to_owned();
    if builtin_library(&name) {
        return BUILTINS;
    }
    if let Some(index) = loaded(&name) {
        return HANDLES + index;
    }
    // Searched like the program's own dependencies (its RPATH/RUNPATH).
    let Some(origin) = OBJECTS.lock().unwrap().first().map(|o| o.origin.clone()) else {
        return fail("dlopen before the program is loaded".into());
    };
    let Some((path, bytes)) = find(&name, &origin) else {
        return fail(format!("{name}: cannot open shared object file"));
    };
    let first = OBJECTS.lock().unwrap().len();
    let result = (|| {
        let elf = Elf::parse(&bytes).map_err(|e| format!("{path}: {e}"))?;
        if elf.is_64 || elf.header.e_machine != 3 || elf.header.e_type != 3 {
            return Err(format!("{path} is not an ELF32/i386 shared library"));
        }
        let mut more = Vec::new();
        map(&elf, &bytes, 0, &path, &mut more)?;
        load_needed(more)?;
        relocate_all()
    })();
    match result {
        Ok(()) => {
            eprintln!("[water] dlopen {path}");
            run_initializers(first);
            HANDLES + first
        }
        Err(e) => fail(e),
    }
}
unsafe extern "C" fn dlsym(handle: usize, name: *const i8) -> usize {
    if name.is_null() {
        return fail("dlsym: no symbol name".into());
    }
    let symbol = text(name);
    let found = match handle {
        BUILTINS => builtin(symbol).ok().flatten(),
        // RTLD_DEFAULT (0), RTLD_NEXT (-1) and dlopen(NULL): global scope.
        0 | GLOBAL | 0xffff_ffff => lookup(symbol).ok().filter(|&a| a != 0),
        h if h >= HANDLES => {
            let own = OBJECTS
                .lock()
                .unwrap()
                .get(h - HANDLES)
                .and_then(|o| o.exports.get(symbol).copied());
            // The lock is released before searching the global scope.
            own.or_else(|| lookup(symbol).ok().filter(|&a| a != 0))
        }
        _ => None,
    };
    found.unwrap_or_else(|| fail(format!("undefined symbol: {symbol}")))
}
unsafe extern "C" fn dlclose(_handle: usize) -> i32 {
    0 // Libraries stay mapped for the process lifetime.
}
unsafe extern "C" fn dlerror() -> *const i8 {
    // The message stays valid until the next dlerror call replaces it.
    static LAST: Mutex<Option<std::ffi::CString>> = Mutex::new(None);
    let message = ERROR.lock().unwrap().take();
    let mut last = LAST.lock().unwrap();
    *last = message;
    last.as_ref().map_or(ptr::null(), |m| m.as_ptr())
}

pub unsafe fn resolve(name: &str) -> Option<usize> {
    Some(match name {
        "dlopen" => dlopen as *const () as usize,
        "dlsym" => dlsym as *const () as usize,
        "dlclose" => dlclose as *const () as usize,
        "dlerror" => dlerror as *const () as usize,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_libraries_are_builtin_and_game_libraries_are_not() {
        assert!(builtin_library("libc.so.6"));
        assert!(builtin_library("/usr/lib/libGL.so.1"));
        assert!(!builtin_library("libtheora.so.0"));
        assert!(!builtin_library("libPhysXLoader.so.1"));
    }

    #[test]
    fn init_arrays_expand_in_order() {
        let array = [0x1000u32, 0, 0x2000, 0xffff_ffff];
        let list = [0x500, usize::MAX - 4, array.as_ptr() as usize];
        assert_eq!(unsafe { initializers(&list) }, [0x500, 0x1000, 0x2000]);
    }
}
