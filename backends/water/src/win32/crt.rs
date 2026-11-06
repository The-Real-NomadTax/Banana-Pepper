use super::*;
use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use windows_sys::Win32::System::{LibraryLoader::*, Threading::*};

struct Random {
    words: [u32; 31],
    front: usize,
    rear: usize,
}
impl Random {
    fn next(&mut self) -> u32 {
        let value = self.words[self.front].wrapping_add(self.words[self.rear]);
        self.words[self.front] = value;
        self.front = (self.front + 1) % 31;
        self.rear = (self.rear + 1) % 31;
        (value >> 1) & 0x7fffffff
    }
    fn seeded(seed: u32) -> Self {
        let mut words = [0; 31];
        words[0] = if seed == 0 { 1 } else { seed };
        let mut value = words[0] as i32 as i64;
        for word in words.iter_mut().skip(1) {
            value = (16807 * value) % 2147483647;
            if value < 0 {
                value += 2147483647;
            }
            *word = value as u32;
        }
        let mut result = Self {
            words,
            front: 3,
            rear: 0,
        };
        for _ in 0..310 {
            result.next();
        }
        result
    }
}
static RANDOM: OnceLock<Mutex<Random>> = OnceLock::new();
unsafe extern "C" fn srand(seed: u32) {
    *RANDOM
        .get_or_init(|| Mutex::new(Random::seeded(1)))
        .lock()
        .unwrap() = Random::seeded(seed);
}
unsafe extern "C" fn rand() -> i32 {
    RANDOM
        .get_or_init(|| Mutex::new(Random::seeded(1)))
        .lock()
        .unwrap()
        .next() as i32
}

unsafe fn host(name: &str) -> usize {
    proc(LoadLibraryA(c"msvcrt.dll".as_ptr().cast()), name)
}
pub unsafe fn errno(value: i32) -> i32 {
    let f: unsafe extern "C" fn() -> *mut i32 = std::mem::transmute(host("_errno"));
    *f() = value;
    -1
}
unsafe extern "C" fn errno_location() -> *mut i32 {
    let f: unsafe extern "C" fn() -> *mut i32 = std::mem::transmute(host("_errno"));
    f()
}
unsafe extern "C" fn start_main(
    main: usize,
    argc: i32,
    argv: *const *const i8,
    init: usize,
    _fini: usize,
    _rtld: usize,
    _end: usize,
) -> ! {
    eprintln!("[water] Linux CRT main={main:#x}, argc={argc}");
    if init != 0 {
        eprintln!("[water] calling ELF init {init:#x}");
        let f: unsafe extern "C" fn() = std::mem::transmute(init);
        f();
        eprintln!("[water] ELF init returned");
    }
    let f: unsafe extern "C" fn(i32, *const *const i8, *const *const i8) -> i32 =
        std::mem::transmute(main);
    let env = [ptr::null()];
    let result = f(argc, argv, env.as_ptr());
    ExitProcess(result as u32)
}
unsafe extern "C" fn quit(code: i32) -> ! {
    ExitProcess(code as u32)
}
unsafe extern "C" fn fopen(name: *const i8, mode: *const i8) -> usize {
    let mode = text(mode);
    let write = mode.contains(['w', 'a', '+']);
    let path = match path(text(name), write) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[water] fopen: {e}");
            errno(13);
            return 0;
        }
    };
    if write {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
    }
    let mode = CString::new(if mode.contains('b') {
        mode.to_owned()
    } else {
        format!("{mode}b")
    })
    .unwrap();
    let name = cpath(&path);
    let f: unsafe extern "C" fn(*const i8, *const i8) -> usize = std::mem::transmute(host("fopen"));
    let file = f(name.as_ptr(), mode.as_ptr());
    file
}
unsafe extern "C" fn rename(from: *const i8, to: *const i8) -> i32 {
    let (Ok(from), Ok(to)) = (path(text(from), true), path(text(to), true)) else {
        return errno(13);
    };
    // POSIX rename replaces an existing destination; the legacy Windows CRT
    // rename export does not. Rust's Windows implementation uses replacement.
    match fs::rename(from, to) {
        Ok(()) => 0,
        Err(_) => errno(5),
    }
}
unsafe extern "C" fn remove(name: *const i8) -> i32 {
    let Ok(path) = path(text(name), true) else {
        return errno(13);
    };
    let f: unsafe extern "C" fn(*const i8) -> i32 = std::mem::transmute(host("remove"));
    f(cpath(&path).as_ptr())
}
unsafe extern "C" fn usleep(microseconds: u32) -> i32 {
    std::thread::sleep(Duration::from_micros(microseconds as u64));
    0
}
unsafe extern "C" fn clock_gettime(clock: i32, value: *mut i32) -> i32 {
    static START: OnceLock<Instant> = OnceLock::new();
    if value.is_null() {
        return errno(14);
    }
    let time = match clock {
        0 => SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default(),
        1 | 4 => START.get_or_init(Instant::now).elapsed(),
        _ => return errno(22),
    };
    *value = time.as_secs() as i32;
    *value.add(1) = time.subsec_nanos() as i32;
    0
}
unsafe extern "C" fn sigemptyset(set: *mut u8) -> i32 {
    if set.is_null() {
        return errno(14);
    }
    ptr::write_bytes(set, 0, 128);
    0
}
unsafe extern "C" fn sigaction(_number: i32, _action: usize, old: *mut u8) -> i32 {
    // The game installs crash diagnostics. Windows faults are reported by the
    // worker's VEH; POSIX signal delivery is not emulated.
    if !old.is_null() {
        ptr::write_bytes(old, 0, 140);
    }
    0
}
unsafe extern "C" fn tty_fail(_a: usize, _b: usize, _c: usize) -> i32 {
    errno(25)
}
unsafe extern "C" fn fcntl(_fd: i32, command: i32, _value: usize) -> i32 {
    match command {
        1 | 2 | 3 | 4 => 0,
        _ => errno(22),
    }
}

static MUTEXES: OnceLock<Mutex<HashMap<usize, usize>>> = OnceLock::new();
unsafe fn mutex_address(address: usize) -> usize {
    let mut mutexes = MUTEXES.get_or_init(Default::default).lock().unwrap();
    *mutexes.entry(address).or_insert_with(|| {
        let p = Box::into_raw(Box::new(std::mem::zeroed::<CRITICAL_SECTION>()));
        InitializeCriticalSection(p);
        p as usize
    })
}
unsafe extern "C" fn mutex_init(address: usize, _attributes: usize) -> i32 {
    mutex_address(address);
    0
}
unsafe extern "C" fn mutex_lock(address: usize) -> i32 {
    EnterCriticalSection(mutex_address(address) as *mut CRITICAL_SECTION);
    0
}
unsafe extern "C" fn mutex_unlock(address: usize) -> i32 {
    LeaveCriticalSection(mutex_address(address) as *mut CRITICAL_SECTION);
    0
}
unsafe extern "C" fn mutex_destroy(address: usize) -> i32 {
    if let Some(p) = MUTEXES
        .get_or_init(Default::default)
        .lock()
        .unwrap()
        .remove(&address)
    {
        DeleteCriticalSection(p as *mut CRITICAL_SECTION);
        drop(Box::from_raw(p as *mut CRITICAL_SECTION));
    }
    0
}
unsafe extern "system" fn thread_entry(parameter: *mut c_void) -> u32 {
    let (function, arg) = *Box::from_raw(parameter as *mut (usize, usize));
    let f: unsafe extern "C" fn(usize) -> usize = std::mem::transmute(function);
    f(arg) as u32
}
unsafe extern "C" fn pthread_create(
    thread: *mut u32,
    _attributes: usize,
    function: usize,
    arg: usize,
) -> i32 {
    if thread.is_null() || function == 0 {
        return 22;
    }
    let parameter = Box::into_raw(Box::new((function, arg)));
    let handle = CreateThread(
        ptr::null(),
        4 * 1024 * 1024,
        Some(thread_entry),
        parameter.cast(),
        0,
        ptr::null_mut(),
    );
    if handle.is_null() {
        drop(Box::from_raw(parameter));
        return 11;
    }
    *thread = handle as u32;
    eprintln!("[water] Linux pthread -> Windows thread {handle:p}");
    0
}
unsafe extern "C" fn pthread_cancel(_thread: usize) -> i32 {
    3
} // ESRCH; no unsafe TerminateThread.

#[unsafe(naked)]
unsafe extern "C" fn setjmp(_buffer: *mut u32) -> i32 {
    core::arch::naked_asm!(
        "mov edx,[esp+4]",
        "mov [edx],ebx",
        "mov [edx+4],esi",
        "mov [edx+8],edi",
        "mov [edx+12],ebp",
        "lea eax,[esp+4]",
        "mov [edx+16],eax",
        "mov eax,[esp]",
        "mov [edx+20],eax",
        "xor eax,eax",
        "ret"
    );
}
#[unsafe(naked)]
unsafe extern "C" fn longjmp(_buffer: *mut u32, _value: i32) -> ! {
    core::arch::naked_asm!(
        "mov edx,[esp+4]",
        "mov eax,[esp+8]",
        "test eax,eax",
        "jnz 2f",
        "inc eax",
        "2:",
        "mov ebx,[edx]",
        "mov esi,[edx+4]",
        "mov edi,[edx+8]",
        "mov ebp,[edx+12]",
        "mov esp,[edx+16]",
        "jmp [edx+20]"
    );
}

pub unsafe fn resolve(name: &str) -> Option<usize> {
    Some(match name {
        "__libc_start_main" => start_main as *const () as usize,
        "rand" => rand as *const () as usize,
        "srand" => srand as *const () as usize,
        "__errno_location" => errno_location as *const () as usize,
        "fopen" => fopen as *const () as usize,
        "rename" => rename as *const () as usize,
        "remove" => remove as *const () as usize,
        "usleep" => usleep as *const () as usize,
        "clock_gettime" => clock_gettime as *const () as usize,
        "sigemptyset" => sigemptyset as *const () as usize,
        "sigaction" => sigaction as *const () as usize,
        "fcntl" => fcntl as *const () as usize,
        "tcgetattr" | "tcsetattr" | "tcflush" | "cfsetispeed" | "cfsetospeed" => {
            tty_fail as *const () as usize
        }
        "pthread_mutex_init" => mutex_init as *const () as usize,
        "pthread_mutex_lock" => mutex_lock as *const () as usize,
        "pthread_mutex_unlock" => mutex_unlock as *const () as usize,
        "pthread_mutex_destroy" => mutex_destroy as *const () as usize,
        "pthread_create" => pthread_create as *const () as usize,
        "pthread_cancel" => pthread_cancel as *const () as usize,
        "_setjmp" => setjmp as *const () as usize,
        "longjmp" => longjmp as *const () as usize,
        "exit" => quit as *const () as usize,
        "__strtod_internal" => host("strtod"),
        "__strtol_internal" => host("strtol"),
        "time" => host("_time32"),
        "localtime" => host("_localtime32"),
        "gmtime" => host("_gmtime32"),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn linux_random_sequence_uses_31_bits() {
        let mut r = Random::seeded(1);
        assert_eq!(
            [r.next(), r.next(), r.next()],
            [1804289383, 846930886, 1681692777]
        );
    }
}
