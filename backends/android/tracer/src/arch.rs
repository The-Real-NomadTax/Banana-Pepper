//! Per-architecture tracee registers and system call numbers. AArch64 is the
//! Android target; x86-64 lets the same tracer run its tests on desktop Linux.
use std::io;

/// `struct ptrace_syscall_info` (Linux 5.3+), entry and exit views.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct SyscallInfo {
    op: u8,
    _pad: [u8; 3],
    arch: u32,
    instruction_pointer: u64,
    stack_pointer: u64,
    /// Entry: nr, args[6]. Exit: rval (i64), is_error (u8).
    data: [u64; 7],
}

const PTRACE_GET_SYSCALL_INFO: libc::c_uint = 0x420e;
const INFO_ENTRY: u8 = 1;
const INFO_EXIT: u8 = 2;
const INFO_SECCOMP: u8 = 3;

/// The seccomp audit architecture of this build.
#[cfg(target_arch = "x86_64")]
pub const AUDIT_ARCH: u32 = 0xc000_003e;
#[cfg(target_arch = "aarch64")]
pub const AUDIT_ARCH: u32 = 0xc000_00b7;

/// Where a tracee stopped in a system call.
pub enum Stop {
    Entry {
        nr: i64,
        args: [u64; 6],
    },
    Exit {
        result: i64,
    },
    /// A system-call stop with no syscall information (should not happen).
    Other,
}

pub fn syscall_stop(tid: i32) -> io::Result<Stop> {
    let mut info = SyscallInfo::default();
    // SAFETY: the kernel writes at most `size` bytes into `info`.
    let r = unsafe {
        libc::ptrace(
            PTRACE_GET_SYSCALL_INFO as _,
            tid,
            std::mem::size_of::<SyscallInfo>(),
            &mut info as *mut SyscallInfo,
        )
    };
    if r < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(match info.op {
        // A seccomp stop carries the same entry view (nr, args).
        INFO_ENTRY | INFO_SECCOMP => Stop::Entry {
            nr: info.data[0] as i64,
            args: info.data[1..7].try_into().unwrap(),
        },
        INFO_EXIT => Stop::Exit {
            result: info.data[0] as i64,
        },
        _ => Stop::Other,
    })
}

#[cfg(target_arch = "aarch64")]
mod imp {
    use std::io;
    /// `struct user_pt_regs`.
    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct Regs {
        pub regs: [u64; 31],
        pub sp: u64,
        pub pc: u64,
        pub pstate: u64,
    }
    const NT_PRSTATUS: libc::c_int = 1;
    const NT_ARM_SYSTEM_CALL: libc::c_int = 0x404;

    fn regset<T>(
        tid: i32,
        request: libc::c_uint,
        kind: libc::c_int,
        value: &mut T,
    ) -> io::Result<()> {
        let mut vector = libc::iovec {
            iov_base: value as *mut T as *mut libc::c_void,
            iov_len: std::mem::size_of::<T>(),
        };
        // SAFETY: the iovec describes `value` exactly.
        let r = unsafe { libc::ptrace(request as _, tid, kind as usize, &mut vector) };
        if r < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    impl Regs {
        pub fn read(tid: i32) -> io::Result<Self> {
            let mut regs = Regs {
                regs: [0; 31],
                sp: 0,
                pc: 0,
                pstate: 0,
            };
            regset(tid, libc::PTRACE_GETREGSET as _, NT_PRSTATUS, &mut regs)?;
            Ok(regs)
        }
        pub fn write(&self, tid: i32) -> io::Result<()> {
            let mut copy = *self;
            regset(tid, libc::PTRACE_SETREGSET as _, NT_PRSTATUS, &mut copy)
        }
        pub fn set_arg(&mut self, n: usize, value: u64) {
            self.regs[n] = value;
        }
        pub fn set_result(&mut self, value: i64) {
            self.regs[0] = value as u64;
        }
        pub fn sp(&self) -> u64 {
            self.sp
        }
        /// From a syscall-exit stop: run system call `nr` again at the
        /// same `svc` instruction, with `args`.
        pub fn inject(&mut self, nr: i64, args: &[u64]) {
            self.pc -= 4;
            self.regs[8] = nr as u64;
            for (i, &a) in args.iter().enumerate() {
                self.regs[i] = a;
            }
        }
    }

    /// Replace the system call about to run (-1 skips it).
    pub fn set_syscall(tid: i32, nr: i64) -> io::Result<()> {
        let mut value = nr as i32;
        regset(
            tid,
            libc::PTRACE_SETREGSET as _,
            NT_ARM_SYSTEM_CALL,
            &mut value,
        )
    }
}

#[cfg(target_arch = "x86_64")]
mod imp {
    use std::io;
    /// `struct user_regs_struct`.
    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    pub struct Regs {
        pub r15: u64,
        pub r14: u64,
        pub r13: u64,
        pub r12: u64,
        pub rbp: u64,
        pub rbx: u64,
        pub r11: u64,
        pub r10: u64,
        pub r9: u64,
        pub r8: u64,
        pub rax: u64,
        pub rcx: u64,
        pub rdx: u64,
        pub rsi: u64,
        pub rdi: u64,
        pub orig_rax: u64,
        pub rip: u64,
        pub cs: u64,
        pub eflags: u64,
        pub rsp: u64,
        pub ss: u64,
        pub fs_base: u64,
        pub gs_base: u64,
        pub ds: u64,
        pub es: u64,
        pub fs: u64,
        pub gs: u64,
    }
    impl Regs {
        pub fn read(tid: i32) -> io::Result<Self> {
            let mut regs = Regs::default();
            // SAFETY: GETREGS fills a user_regs_struct.
            let r = unsafe { libc::ptrace(libc::PTRACE_GETREGS as _, tid, 0usize, &mut regs) };
            if r < 0 {
                Err(io::Error::last_os_error())
            } else {
                Ok(regs)
            }
        }
        pub fn write(&self, tid: i32) -> io::Result<()> {
            // SAFETY: SETREGS reads a user_regs_struct.
            let r = unsafe { libc::ptrace(libc::PTRACE_SETREGS as _, tid, 0usize, self) };
            if r < 0 {
                Err(io::Error::last_os_error())
            } else {
                Ok(())
            }
        }
        pub fn set_arg(&mut self, n: usize, value: u64) {
            *[
                &mut self.rdi,
                &mut self.rsi,
                &mut self.rdx,
                &mut self.r10,
                &mut self.r8,
                &mut self.r9,
            ][n] = value;
        }
        pub fn set_result(&mut self, value: i64) {
            self.rax = value as u64;
        }
        pub fn sp(&self) -> u64 {
            self.rsp
        }
        /// From a syscall-exit stop: run system call `nr` again at the
        /// same `syscall` instruction, with `args`.
        pub fn inject(&mut self, nr: i64, args: &[u64]) {
            self.rip -= 2;
            self.rax = nr as u64;
            self.orig_rax = nr as u64;
            for (i, &a) in args.iter().enumerate() {
                self.set_arg(i, a);
            }
        }
    }

    /// Replace the system call about to run (-1 skips it).
    pub fn set_syscall(tid: i32, nr: i64) -> io::Result<()> {
        let mut regs = Regs::read(tid)?;
        regs.orig_rax = nr as u64;
        regs.write(tid)
    }
}

pub use imp::{Regs, set_syscall};

/// System call numbers the tracer handles. Calls an architecture lacks
/// (AArch64 has only the `*at` forms) are -1 and never match.
pub mod nr {
    macro_rules! calls {
        ($($name:ident),* $(,)?) => {
            $(
                #[allow(non_upper_case_globals, dead_code)]
                pub const $name: i64 = calls!(@n $name);
            )*
        };
        (@n $name:ident) => {{
            #[cfg(target_arch = "x86_64")] { x86(stringify!($name)) }
            #[cfg(target_arch = "aarch64")] { arm(stringify!($name)) }
        }};
    }
    /// x86-64 numbers (arch/x86/entry/syscalls/syscall_64.tbl).
    #[allow(dead_code)]
    const fn x86(name: &str) -> i64 {
        match name.as_bytes() {
            b"read" => 0,
            b"lseek" => 8,
            b"mmap" => 9,
            b"munmap" => 11,
            b"pread64" => 17,
            b"pwrite64" => 18,
            b"readv" => 19,
            b"writev" => 20,
            b"mremap" => 25,
            b"msync" => 26,
            b"dup" => 32,
            b"dup2" => 33,
            b"sendfile" => 40,
            b"socket" => 41,
            b"recvmsg" => 47,
            b"fcntl" => 72,
            b"fsync" => 74,
            b"fdatasync" => 75,
            b"ftruncate" => 77,
            b"fchdir" => 81,
            b"fchmod" => 91,
            b"getdents64" => 217,
            b"splice" => 275,
            b"fallocate" => 285,
            b"dup3" => 292,
            b"preadv" => 295,
            b"pwritev" => 296,
            b"copy_file_range" => 326,
            b"preadv2" => 327,
            b"pwritev2" => 328,
            b"write" => 1,
            b"open" => 2,
            b"close" => 3,
            b"stat" => 4,
            b"fstat" => 5,
            b"lstat" => 6,
            b"access" => 21,
            b"execve" => 59,
            b"truncate" => 76,
            b"getcwd" => 79,
            b"chdir" => 80,
            b"rename" => 82,
            b"mkdir" => 83,
            b"rmdir" => 84,
            b"creat" => 85,
            b"link" => 86,
            b"unlink" => 87,
            b"symlink" => 88,
            b"readlink" => 89,
            b"chmod" => 90,
            b"chown" => 92,
            b"lchown" => 94,
            b"fchown" => 93,
            b"utime" => 132,
            b"mknod" => 133,
            b"statfs" => 137,
            b"chroot" => 161,
            b"getuid" => 102,
            b"getgid" => 104,
            b"setuid" => 105,
            b"setgid" => 106,
            b"geteuid" => 107,
            b"getegid" => 108,
            b"setreuid" => 113,
            b"setregid" => 114,
            b"setgroups" => 116,
            b"setresuid" => 117,
            b"getresuid" => 118,
            b"setresgid" => 119,
            b"getresgid" => 120,
            b"setfsuid" => 122,
            b"setfsgid" => 123,
            b"utimes" => 235,
            b"setxattr" => 188,
            b"lsetxattr" => 189,
            b"getxattr" => 191,
            b"lgetxattr" => 192,
            b"listxattr" => 194,
            b"llistxattr" => 195,
            b"removexattr" => 197,
            b"lremovexattr" => 198,
            b"inotify_add_watch" => 254,
            b"openat" => 257,
            b"mkdirat" => 258,
            b"mknodat" => 259,
            b"fchownat" => 260,
            b"futimesat" => 261,
            b"newfstatat" => 262,
            b"unlinkat" => 263,
            b"renameat" => 264,
            b"linkat" => 265,
            b"symlinkat" => 266,
            b"readlinkat" => 267,
            b"fchmodat" => 268,
            b"faccessat" => 269,
            b"utimensat" => 280,
            b"renameat2" => 316,
            b"execveat" => 322,
            b"statx" => 332,
            b"faccessat2" => 439,
            b"fchmodat2" => 452,
            b"bind" => 49,
            b"connect" => 42,
            b"openat2" => 437,
            _ => -1,
        }
    }
    /// AArch64 numbers (include/uapi/asm-generic/unistd.h).
    #[allow(dead_code)]
    const fn arm(name: &str) -> i64 {
        match name.as_bytes() {
            b"read" => 63,
            b"lseek" => 62,
            b"mmap" => 222,
            b"munmap" => 215,
            b"pread64" => 67,
            b"pwrite64" => 68,
            b"readv" => 65,
            b"writev" => 66,
            b"mremap" => 216,
            b"msync" => 227,
            b"dup" => 23,
            b"sendfile" => 71,
            b"socket" => 198,
            b"recvmsg" => 212,
            b"fcntl" => 25,
            b"fsync" => 82,
            b"fdatasync" => 83,
            b"ftruncate" => 46,
            b"fchdir" => 50,
            b"fchmod" => 52,
            b"getdents64" => 61,
            b"splice" => 76,
            b"fallocate" => 47,
            b"dup3" => 24,
            b"preadv" => 69,
            b"pwritev" => 70,
            b"copy_file_range" => 285,
            b"preadv2" => 286,
            b"pwritev2" => 287,
            b"write" => 64,
            b"close" => 57,
            b"fstat" => 80,
            b"execve" => 221,
            b"truncate" => 45,
            b"getcwd" => 17,
            b"chdir" => 49,
            b"chroot" => 51,
            b"fchown" => 55,
            b"statfs" => 43,
            b"getuid" => 174,
            b"geteuid" => 175,
            b"getgid" => 176,
            b"getegid" => 177,
            b"setuid" => 146,
            b"setgid" => 144,
            b"setreuid" => 145,
            b"setregid" => 143,
            b"setgroups" => 159,
            b"setresuid" => 147,
            b"getresuid" => 148,
            b"setresgid" => 149,
            b"getresgid" => 150,
            b"setfsuid" => 151,
            b"setfsgid" => 152,
            b"setxattr" => 5,
            b"lsetxattr" => 6,
            b"getxattr" => 8,
            b"lgetxattr" => 9,
            b"listxattr" => 11,
            b"llistxattr" => 12,
            b"removexattr" => 14,
            b"lremovexattr" => 15,
            b"inotify_add_watch" => 27,
            b"openat" => 56,
            b"mkdirat" => 34,
            b"mknodat" => 33,
            b"fchownat" => 54,
            b"newfstatat" => 79,
            b"unlinkat" => 35,
            b"linkat" => 37,
            b"symlinkat" => 36,
            b"readlinkat" => 78,
            b"fchmodat" => 53,
            b"faccessat" => 48,
            b"utimensat" => 88,
            b"renameat2" => 276,
            b"execveat" => 281,
            b"statx" => 291,
            b"faccessat2" => 439,
            b"fchmodat2" => 452,
            b"bind" => 200,
            b"connect" => 203,
            b"openat2" => 437,
            _ => -1,
        }
    }
    calls!(
        lseek,
        mmap,
        munmap,
        pread64,
        pwrite64,
        readv,
        writev,
        mremap,
        msync,
        dup,
        dup2,
        sendfile,
        socket,
        recvmsg,
        fcntl,
        fsync,
        fdatasync,
        ftruncate,
        fchdir,
        fchmod,
        getdents64,
        splice,
        fallocate,
        dup3,
        preadv,
        pwritev,
        copy_file_range,
        preadv2,
        pwritev2,
        read,
        write,
        open,
        close,
        stat,
        fstat,
        lstat,
        access,
        execve,
        truncate,
        getcwd,
        chdir,
        rename,
        mkdir,
        rmdir,
        creat,
        link,
        unlink,
        symlink,
        readlink,
        chmod,
        chown,
        lchown,
        fchown,
        utime,
        mknod,
        statfs,
        chroot,
        getuid,
        getgid,
        setuid,
        setgid,
        geteuid,
        getegid,
        setreuid,
        setregid,
        setgroups,
        setresuid,
        getresuid,
        setresgid,
        getresgid,
        setfsuid,
        setfsgid,
        utimes,
        setxattr,
        lsetxattr,
        getxattr,
        lgetxattr,
        listxattr,
        llistxattr,
        removexattr,
        lremovexattr,
        inotify_add_watch,
        openat,
        mkdirat,
        mknodat,
        fchownat,
        futimesat,
        newfstatat,
        unlinkat,
        renameat,
        linkat,
        symlinkat,
        readlinkat,
        fchmodat,
        faccessat,
        utimensat,
        renameat2,
        execveat,
        statx,
        faccessat2,
        fchmodat2,
        bind,
        connect,
        openat2,
    );
}

/// `struct stat` uid/gid offsets, for fake-root ownership.
#[cfg(target_arch = "x86_64")]
pub const STAT_UID: usize = 28;
#[cfg(target_arch = "aarch64")]
pub const STAT_UID: usize = 24;
pub const STATX_UID: usize = 20;
