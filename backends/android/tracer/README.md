# banana-tracer

A Rust ptrace tracer that gives Android guests their own root: the job PRoot
does today. It takes PRoot's command line (`--kill-on-exit --link2symlink -0 -r
ROOT -b HOST:GUEST… -w DIR -v LEVEL COMMAND…`), so the backend switch is a change
of executable. It is one Rust executable, with no talloc, libandroid-shmem or
loader beside it. It runs on Linux AArch64 (Android) and
x86-64 (desktop Linux, where its tests run).

**Status: milestone 1 of 2. The Android backend still runs PRoot.**

## Done (milestone 1: PRoot's core)

- Processes and threads: fork, vfork and clone are followed. Exec moves the
  thread to its process id, and `PTRACE_O_EXITKILL` plus `--kill-on-exit` tie
  every tracee to the tracer and the command. The exit status passes through
  (128+signal when killed). Entry and exit stops come from
  `PTRACE_GET_SYSCALL_INFO`. Rewritten argument registers are restored on return.
- Guest paths: the root plus binds (longest guest path first). Symbolic links
  resolve inside the guest, never on the host. `/proc/self` and
  `/proc/thread-self` name the tracee. `/proc/<pid>/` magic links are left to
  the kernel. Relative paths and `*at` directory descriptors resolve against
  the guest view of the kernel's cwd or descriptor.
- Translated calls: open/openat/openat2, creat, the stat family (stat, lstat,
  newfstatat, statx), access/faccessat/faccessat2, mkdir/mknod/unlink/rmdir and
  their `*at` forms, rename/renameat/renameat2, symlink/symlinkat,
  readlink/readlinkat, chmod/fchmodat/fchmodat2, chown/lchown/fchownat,
  utime/utimes/futimesat/utimensat, truncate, chdir, chroot (only to the guest
  root), statfs, the xattr calls, inotify_add_watch, and AF_UNIX path addresses
  for bind and connect.
- Output fixes: getcwd returns guest paths, readlink of `/proc/*/exe` returns
  the guest program, and readlink of other `/proc` links returns guest paths.
- exec: dynamic ELF programs run through their own guest interpreter
  (`ld.so --argv0 NAME PROGRAM ARGS`), `#!` scripts through their guest
  interpreter, and static programs directly. execveat with a path becomes
  execve. `/proc/self/exe` keeps naming the guest program.
- `-0`: id queries return 0, id changes succeed, chown succeeds on existing
  files, and stat results show the tracer's own ids as root.
- `--link2symlink`: link() moves the file to a hidden `.l2s.*` name in its
  directory, and both names become symbolic links to it. Android denies hard
  links; Xvfb's lock file relies on link().

`cargo test -p banana-tracer` (Linux) checks path resolution and the command
line. `tests/guest.rs` runs a shell, coreutils and Python in a guest root
through the tracer: guest-only files, absolute links, cwd and getcwd,
`/proc/self/exe`, fake root, link emulation, AF_UNIX connect, exit status and
kill-on-exit. The AArch64 build links for `aarch64-linux-android` but has not
yet run on a device.

## Remaining before it replaces PRoot (milestone 2)

1. **View protocol 2** (`BANANA_VFS_SOCKET`, served by `banana-vfs::rootless`
   unchanged). This covers the op table (resolve, metadata, open, hydrate,
   sync, truncate, unlink, rename, mkdir, chmod/utime, getdents, guest path,
   read, write, descriptor) for view paths. It also needs:
   - Descriptor handoff into the tracee: injected socket, connect and recvmsg,
     with a peer-credential check.
   - The open-description table across dup, fcntl, fork, exec and close.
   - Reads and writes through the server, lseek, getdents64, fstat, ftruncate,
     fchmod, fchdir and fsync.
   - mmap hydration, msync, and mremap and splice refusal.
   - A virtual cwd (`BANANA_VFS_EMPTY`), `/proc` links of view files, and exec
     of view programs.

   `backends/android/proot/banana_vfs.c` and `banana_fd.h` are the reference.
2. **seccomp-bpf filtering**, so only handled calls stop the tracee. PRoot does
   this, and every call stopping is slow for Box64 guests.
3. **Group stops** (`PTRACE_SEIZE` and `PTRACE_LISTEN`) for SIGSTOP and
   SIGTSTP inside the guest.
4. **Device validation** on the SM-S721U: Xvfb, PulseAudio, Box64/Box32 and a
   title under the tracer.
5. **The switch**:
   - Declare the binary as an Android artifact and have the backend spawn it.
   - Replace the protocol-string check with the tracer's.
   - Delete `backends/android/proot`, the `proot`/`proot-deps` xtask steps, and
     the talloc, libandroid-shmem and PRoot notices and assets.

## Known differences from PRoot

- Emulated hard links are symbolic links to lstat.
- getsockname and getpeername report host paths.
- openat2's `RESOLVE_*` flags are not applied to the guest walk.
- `/proc/<pid>/maps` names the guest `ld.so` for dynamically linked programs.
