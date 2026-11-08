# banana-tracer

The Rust ptrace tracer every Android guest process runs under. It gives the
guest the runtime image as its root and serves game paths from the session's
view. It replaced PRoot and its view extension, along with their loader,
talloc and libandroid-shmem.

The Android backend runs it as `files/bin/banana-tracer` with PRoot's
command-line spelling: `--kill-on-exit --link2symlink -0 -r ROOT
-b HOST:GUEST… -w DIR -v LEVEL COMMAND…`. `cargo xtask android apk` builds it
and packages it as an APK asset. Its binary carries `BANANA_TRACER_VIEW_PROTOCOL_2`
(`--version` prints it), which the backend's capability check requires. It runs
on Linux AArch64 (Android) and x86-64 (desktop Linux, where its tests run).

**Status: in use by Android sessions. Tested on Linux x86-64; not yet run on a device.**

## Guest root (PRoot's core)

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

## The view and seccomp

- **View protocol 2** (`view.rs`). With `BANANA_VFS_SOCKET` and
  `BANANA_VFS_EMPTY`, the tracer holds one connection to the session's
  `banana-vfs::rootless` server, which is unchanged, and serves the guest paths
  it owns. It ports the former PRoot view extension (`banana_vfs.c`, `banana_fd.h`):
  - **Path calls:** open, creat, the stat family, access, unlink, rmdir, mkdir,
    rename, chmod, utimensat, truncate, chdir and readlink. Links and nodes on
    view paths are refused.
  - **Descriptor handoff:** an opened view file arrives in the tracee as the
    server's cache memfd. The tracer injects socket, connect, recvmsg and close
    on a random abstract socket, and checks the peer's credentials against the
    tracee's thread group. Android denies reopening memfds through `/proc`.
  - **Open descriptions:** offset, mode and close-on-exec are tracked per
    descriptor table, shared by threads, copied by fork and filtered by exec.
    They follow dup, dup2, dup3, F_DUPFD and F_SETFD, and are dropped when
    `/proc` shows the descriptor is no longer the view's memfd.
  - **Descriptor calls:** read, write, pread64, pwrite64, readv and writev go
    through the server. Also lseek, getdents64, fstat, `fstatat(fd, "",
    AT_EMPTY_PATH)`, ftruncate, fchmod, fchdir, fsync, close with a sync, and
    F_GETFL/F_SETFL.
  - **Maps:** mmap hydrates the mapped range, then the kernel maps the memfd.
    msync syncs. mremap of view mappings, preadv/pwritev, fallocate, splice,
    sendfile and copy_file_range are refused, so libc falls back to plain reads
    and writes.
  - **Working directory and exec:** a view cwd (including the initial `-w`)
    keeps the kernel in `BANANA_VFS_EMPTY` while the tracer resolves relative
    paths and answers getcwd. `/proc/*/fd` links name the view file. A dynamic
    view program execs through its guest `ld.so`, which opens it like any view
    file, so no `/proc` reopen is needed. A static view program runs from the
    server's cache path.
- **seccomp-bpf.** The first child installs a filter before its attach stop.
  It traps path, exec and cwd calls, id calls with `-0`, and descriptor calls
  only when a view is attached, so Xvfb and PulseAudio never stop on reads or
  writes. Everything else runs without a stop. The tracer confirms the filter
  through `Seccomp_filters` and otherwise traces every call;
  `BANANA_TRACER_NO_SECCOMP=1` forces that mode. With the filter, a
  one-byte-at-a-time `dd` of 300,000 bytes takes 0.08 s (0.07 s natively,
  33 s with every call stopping).

`tests/view.rs` serves a `MemorySource` view through the real server. Guest
programs then exercise it:
- shell, coreutils and Python run relative paths in a view cwd
- directory listing, and a 200 KB file through redirection, `tail` (lseek) and
  `mmap`
- stat, create, write, mkdir and rename, with the result read back from the
  View's overlay
- cd into and out of the view
- exec of a dynamic ELF stored in the view
- dup sharing an offset, and `/proc/self/fd` links

Every test passes with and without the filter.

## Remaining

1. **Group stops** (`PTRACE_SEIZE` and `PTRACE_LISTEN`). A SIGSTOP or SIGTSTP
   inside the guest is delivered but does not keep the process stopped, as
   under the current loop.
2. **Device validation** on the SM-S721U: Xvfb, PulseAudio, Box64/Box32 and a
   title under the tracer. The AArch64 register, injection and seccomp paths
   are only compiled so far.
## Known differences from PRoot

- Emulated hard links are symbolic links to lstat.
- getsockname and getpeername report host paths.
- openat2's `RESOLVE_*` flags are not applied to the guest walk.
- `/proc/<pid>/maps` names the guest `ld.so` for dynamically linked programs.
