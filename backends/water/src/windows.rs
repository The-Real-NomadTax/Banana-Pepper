//! Direct x86-64 execution with Linux syscall sites trapped before entry.
//! All unsafe host execution is confined to the worker process.
use crate::{
    elf::Image,
    stack,
    vfs::{self, File},
};
use std::{
    collections::{BTreeSet, HashMap},
    ffi::c_void,
    io::{self, Read, SeekFrom, Write},
    ptr,
    sync::atomic::{AtomicPtr, Ordering},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use windows_sys::Win32::{
    Security::Cryptography::{BCRYPT_USE_SYSTEM_PREFERRED_RNG, BCryptGenRandom},
    System::{
        Diagnostics::Debug::{
            AddVectoredExceptionHandler, EXCEPTION_CONTINUE_EXECUTION, EXCEPTION_CONTINUE_SEARCH,
            EXCEPTION_POINTERS, FlushInstructionCache, RemoveVectoredExceptionHandler,
        },
        Memory::{
            MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_EXECUTE_READ, PAGE_NOACCESS, PAGE_READONLY,
            PAGE_READWRITE, VirtualAlloc, VirtualFree, VirtualProtect,
        },
        Threading::{
            ConvertThreadToFiber, CreateFiberEx, ExitProcess, GetCurrentProcess,
            GetCurrentProcessId, GetCurrentThreadId, SwitchToFiber,
        },
    },
};

const EIO: i32 = 5;
const EFAULT: i32 = 14;
const EINVAL: i32 = 22;
const ENOSYS: i32 = 38;
const MAX_IO: usize = 16 * 1024 * 1024;
const MAX_DYNAMIC: usize = 256 * 1024 * 1024;
static STATE: AtomicPtr<State> = AtomicPtr::new(ptr::null_mut());

#[derive(Clone, Copy)]
struct Region {
    start: u64,
    size: u64,
    read: bool,
    write: bool,
    allocation: bool,
}
struct Descriptor {
    file: File,
    flags: u64,
}
struct State {
    plan: Image,
    argv: Vec<String>,
    memory: Vec<Region>,
    files: HashMap<u64, Descriptor>,
    next_fd: u64,
    standard_open: [bool; 3],
    started: Instant,
    unknown: BTreeSet<u64>,
    calls: u64,
    dynamic_bytes: usize,
    brk_base: u64,
    brk_end: u64,
}

fn error(error: io::Error) -> i32 {
    match error.kind() {
        io::ErrorKind::Interrupted => 4,
        io::ErrorKind::WouldBlock => 11,
        _ => vfs::errno(&error),
    }
}
fn protection(bits: u64) -> Result<u32, i32> {
    match bits {
        0 => Ok(PAGE_NOACCESS),
        1 => Ok(PAGE_READONLY),
        2 | 3 => Ok(PAGE_READWRITE),
        _ => Err(13),
    }
}
fn rounded(value: u64) -> Result<u64, i32> {
    if value == 0 || value > MAX_DYNAMIC as u64 {
        return Err(EINVAL);
    }
    value.checked_add(4095).map(|n| n & !4095).ok_or(EINVAL)
}

impl State {
    fn check(&self, address: u64, length: usize, write: bool) -> Result<(), i32> {
        if length == 0 {
            return Ok(());
        }
        if length > MAX_IO {
            return Err(EINVAL);
        }
        let end = address.checked_add(length as u64).ok_or(EFAULT)?;
        if self.memory.iter().any(|r| {
            address >= r.start && end <= r.start + r.size && if write { r.write } else { r.read }
        }) {
            Ok(())
        } else {
            Err(EFAULT)
        }
    }
    fn bytes(&self, address: u64, length: usize) -> Result<Vec<u8>, i32> {
        self.check(address, length, false)?;
        if length == 0 {
            return Ok(Vec::new());
        }
        // SAFETY: the complete range belongs to a readable guest mapping.
        Ok(unsafe { std::slice::from_raw_parts(address as *const u8, length) }.to_vec())
    }
    fn store(&self, address: u64, bytes: &[u8]) -> Result<(), i32> {
        self.check(address, bytes.len(), true)?;
        if !bytes.is_empty() {
            unsafe {
                ptr::copy_nonoverlapping(bytes.as_ptr(), address as *mut u8, bytes.len());
            }
        }
        Ok(())
    }
    fn string(&self, address: u64) -> Result<String, i32> {
        let mut result = Vec::new();
        for offset in 0..4096 {
            let at = address.checked_add(offset).ok_or(EFAULT)?;
            self.check(at, 1, false)?;
            let value = unsafe { *(at as *const u8) };
            if value == 0 {
                return String::from_utf8(result).map_err(|_| EINVAL);
            }
            result.push(value);
        }
        Err(36) // ENAMETOOLONG
    }
    fn allocate(&mut self, length: u64, prot: u64) -> Result<u64, i32> {
        let length = rounded(length)?;
        if self.dynamic_bytes + length as usize > MAX_DYNAMIC {
            return Err(12);
        }
        let flags = protection(prot)?;
        let address = unsafe {
            VirtualAlloc(
                ptr::null(),
                length as usize,
                MEM_RESERVE | MEM_COMMIT,
                flags,
            )
        };
        if address.is_null() {
            return Err(12);
        }
        self.dynamic_bytes += length as usize;
        self.memory.push(Region {
            start: address as u64,
            size: length,
            read: prot & 1 != 0 || prot & 2 != 0,
            write: prot & 2 != 0,
            allocation: true,
        });
        Ok(address as u64)
    }
    fn open(&mut self, name: String, flags: u64) -> Result<u64, i32> {
        // Linux O_ACCMODE, CREAT, EXCL, TRUNC, APPEND, CLOEXEC, LARGEFILE.
        if flags & !(3 | 0x40 | 0x80 | 0x200 | 0x400 | 0x80000 | 0x8000) != 0 || flags & 3 == 3 {
            return Err(EINVAL);
        }
        if self.files.len() >= 4096 {
            return Err(24);
        }
        let file = File::open(&name, vfs::Options::linux(flags)).map_err(error)?;
        if self.files.len() >= 4096 {
            return Err(24);
        }
        let fd = self.next_fd;
        self.next_fd += 1;
        self.files.insert(fd, Descriptor { file, flags });
        Ok(fd)
    }
    fn stat(&self, metadata: banana_wire::Meta, address: u64) -> Result<u64, i32> {
        let mut bytes = [0u8; 144];
        let put64 = |bytes: &mut [u8], offset, value: u64| {
            bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes())
        };
        put64(&mut bytes, 16, 1);
        bytes[24..28].copy_from_slice(
            &(if metadata.directory {
                0o040555u32
            } else {
                0o100644u32
            })
            .to_le_bytes(),
        );
        bytes[28..32].copy_from_slice(&1000u32.to_le_bytes());
        bytes[32..36].copy_from_slice(&1000u32.to_le_bytes());
        put64(&mut bytes, 48, metadata.len);
        put64(&mut bytes, 56, 4096);
        put64(&mut bytes, 64, metadata.len.div_ceil(512));
        // The view keeps no timestamps; report the session start.
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        for offset in [72, 88, 104] {
            put64(&mut bytes, offset, now.as_secs());
            put64(&mut bytes, offset + 8, now.subsec_nanos() as u64);
        }
        self.store(address, &bytes)?;
        Ok(0)
    }
    fn dispatch(&mut self, number: u64, a: [u64; 6]) -> Result<u64, i32> {
        self.calls += 1;
        match number {
            0 => {
                // read
                let length = usize::try_from(a[2]).map_err(|_| EINVAL)?;
                self.check(a[1], length, true)?;
                let mut bytes = vec![0u8; length];
                let count = if a[0] == 0 && self.standard_open[0] {
                    io::stdin().read(&mut bytes).map_err(error)?
                } else {
                    let file = &mut self.files.get_mut(&a[0]).ok_or(9)?.file;
                    file.read(&mut bytes).map_err(error)?
                };
                self.store(a[1], &bytes[..count])?;
                Ok(count as u64)
            }
            1 => {
                // write
                let bytes = self.bytes(a[1], usize::try_from(a[2]).map_err(|_| EINVAL)?)?;
                let count = match a[0] {
                    1 if self.standard_open[1] => io::stdout().write(&bytes).map_err(error)?,
                    2 if self.standard_open[2] => io::stderr().write(&bytes).map_err(error)?,
                    fd => self
                        .files
                        .get_mut(&fd)
                        .ok_or(9)?
                        .file
                        .write(&bytes)
                        .map_err(error)?,
                };
                Ok(count as u64)
            }
            2 => {
                let name = self.string(a[0])?;
                self.open(name, a[1])
            }
            3 => {
                if a[0] < 3 && self.standard_open[a[0] as usize] {
                    self.standard_open[a[0] as usize] = false;
                    return Ok(0);
                }
                self.files.remove(&a[0]).map(|_| 0).ok_or(9)
            }
            5 => {
                let metadata = self.files.get(&a[0]).ok_or(9)?.file.metadata().map_err(error)?;
                self.stat(metadata, a[1])
            }
            8 => {
                let file = &mut self.files.get_mut(&a[0]).ok_or(9)?.file;
                let position = match a[2] {
                    0 => SeekFrom::Start(a[1]),
                    1 => SeekFrom::Current(a[1] as i64),
                    2 => SeekFrom::End(a[1] as i64),
                    _ => return Err(EINVAL),
                };
                file.seek(position).map_err(error)
            }
            9 => {
                // anonymous private mmap, non-executable
                if a[3] & 0x22 != 0x22 || a[3] & !0x20022 != 0 || a[5] != 0 {
                    return Err(95);
                }
                self.allocate(a[1], a[2])
            }
            10 => {
                let size = rounded(a[1])?;
                let flags = protection(a[2])?;
                let region = self
                    .memory
                    .iter_mut()
                    .find(|r| r.allocation && r.start == a[0] && r.size == size)
                    .ok_or(EINVAL)?;
                let mut old = 0;
                if unsafe {
                    VirtualProtect(
                        region.start as *const c_void,
                        size as usize,
                        flags,
                        &mut old,
                    )
                } == 0
                {
                    return Err(13);
                }
                region.read = a[2] & 3 != 0;
                region.write = a[2] & 2 != 0;
                Ok(0)
            }
            11 => {
                let size = rounded(a[1])?;
                let index = self
                    .memory
                    .iter()
                    .position(|r| {
                        r.allocation
                            && r.start == a[0]
                            && r.size == size
                            && r.start != self.brk_base
                    })
                    .ok_or(EINVAL)?;
                if unsafe { VirtualFree(a[0] as *mut c_void, 0, MEM_RELEASE) } == 0 {
                    return Err(EINVAL);
                }
                self.dynamic_bytes -= size as usize;
                self.memory.remove(index);
                Ok(0)
            }
            12 => {
                if self.brk_base == 0 {
                    self.brk_base = self.allocate(MAX_IO as u64, 3)?;
                    self.brk_end = self.brk_base;
                }
                if a[0] >= self.brk_base && a[0] <= self.brk_base + MAX_IO as u64 {
                    self.brk_end = a[0];
                }
                Ok(self.brk_end)
            }
            16 => Err(25), // ioctl: no terminal/device ABI yet
            35 => {
                let bytes = self.bytes(a[0], 16)?;
                let sec = i64::from_le_bytes(bytes[..8].try_into().unwrap());
                let nanos = i64::from_le_bytes(bytes[8..].try_into().unwrap());
                if sec < 0 || !(0..1_000_000_000).contains(&nanos) || sec > 3600 {
                    return Err(EINVAL);
                }
                std::thread::sleep(Duration::new(sec as u64, nanos as u32));
                if a[1] != 0 {
                    self.store(a[1], &[0; 16])?;
                }
                Ok(0)
            }
            39 => Ok(unsafe { GetCurrentProcessId() } as u64),
            60 | 231 => {
                eprintln!(
                    "[watertime] guest exit={} syscalls={} elapsed_us={}",
                    a[0] & 255,
                    self.calls,
                    self.started.elapsed().as_micros()
                );
                unsafe { ExitProcess((a[0] & 255) as u32) }
            }
            63 => {
                let mut bytes = [0u8; 65 * 6];
                for (index, value) in [
                    "Linux",
                    "banana-pepper",
                    "6.0.0-arcade",
                    "native Windows ABI translator",
                    "x86_64",
                    "local",
                ]
                .iter()
                .enumerate()
                {
                    bytes[index * 65..index * 65 + value.len()].copy_from_slice(value.as_bytes());
                }
                self.store(a[0], &bytes)?;
                Ok(0)
            }
            72 => {
                if a[0] >= 3 && !self.files.contains_key(&a[0])
                    || a[0] < 3 && !self.standard_open[a[0] as usize]
                {
                    return Err(9);
                }
                match a[1] {
                    1 => Ok(1),
                    2 => Ok(0),
                    3 => Ok(self
                        .files
                        .get(&a[0])
                        .map(|d| d.flags)
                        .unwrap_or(if a[0] == 0 { 0 } else { 1 })),
                    _ => Err(EINVAL),
                }
            }
            89 | 267 => {
                let (name, buffer, size) = if number == 89 {
                    (a[0], a[1], a[2])
                } else {
                    if a[0] as i64 != -100 {
                        return Err(95);
                    }
                    (a[1], a[2], a[3])
                };
                if self.string(name)? != "/proc/self/exe" {
                    return Err(ENOSYS);
                }
                let bytes = self.argv[0].as_bytes();
                let length = bytes.len().min(usize::try_from(size).map_err(|_| EINVAL)?);
                self.store(buffer, &bytes[..length])?;
                Ok(length as u64)
            }
            96 => {
                if a[1] != 0 {
                    return Err(EINVAL);
                }
                let time = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(|_| EIO)?;
                let mut bytes = Vec::from(time.as_secs().to_le_bytes());
                bytes.extend_from_slice(&(time.subsec_micros() as u64).to_le_bytes());
                if a[0] != 0 {
                    self.store(a[0], &bytes)?;
                }
                Ok(0)
            }
            102 | 104 | 107 | 108 => Ok(1000),
            186 => Ok(unsafe { GetCurrentThreadId() } as u64),
            228 => {
                let time = match a[0] {
                    0 => SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map_err(|_| EIO)?,
                    1 => self.started.elapsed(),
                    _ => return Err(EINVAL),
                };
                let mut bytes = Vec::from(time.as_secs().to_le_bytes());
                bytes.extend_from_slice(&(time.subsec_nanos() as u64).to_le_bytes());
                self.store(a[1], &bytes)?;
                Ok(0)
            }
            257 => {
                if a[0] as i64 != -100 {
                    return Err(95);
                }
                let name = self.string(a[1])?;
                self.open(name, a[2])
            }
            262 => {
                if a[0] as i64 != -100 || a[3] != 0 {
                    return Err(95);
                }
                let metadata = vfs::stat(&self.string(a[1])?).map_err(error)?;
                self.stat(metadata, a[2])
            }
            318 => {
                let length = usize::try_from(a[1]).map_err(|_| EINVAL)?;
                self.check(a[0], length, true)?;
                if a[2] & !7 != 0 {
                    return Err(EINVAL);
                }
                if unsafe {
                    BCryptGenRandom(
                        ptr::null_mut(),
                        a[0] as *mut u8,
                        length as u32,
                        BCRYPT_USE_SYSTEM_PREFERRED_RNG,
                    )
                } < 0
                {
                    return Err(EIO);
                }
                Ok(length as u64)
            }
            _ => {
                if self.unknown.insert(number) {
                    eprintln!("[watertime] unsupported syscall {number}; returning Linux ENOSYS");
                }
                Err(ENOSYS)
            }
        }
    }
}

unsafe extern "system" fn exception(info: *mut EXCEPTION_POINTERS) -> i32 {
    let state = STATE.load(Ordering::Acquire);
    if state.is_null() || info.is_null() {
        return EXCEPTION_CONTINUE_SEARCH;
    }
    let info = unsafe { &*info };
    if info.ExceptionRecord.is_null() || info.ContextRecord.is_null() {
        return EXCEPTION_CONTINUE_SEARCH;
    }
    let context = unsafe { &mut *info.ContextRecord };
    let state = unsafe { &mut *state };
    let code = unsafe { (*info.ExceptionRecord).ExceptionCode };
    if code as u32 == 0xc000001d && state.plan.syscalls.binary_search(&context.Rip).is_ok() {
        let args = [
            context.Rdi,
            context.Rsi,
            context.Rdx,
            context.R10,
            context.R8,
            context.R9,
        ];
        let number = context.Rax;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            state.dispatch(number, args)
        }));
        match result {
            Ok(result) => context.Rax = result.unwrap_or_else(|e| (-i64::from(e)) as u64),
            Err(_) => unsafe { ExitProcess(125) },
        }
        context.Rip += 2;
        context.Rcx = context.Rip;
        context.R11 = context.EFlags as u64;
        return EXCEPTION_CONTINUE_EXECUTION;
    }
    if context.Rip >= state.plan.image_start && context.Rip < state.plan.image_end {
        eprintln!(
            "[watertime] guest fault: Windows exception {:#x} at Linux RIP {:#x}",
            code as u32, context.Rip
        );
        unsafe { ExitProcess(125) }
    }
    EXCEPTION_CONTINUE_SEARCH
}

unsafe extern "system" fn enter(parameter: *mut c_void) {
    let state = unsafe { &mut *(parameter as *mut State) };
    let base: u64;
    let limit: u64;
    let current: u64;
    unsafe {
        std::arch::asm!("mov {base}, gs:[8]","mov {limit}, gs:[16]","mov {current}, rsp",base=out(reg)base,limit=out(reg)limit,current=out(reg)current,options(nostack,readonly));
    }
    state.memory.push(Region {
        start: limit,
        size: base - limit,
        read: true,
        write: true,
        allocation: false,
    });
    let mut random = [0u8; 16];
    if unsafe {
        BCryptGenRandom(
            ptr::null_mut(),
            random.as_mut_ptr(),
            16,
            BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        )
    } < 0
    {
        unsafe { ExitProcess(125) }
    }
    let aux = vec![
        (3, state.plan.phdr),
        (4, 56),
        (5, state.plan.phnum as u64),
        (6, 4096),
        (9, state.plan.entry),
        (11, 1000),
        (12, 1000),
        (13, 1000),
        (14, 1000),
        (23, 0),
    ];
    let stack = match stack::build(
        current - 65536,
        &state.argv,
        &["LANG=C.UTF-8".into()],
        aux,
        random,
    ) {
        Ok(stack) => stack,
        Err(e) => {
            eprintln!("{e}");
            unsafe { ExitProcess(125) }
        }
    };
    if stack.address < limit + 65536 {
        unsafe { ExitProcess(125) }
    }
    unsafe {
        ptr::copy_nonoverlapping(
            stack.bytes.as_ptr(),
            stack.address as *mut u8,
            stack.bytes.len(),
        );
    }
    let entry = state.plan.entry;
    let address = stack.address;
    state.started = Instant::now();
    // Linux _start expects a raw initial stack, not a C function call. The
    // Windows fiber owns this stack and keeps the host TEB limits consistent.
    unsafe {
        std::arch::asm!("mov rsp, r11","xor rbp, rbp","xor rdx, rdx","jmp r10",in("r11")address,in("r10")entry,options(noreturn));
    }
}

pub fn run(bytes: &[u8], plan: &Image, argv: &[String]) -> Result<(), String> {
    if !STATE.load(Ordering::Acquire).is_null() {
        return Err("Only one guest may execute in a worker".into());
    }
    let size = (plan.image_end - plan.image_start) as usize;
    let allocation = unsafe {
        VirtualAlloc(
            plan.image_start as *const c_void,
            size,
            MEM_RESERVE | MEM_COMMIT,
            PAGE_READWRITE,
        )
    };
    if allocation.is_null() {
        return Err(format!(
            "Cannot reserve ELF address range {:#x}..{:#x}: {}",
            plan.image_start,
            plan.image_end,
            io::Error::last_os_error()
        ));
    }
    if allocation as u64 != plan.image_start {
        unsafe {
            VirtualFree(allocation, 0, MEM_RELEASE);
        }
        return Err("Windows did not honor the ELF load address".into());
    }
    let mut state = Box::new(State {
        plan: plan.clone(),
        argv: argv.to_vec(),
        memory: Vec::new(),
        files: HashMap::new(),
        next_fd: 3,
        standard_open: [true; 3],
        started: Instant::now(),
        unknown: BTreeSet::new(),
        calls: 0,
        dynamic_bytes: 0,
        brk_base: 0,
        brk_end: 0,
    });
    for segment in &plan.segments {
        unsafe {
            ptr::copy_nonoverlapping(
                bytes.as_ptr().add(segment.file_offset as usize),
                segment.address as *mut u8,
                segment.file_size as usize,
            );
        }
        state.memory.push(Region {
            start: segment.address,
            size: segment.memory_size,
            read: segment.flags & 5 != 0,
            write: segment.flags & 2 != 0,
            allocation: false,
        });
    }
    // No Linux SYSCALL instruction is allowed to reach the Windows kernel.
    // The static decoder certified each site before this image was mapped.
    for &address in &plan.syscalls {
        unsafe {
            ptr::copy_nonoverlapping([0x0f, 0x0b].as_ptr(), address as *mut u8, 2);
        }
    }
    let mut old = 0;
    if unsafe { VirtualProtect(allocation, size, PAGE_NOACCESS, &mut old) } == 0 {
        return Err(io::Error::last_os_error().to_string());
    }
    for segment in &plan.segments {
        let start = segment.address & !4095;
        let end = (segment.address + segment.memory_size + 4095) & !4095;
        let flags = if segment.flags & 1 != 0 {
            PAGE_EXECUTE_READ
        } else if segment.flags & 2 != 0 {
            PAGE_READWRITE
        } else if segment.flags & 4 != 0 {
            PAGE_READONLY
        } else {
            PAGE_NOACCESS
        };
        if unsafe {
            VirtualProtect(
                start as *const c_void,
                (end - start) as usize,
                flags,
                &mut old,
            )
        } == 0
        {
            return Err(io::Error::last_os_error().to_string());
        }
    }
    if unsafe { FlushInstructionCache(GetCurrentProcess(), allocation, size) } == 0 {
        return Err(io::Error::last_os_error().to_string());
    }
    let handler = unsafe { AddVectoredExceptionHandler(1, Some(exception)) };
    if handler.is_null() {
        return Err("Could not register Linux syscall handler".into());
    }
    let host_fiber = unsafe { ConvertThreadToFiber(ptr::null()) };
    if host_fiber.is_null() {
        unsafe {
            RemoveVectoredExceptionHandler(handler);
        };
        return Err("Could not create host fiber".into());
    }
    let parameter = (&mut *state) as *mut State;
    let guest = unsafe {
        CreateFiberEx(
            4 * 1024 * 1024,
            4 * 1024 * 1024,
            0,
            Some(enter),
            parameter.cast(),
        )
    };
    if guest.is_null() {
        unsafe {
            RemoveVectoredExceptionHandler(handler);
        };
        return Err("Could not create Linux guest stack".into());
    }
    STATE.store(parameter, Ordering::Release);
    unsafe { SwitchToFiber(guest) };
    // Linux exit/exit_group terminate this worker; a returning _start is a bug.
    Err("Linux entry unexpectedly returned".into())
}
