/* PRoot protocol glue; all content/overlay policy lives in banana-vfs (Rust).
 * SPDX-License-Identifier: GPL-2.0-or-later
 * Native 64-bit tracees only: Box64/Wine guest syscalls reach this host ABI. */
#include "execve/execve.h"
#include "extension/extension.h"
#include "path/binding.h"
#include "path/path.h"
#include "syscall/syscall.h"
#include "syscall/sysnum.h"
#include "tracee/mem.h"
#include "tracee/reg.h"
#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <linux/stat.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/uio.h>
#include <sys/un.h>
#include <talloc.h>
#include <time.h>
#include <unistd.h>

const char banana_vfs_protocol[] = "BANANA_VFS_PROTOCOL_2";
struct __attribute__((packed)) Request {
  uint32_t op, flags;
  uint64_t id, offset, length;
  uint32_t a, b;
};
struct __attribute__((packed)) Reply {
  int32_t error;
  uint32_t size;
  uint64_t id, length;
  uint32_t mode, directory;
  uint64_t modified, accessed, created;
};
struct Broker {
  int fd;
  char tag[96];
};
struct State {
  struct Broker *broker;
  int pending;
  long result;
  uint64_t sync;
  struct FileTable *files;
  int stage, listener, transfer_fd, socket_fd, open_mode;
  int dup_source, fd_operation;
  socklen_t address_size;
  word_t scratch;
  uint64_t open_id;
};
static int full(int fd, void *p, size_t n, int writing) {
  while (n) {
    ssize_t c = writing ? write(fd, p, n) : read(fd, p, n);
    if (c < 0 && errno == EINTR)
      continue;
    if (c <= 0)
      return -EIO;
    p = (char *)p + c;
    n -= c;
  }
  return 0;
}
static int close_broker(struct Broker *b) {
  if (b->fd >= 0)
    close(b->fd);
  return 0;
}
static int exchange(struct State *s, struct Request *h, const void *p,
                    const void *q, struct Reply *r, char *body,
                    size_t capacity) {
  if (full(s->broker->fd, h, sizeof(*h), 1) ||
      (h->a && full(s->broker->fd, (void *)p, h->a, 1)) ||
      (h->b && full(s->broker->fd, (void *)q, h->b, 1)) ||
      full(s->broker->fd, r, sizeof(*r), 0))
    return -EIO;
  if (r->size > capacity)
    return -EOVERFLOW;
  if (r->size && full(s->broker->fd, body, r->size, 0))
    return -EIO;
  if (body && r->size < capacity)
    body[r->size] = 0;
  return -r->error;
}
static int rpc(struct State *s, unsigned op, unsigned flags, uint64_t id,
               uint64_t offset, uint64_t len, const char *p, const char *q,
               struct Reply *r, char *body, size_t capacity) {
  struct Request h = {
      op, flags, id, offset, len, p ? strlen(p) : 0, q ? strlen(q) : 0};
  return exchange(s, &h, p, q, r, body, capacity);
}
static int finish(struct State *s, Tracee *t, long result);
#include "banana_fd.h"
static unsigned descriptor_flags(struct State *s, Tracee *t, int fd) {
  struct FileEntry *entry=file_entry(s,fd);
  if(entry) return entry->file->mode;
  char path[80], line[256];
  unsigned flags = O_PATH;
  snprintf(path, sizeof(path), "/proc/%d/fdinfo/%d", t->pid, fd);
  FILE *input = fopen(path, "r");
  if (!input)
    return flags;
  while (fgets(line, sizeof(line), input))
    if (sscanf(line, "flags: %o", &flags) == 1)
      break;
  fclose(input);
  return flags;
}
static uint64_t descriptor(struct State *s, Tracee *t, int fd) {
  struct FileEntry *entry=file_entry(s,fd);
  char p[80], v[256], prefix[128];
  snprintf(p, sizeof(p), "/proc/%d/fd/%d", t->pid, fd);
  ssize_t n = readlink(p, v, sizeof(v) - 1);
  if (n < 0) { forget_file(s,fd); return 0; }
  v[n] = 0;
  snprintf(prefix, sizeof(prefix), "/memfd:banana-vfs-%s-", s->broker->tag);
  if (strncmp(v, prefix, strlen(prefix))) { forget_file(s,fd); return 0; }
  uint64_t id=strtoull(v+strlen(prefix),NULL,10);
  if(entry && entry->file->id!=id) forget_file(s,fd);
  return id;
}
static uint64_t position(struct State *s, Tracee *t, int fd) {
  struct FileEntry *entry=file_entry(s,fd);
  if(entry) return entry->file->offset;
  char p[80], line[256];
  snprintf(p, sizeof(p), "/proc/%d/fdinfo/%d", t->pid, fd);
  FILE *f = fopen(p, "r");
  uint64_t pos = 0;
  if (!f)
    return 0;
  while (fgets(line, sizeof(line), f))
    if (sscanf(line, "pos: %lu", &pos) == 1)
      break;
  fclose(f);
  return pos;
}
static int path(struct State *s, Tracee *t, int dirfd, word_t address,
                char *out) {
  char raw[PATH_MAX], base[PATH_MAX];
  struct Reply r;
  int n = read_path(t, raw, address);
  if (n < 0)
    return n;
  if (raw[0] == '/') {
    strcpy(out, raw);
    return 0;
  }
  if (dirfd == AT_FDCWD) {
    strncpy(base, t->fs->cwd, sizeof(base) - 1);
    base[sizeof(base) - 1] = 0;
  } else {
    uint64_t id = descriptor(s, t, dirfd);
    if (id) {
      int e = rpc(s, 13, 0, id, 0, 0, NULL, NULL, &r, base, sizeof(base));
      if (e)
        return e;
    } else {
      char p[80];
      snprintf(p, sizeof(p), "/proc/%d/fd/%d", t->pid, dirfd);
      ssize_t c = readlink(p, base, sizeof(base) - 1);
      if (c < 0)
        return -errno;
      base[c] = 0;
      if (detranslate_path(t, base, NULL) < 0)
        return -EBADF;
    }
  }
  if (snprintf(out, PATH_MAX, "%s/%s", base, raw) >= PATH_MAX)
    return -ENAMETOOLONG;
  return 0;
}
static int owned(struct State *s, const char *p) {
  struct Reply r;
  char body[PATH_MAX];
  int e = rpc(s, 0, 0, 0, 0, 0, p, NULL, &r, body, sizeof(body));
  return e == -EXDEV ? 0 : (e < 0 ? e : 1);
}
static int finish(struct State *s, Tracee *t, long result) {
  s->pending = 1;
  s->result = result;
  set_sysnum(t, PR_void);
  poke_reg(t, SYSARG_RESULT, result);
  t->sysexit_pending = true;
  t->restart_how = PTRACE_SYSCALL;
  return 1;
}
static void stat_value(const struct Reply *r, struct stat *v) {
  memset(v, 0, sizeof(*v));
  v->st_ino = r->id;
  v->st_mode = (r->directory ? S_IFDIR : S_IFREG) | r->mode;
  v->st_nlink = r->directory ? 2 : 1;
  v->st_size = r->length;
  v->st_blksize = 65536;
  v->st_blocks = (r->length + 511) / 512;
  v->st_mtim.tv_sec = r->modified / 1000000000;
  v->st_mtim.tv_nsec = r->modified % 1000000000;
  v->st_atim.tv_sec = r->accessed / 1000000000;
  v->st_atim.tv_nsec = r->accessed % 1000000000;
  v->st_ctim.tv_sec = r->created / 1000000000;
  v->st_ctim.tv_nsec = r->created % 1000000000;
}
static int metadata(struct State *s, Tracee *t, const char *p, uint64_t id,
                    word_t output, int statx) {
  struct Reply r;
  int e = rpc(s, 1, 0, id, 0, 0, p, NULL, &r, NULL, 0);
  if (e)
    return finish(s, t, e);
  if (statx) {
    struct statx v;
    memset(&v, 0, sizeof(v));
    v.stx_mask = STATX_BASIC_STATS | STATX_BTIME;
    v.stx_ino = r.id;
    v.stx_mode = (r.directory ? S_IFDIR : S_IFREG) | r.mode;
    v.stx_nlink = r.directory ? 2 : 1;
    v.stx_size = r.length;
    v.stx_blksize = 65536;
    v.stx_blocks = (r.length + 511) / 512;
    v.stx_mtime.tv_sec = r.modified / 1000000000;
    v.stx_mtime.tv_nsec = r.modified % 1000000000;
    v.stx_atime.tv_sec = r.accessed / 1000000000;
    v.stx_atime.tv_nsec = r.accessed % 1000000000;
    v.stx_btime.tv_sec = r.created / 1000000000;
    v.stx_btime.tv_nsec = r.created % 1000000000;
    e = write_data(t, output, &v, sizeof(v));
  } else {
    struct stat v;
    stat_value(&r, &v);
    e = write_data(t, output, &v, sizeof(v));
  }
  return finish(s, t, e < 0 ? e : 0);
}
static int enter(struct State *s, Tracee *t) {
  Sysnum nr = get_sysnum(t, ORIGINAL);
  word_t a = peek_reg(t, ORIGINAL, SYSARG_1),
         b = peek_reg(t, ORIGINAL, SYSARG_2),
         c = peek_reg(t, ORIGINAL, SYSARG_3),
         d = peek_reg(t, ORIGINAL, SYSARG_4),
         e = peek_reg(t, ORIGINAL, SYSARG_5),
         f = peek_reg(t, ORIGINAL, SYSARG_6);
  uint64_t id = 0;
  char p[PATH_MAX], q[PATH_MAX], body[65536];
  struct Reply r;
  int status, dir = AT_FDCWD;
  Reg arg = SYSARG_1;
  unsigned flags = 0;
  s->pending = 0;
  s->sync = 0;
  s->fd_operation=0;
  if(nr==PR_dup || nr==PR_dup2 || nr==PR_dup3 ||
     ((nr==PR_fcntl || nr==PR_fcntl64) && (b==F_DUPFD || b==F_DUPFD_CLOEXEC))) {
    descriptor(s,t,(int)a); // Discard stale entries after exec or close_range.
    s->dup_source=a; s->fd_operation=1;
    s->result=(nr==PR_dup3 && (c&O_CLOEXEC)) ||
              ((nr==PR_fcntl || nr==PR_fcntl64) && b==F_DUPFD_CLOEXEC);
    return 0;
  }
  if((nr==PR_fcntl || nr==PR_fcntl64) && descriptor(s,t,(int)a)) {
    struct FileEntry *entry=file_entry(s,(int)a);
    if(!entry) return finish(s,t,-EBADF);
    if(b==F_GETFL) return finish(s,t,entry->file->mode);
    if(b==F_SETFL) {
      unsigned mask=O_APPEND|O_NONBLOCK|O_ASYNC|O_DIRECT|O_NOATIME;
      entry->file->mode=(entry->file->mode&~mask)|(c&mask);
      return finish(s,t,0);
    }
    if(b==F_SETFD) {
      s->dup_source=a; s->fd_operation=3; s->result=!!(c&FD_CLOEXEC);
    }
    return 0;
  }
  if (nr == PR_mremap) {
    char file[80], line[PATH_MAX + 256], prefix[128];
    snprintf(file, sizeof(file), "/proc/%d/maps", t->pid);
    snprintf(prefix, sizeof(prefix), "memfd:banana-vfs-%s-", s->broker->tag);
    FILE *maps = fopen(file, "r");
    if (!maps)
      return -EACCES;
    int virtual = 0;
    while (fgets(line, sizeof(line), maps)) {
      unsigned long start, end;
      if (sscanf(line, "%lx-%lx", &start, &end) == 2 && a >= start && a < end &&
          strstr(line, prefix)) {
        virtual = 1;
        break;
      }
    }
    fclose(maps);
    return virtual ? finish(s, t, -ENOTSUP) : 0;
  }
  if (nr == PR_msync) {
    status = rpc(s, 4, 0, 0, 0, 0, NULL, NULL, &r, NULL, 0);
    return status;
  }
  if (nr == PR_copy_file_range || nr == PR_sendfile || nr == PR_sendfile64) {
    int source_fd = nr == PR_copy_file_range ? (int)a : (int)b;
    int target_fd = nr == PR_copy_file_range ? (int)c : (int)a;
    uint64_t source = descriptor(s, t, source_fd),
             target = descriptor(s, t, target_fd);
    if (!source && !target)
      return 0;
    // A shared transferred cache descriptor has no kernel-side per-open offset.
    // Let libc's copy loop fall back to the ordinary virtual read/write path.
    return finish(s,t,nr==PR_copy_file_range ? -EXDEV : -EOPNOTSUPP);
  }
  if (nr == PR_splice) {
    if (descriptor(s, t, (int)a) || descriptor(s, t, (int)c))
      return finish(s, t, -EOPNOTSUPP);
    return 0;
  }
  if (nr == PR_preadv || nr == PR_preadv2 || nr == PR_pwritev ||
      nr == PR_pwritev2) {
    if (descriptor(s, t, (int)a))
      return finish(s, t, -EOPNOTSUPP);
    return 0;
  }

  switch (nr) {
  case PR_open:
  case PR_creat:
  case PR_stat:
  case PR_lstat:
  case PR_access:
  case PR_unlink:
  case PR_rmdir:
  case PR_mkdir:
  case PR_rename:
  case PR_chmod:
  case PR_truncate:
  case PR_readlink:
  case PR_chdir:
  case PR_symlink:
  case PR_link:
  case PR_mknod:
    break;
  case PR_openat:
  case PR_openat2:
  case PR_newfstatat:
  case PR_statx:
  case PR_faccessat:
  case PR_faccessat2:
  case PR_unlinkat:
  case PR_mkdirat:
  case PR_renameat:
  case PR_renameat2:
  case PR_fchmodat:
  case PR_utimensat:
  case PR_readlinkat:
  case PR_symlinkat:
  case PR_linkat:
  case PR_mknodat:
    dir = (int)a;
    arg = SYSARG_2;
    break;
  default:
    goto descriptor_call;
  }
  if ((nr == PR_newfstatat || nr == PR_statx) && dir != AT_FDCWD &&
      ((nr == PR_statx ? c : d) & AT_EMPTY_PATH)) {
    char empty[PATH_MAX];
    if (read_path(t, empty, peek_reg(t, ORIGINAL, arg)) < 0)
      return -EFAULT;
    if (!empty[0] && (id = descriptor(s, t, dir)))
      return metadata(s, t, NULL, id, nr == PR_statx ? e : c, nr == PR_statx);
  }
  status = path(s, t, dir, peek_reg(t, ORIGINAL, arg), p);
  if (status < 0)
    return status;
  if (!p[0] && (nr == PR_newfstatat || nr == PR_statx)) {
    id = descriptor(s, t, dir);
    if (id)
      return metadata(s, t, NULL, id, nr == PR_statx ? e : c, nr == PR_statx);
  }
  status = owned(s, p);
  if (status <= 0)
    return status;
  switch (nr) {
  case PR_open:
  case PR_openat:
  case PR_creat: {
    int mode = nr == PR_creat ? (O_WRONLY | O_CREAT | O_TRUNC)
                              : (int)(nr == PR_open ? b : c);
    flags = ((mode & O_ACCMODE) != O_WRONLY ? 1 : 0) |
            ((mode & O_ACCMODE) != O_RDONLY ? 2 : 0) |
            ((mode & O_CREAT) ? 4 : 0) | ((mode & O_EXCL) ? 8 : 0) |
            ((mode & O_TRUNC) ? 16 : 0) | ((mode & O_DIRECTORY) ? 32 : 0);
    if (mode & O_PATH)
      flags = 1 | ((mode & O_DIRECTORY) ? 32 : 0);
    status = rpc(s, 2, flags, 0, 0, 0, p, NULL, &r, body, sizeof(body));
    if (status)
      return finish(s, t, status);
    status = begin_transfer(s,t,r.id,mode);
    return status<0 ? finish(s,t,status) : status;
  }
  case PR_openat2:
    return finish(s, t, -ENOTSUP);
  case PR_stat:
  case PR_lstat:
    return metadata(s, t, p, 0, b, 0);
  case PR_newfstatat:
    return metadata(s, t, p, 0, c, 0);
  case PR_statx:
    return metadata(s, t, p, 0, e, 1);
  case PR_access:
  case PR_faccessat:
  case PR_faccessat2:
    status = rpc(s, 1, 0, 0, 0, 0, p, NULL, &r, NULL, 0);
    return finish(s, t, status);
  case PR_unlink:
  case PR_rmdir:
  case PR_unlinkat:
    status =
        rpc(s, 6, nr == PR_rmdir || (nr == PR_unlinkat && (c & AT_REMOVEDIR)),
            0, 0, 0, p, NULL, &r, NULL, 0);
    return finish(s, t, status);
  case PR_mkdir:
  case PR_mkdirat:
    status = rpc(s, 8, 0, 0, 0, 0, p, NULL, &r, NULL, 0);
    return finish(s, t, status);
  case PR_rename:
  case PR_renameat:
  case PR_renameat2:
    status = path(s, t, nr == PR_rename ? AT_FDCWD : (int)c,
                  nr == PR_rename ? b : d, q);
    if (status < 0)
      return status;
    if (nr == PR_renameat2 && (e & ~1))
      return finish(s, t, -ENOTSUP);
    status =
        rpc(s, 7, nr == PR_renameat2 ? (e & 1) : 0, 0, 0, 0, p, q, &r, NULL, 0);
    return finish(s, t, status);
  case PR_chmod:
  case PR_fchmodat:
    status = rpc(s, 9, 0x80000000 | (nr == PR_chmod ? b : c), 0, UINT64_MAX,
                 UINT64_MAX, p, NULL, &r, NULL, 0);
    return finish(s, t, status);
  case PR_utimensat: {
    struct timespec times[2];
    uint64_t at, mt;
    struct timespec now;
    clock_gettime(CLOCK_REALTIME, &now);
    at = mt = (uint64_t)now.tv_sec * 1000000000 + now.tv_nsec;
    if (c) {
      if (read_data(t, times, c, sizeof(times)) < 0)
        return -EFAULT;
      at = times[0].tv_nsec == UTIME_OMIT ? UINT64_MAX
           : times[0].tv_nsec == UTIME_NOW
               ? at
               : (uint64_t)times[0].tv_sec * 1000000000 + times[0].tv_nsec;
      mt = times[1].tv_nsec == UTIME_OMIT ? UINT64_MAX
           : times[1].tv_nsec == UTIME_NOW
               ? mt
               : (uint64_t)times[1].tv_sec * 1000000000 + times[1].tv_nsec;
    }
    status = rpc(s, 9, 0, 0, mt, at, p, NULL, &r, NULL, 0);
    return finish(s, t, status);
  }
  case PR_truncate:
    status = rpc(s, 5, 0, 0, 0, b, p, NULL, &r, NULL, 0);
    return finish(s, t, status);
  case PR_chdir:
    status = rpc(s, 1, 0, 0, 0, 0, p, NULL, &r, NULL, 0);
    if (!status && !r.directory)
      status = -ENOTDIR;
    if (!status) {
      TALLOC_FREE(t->fs->cwd);
      t->fs->cwd = talloc_strdup(t->fs, p);
    }
    return finish(s, t, status);
  case PR_readlink:
  case PR_readlinkat:
    return finish(s, t, -EINVAL);
  default:
    return finish(s, t, -ENOTSUP);
  }
descriptor_call:
  switch (nr) {
  case PR_read:
  case PR_write:
  case PR_pread64:
  case PR_pwrite64:
  case PR_readv:
  case PR_writev:
  case PR_fstat:
  case PR_ftruncate:
  case PR_fsync:
  case PR_fdatasync:
  case PR_fchmod:
  case PR_fchdir:
  case PR_getdents64:
  case PR_fallocate:
  case PR_close:
  case PR_lseek:
    id = descriptor(s, t, (int)a);
    break;
  case PR_mmap:
  case PR_mmap2:
    id = descriptor(s, t, (int)e);
    break;
  default:
    return 0;
  }
  if (!id)
    return 0;
  switch (nr) {
  case PR_fstat:
    return metadata(s, t, NULL, id, b, 0);
  case PR_ftruncate:
    if ((descriptor_flags(s, t, (int)a) & (O_ACCMODE | O_PATH)) == O_RDONLY ||
        (descriptor_flags(s, t, (int)a) & O_PATH))
      return finish(s, t, -EBADF);
    status = rpc(s, 5, 0, id, 0, b, NULL, NULL, &r, NULL, 0);
    return finish(s, t, status);
  case PR_fchmod:
    if ((descriptor_flags(s, t, (int)a) & O_ACCMODE) == O_RDONLY)
      return finish(s, t, -EROFS);
    status = rpc(s, 9, 0x80000000 | b, id, UINT64_MAX, UINT64_MAX, NULL, NULL,
                 &r, NULL, 0);
    return finish(s, t, status);
  case PR_fchdir:
    status = rpc(s, 13, 0, id, 0, 0, NULL, NULL, &r, body, sizeof(body));
    if (status)
      return finish(s, t, status);
    status = rpc(s, 1, 0, id, 0, 0, NULL, NULL, &r, NULL, 0);
    if (!status && !r.directory)
      status = -ENOTDIR;
    if (!status) {
      TALLOC_FREE(t->fs->cwd);
      t->fs->cwd = talloc_strdup(t->fs, body);
    }
    return finish(s, t, status);
  case PR_getdents64: {
    uint64_t off = position(s, t, (int)a);
    status = rpc(s, 12, 0, id, off, c, NULL, NULL, &r, body, sizeof(body));
    if (status)
      return finish(s, t, status);
    status = write_data(t, b, body, r.size);
    if (status < 0)
      return status;
    struct FileEntry *entry=file_entry(s,(int)a);
    if(!entry) return finish(s,t,-EBADF);
    entry->file->offset=r.length;
    return finish(s,t,r.size);
  }
  case PR_fallocate:
    return finish(s, t, -EOPNOTSUPP);
  case PR_fsync:
  case PR_fdatasync:
    status = rpc(s, 4, 0, id, 0, 0, NULL, NULL, &r, NULL, 0);
    return finish(s, t, status);
  case PR_close:
    status = rpc(s, 4, 0, id, 0, 0, NULL, NULL, &r, NULL, 0);
    if (status)
      return finish(s, t, status);
    s->fd_operation=2; s->dup_source=a;
    return 0;
  case PR_lseek: {
    struct FileEntry *entry=file_entry(s,(int)a);
    if(!entry || (entry->file->mode&O_PATH)) return finish(s,t,-EBADF);
    int64_t base=0, next;
    if(c==SEEK_CUR) base=entry->file->offset;
    else if(c==SEEK_END) {
      status=rpc(s,1,0,id,0,0,NULL,NULL,&r,NULL,0);
      if(status) return finish(s,t,status);
      base=r.length;
    } else if(c!=SEEK_SET) return finish(s,t,-EINVAL);
    if(__builtin_add_overflow(base,(int64_t)b,&next) || next<0)
      return finish(s,t,-EINVAL);
    entry->file->offset=next;
    return finish(s,t,next);
  }
  default: {
    if (nr == PR_mmap || nr == PR_mmap2) {
      unsigned mode=descriptor_flags(s,t,(int)e);
      if((mode&O_PATH) || (mode&O_ACCMODE)==O_WRONLY ||
         ((d&MAP_SHARED) && (c&PROT_WRITE) && (mode&O_ACCMODE)==O_RDONLY))
        return finish(s,t,-EACCES);
      status = rpc(s, 3, (d & MAP_SHARED) ? 2 : 0, id,
                   nr == PR_mmap2 ? f * 4096 : f, b, NULL, NULL, &r, NULL, 0);
      if (status)
        return finish(s, t, status);
      t->sysexit_pending = true;
      t->restart_how = PTRACE_SYSCALL;
      return 0;
    }
    unsigned writing = nr == PR_write || nr == PR_pwrite64 || nr == PR_writev;
    unsigned mode = descriptor_flags(s, t, (int)a);
    if ((mode & O_PATH) || (writing ? (mode & O_ACCMODE) == O_RDONLY
                                    : (mode & O_ACCMODE) == O_WRONLY))
      return finish(s, t, -EBADF);
    uint64_t offset =
        nr == PR_pread64 || nr == PR_pwrite64 ? d : position(s, t, (int)a);
    if (writing && (mode & O_APPEND) && nr != PR_pwrite64) {
      status = rpc(s, 1, 0, id, 0, 0, NULL, NULL, &r, NULL, 0);
      if (status)
        return finish(s, t, status);
      offset = r.length;
    }
    struct iovec vectors[1024];
    size_t count = 1;
    if (nr == PR_readv || nr == PR_writev) {
      if (c > 1024)
        return finish(s, t, -EINVAL);
      count = c;
      if (read_data(t, vectors, b, count * sizeof(*vectors)) < 0)
        return -EFAULT;
    } else {
      vectors[0] = (struct iovec){(void *)b, c};
    }
    size_t length = 0;
    for (size_t i = 0; i < count; i++) {
      size_t n = vectors[i].iov_len;
      if (n > sizeof(body) - length)
        n = sizeof(body) - length;
      length += n;
      if (length == sizeof(body))
        break;
    }
    if (writing) {
      size_t done = 0;
      for (size_t i = 0; i < count && done < length; i++) {
        size_t n = vectors[i].iov_len;
        if (n > length - done)
          n = length - done;
        if (read_data(t, body + done, (word_t)vectors[i].iov_base, n) < 0)
          return -EFAULT;
        done += n;
      }
      struct Request h = {16, 0, id, offset, length, length, 0};
      status = exchange(s, &h, body, NULL, &r, NULL, 0);
    } else {
      status =
          rpc(s, 15, 0, id, offset, length, NULL, NULL, &r, body, sizeof(body));
      if (!status) {
        size_t done = 0;
        for (size_t i = 0; i < count && done < r.size; i++) {
          size_t n = vectors[i].iov_len;
          if (n > r.size - done)
            n = r.size - done;
          if (write_data(t, (word_t)vectors[i].iov_base, body + done, n) < 0) {
            if (!done)
              return -EFAULT;
            break;
          }
          done += n;
        }
        r.length = done;
      }
    }
    if (status)
      return finish(s, t, status);
    if (nr == PR_pread64 || nr == PR_pwrite64)
      return finish(s, t, r.length);
    struct FileEntry *entry=file_entry(s,(int)a);
    if(!entry) return finish(s,t,-EBADF);
    entry->file->offset=offset+r.length;
    return finish(s,t,r.length);
  }
  }
}
int banana_vfs_callback(Extension *x, ExtensionEvent event, intptr_t d1,
                        intptr_t d2) {
  struct State *s = x->config;
  Tracee *t = TRACEE(x);
  switch (event) {
  case INITIALIZATION: {
    const char *endpoint = getenv("BANANA_VFS_SOCKET");
    if (!endpoint)
      return -EINVAL;
    s = talloc_zero(x, struct State);
    x->config = s;
    s->listener=s->transfer_fd=s->socket_fd=-1;
    talloc_set_destructor(s,transfer_cleanup);
    s->files=talloc_zero(s,struct FileTable);
    if(!s->files) return -ENOMEM;
    s->broker = talloc_zero(s, struct Broker);
    s->broker->fd = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
    if (s->broker->fd < 0)
      return -errno;
    struct sockaddr_un addr = {.sun_family = AF_UNIX};
    if (strlen(endpoint) >= sizeof(addr.sun_path))
      return -ENAMETOOLONG;
    strcpy(addr.sun_path, endpoint);
    if (connect(s->broker->fd, (void *)&addr, sizeof(addr)))
      return -errno;
    talloc_set_destructor(s->broker, close_broker);
    struct Reply r;
    int e = rpc(s, 11, 0, 0, 0, 0, NULL, NULL, &r, s->broker->tag,
                sizeof(s->broker->tag));
    if (e || r.length != 2)
      return e ? e : -EPROTO;
    static const FilteredSysnum calls[] = {
        {PR_dup, FILTER_SYSEXIT}, {PR_dup2, FILTER_SYSEXIT},
        {PR_dup3, FILTER_SYSEXIT}, {PR_fcntl, FILTER_SYSEXIT},
        {PR_fcntl64, FILTER_SYSEXIT},
        {PR_mremap, FILTER_SYSEXIT},     {PR_munmap, FILTER_SYSEXIT},
        {PR_msync, FILTER_SYSEXIT},      {PR_copy_file_range, FILTER_SYSEXIT},
        {PR_sendfile, FILTER_SYSEXIT},   {PR_sendfile64, FILTER_SYSEXIT},
        {PR_splice, FILTER_SYSEXIT},     {PR_preadv, FILTER_SYSEXIT},
        {PR_preadv2, FILTER_SYSEXIT},    {PR_pwritev, FILTER_SYSEXIT},
        {PR_pwritev2, FILTER_SYSEXIT},   {PR_open, FILTER_SYSEXIT},
        {PR_openat, FILTER_SYSEXIT},     {PR_openat2, FILTER_SYSEXIT},
        {PR_creat, FILTER_SYSEXIT},      {PR_stat, FILTER_SYSEXIT},
        {PR_lstat, FILTER_SYSEXIT},      {PR_fstat, FILTER_SYSEXIT},
        {PR_newfstatat, FILTER_SYSEXIT}, {PR_statx, FILTER_SYSEXIT},
        {PR_access, FILTER_SYSEXIT},     {PR_faccessat, FILTER_SYSEXIT},
        {PR_faccessat2, FILTER_SYSEXIT}, {PR_read, FILTER_SYSEXIT},
        {PR_write, FILTER_SYSEXIT},      {PR_pread64, FILTER_SYSEXIT},
        {PR_pwrite64, FILTER_SYSEXIT},   {PR_readv, FILTER_SYSEXIT},
        {PR_writev, FILTER_SYSEXIT},     {PR_mmap, FILTER_SYSEXIT},
        {PR_mmap2, FILTER_SYSEXIT},      {PR_close, FILTER_SYSEXIT},
        {PR_lseek, FILTER_SYSEXIT},      {PR_ftruncate, FILTER_SYSEXIT},
        {PR_truncate, FILTER_SYSEXIT},   {PR_fsync, FILTER_SYSEXIT},
        {PR_fdatasync, FILTER_SYSEXIT},  {PR_getdents64, FILTER_SYSEXIT},
        {PR_unlink, FILTER_SYSEXIT},     {PR_unlinkat, FILTER_SYSEXIT},
        {PR_rmdir, FILTER_SYSEXIT},      {PR_mkdir, FILTER_SYSEXIT},
        {PR_mkdirat, FILTER_SYSEXIT},    {PR_rename, FILTER_SYSEXIT},
        {PR_renameat, FILTER_SYSEXIT},   {PR_renameat2, FILTER_SYSEXIT},
        {PR_chmod, FILTER_SYSEXIT},      {PR_fchmod, FILTER_SYSEXIT},
        {PR_fchmodat, FILTER_SYSEXIT},   {PR_utimensat, FILTER_SYSEXIT},
        {PR_chdir, FILTER_SYSEXIT},      {PR_fchdir, FILTER_SYSEXIT},
        {PR_fallocate, FILTER_SYSEXIT},  {PR_readlink, FILTER_SYSEXIT},
        {PR_readlinkat, FILTER_SYSEXIT}, {PR_link, FILTER_SYSEXIT},
        {PR_linkat, FILTER_SYSEXIT},     {PR_symlink, FILTER_SYSEXIT},
        {PR_symlinkat, FILTER_SYSEXIT},  {PR_mknod, FILTER_SYSEXIT},
        {PR_mknodat, FILTER_SYSEXIT},    FILTERED_SYSNUM_END};
    x->filtered_sysnums = calls;
    return 0;
  }
  case INHERIT_PARENT:
    return 1;
  case INHERIT_CHILD: {
    struct State *parent = ((Extension *)d1)->config;
    s = talloc_zero(x, struct State);
    s->broker = talloc_reference(s, parent->broker);
    s->listener=s->transfer_fd=s->socket_fd=-1;
    talloc_set_destructor(s,transfer_cleanup);
    s->files=(d2 & CLONE_FILES) ? talloc_reference(s,parent->files)
                              : copy_files(s,parent->files,0);
    if(!s->files) return -ENOMEM;
    x->config = s;
    return 0;
  }
  case SYSCALL_ENTER_START:
    return enter(s, t);
  case SYSCALL_CHAINED_EXIT:
    return s->stage ? transfer_step(s,t) : 0;
  case SYSCALL_EXIT_START:
    if(s->stage) return transfer_step(s,t);
    if(s->fd_operation) {
      long result=(long)peek_reg(t,CURRENT,SYSARG_RESULT);
      int operation=s->fd_operation;
      s->fd_operation=0;
      if(result>=0) {
        struct FileEntry *entry=file_entry(s,s->dup_source);
        if(operation==1) {
          if(entry && result!=s->dup_source) {
            int e=remember_file(s,result,entry->file,s->result);
            if(e) return e;
          } else if(!entry) forget_file(s,result);
        } else if(operation==2) forget_file(s,s->dup_source);
        else if(operation==3 && entry) entry->cloexec=s->result;
      }
    }
    if (s->pending) {
      poke_reg(t, SYSARG_RESULT, s->result);
      s->pending = 0;
      return 1;
    }
    if (s->sync) {
      struct Reply r;
      int e = rpc(s, 4, 0, s->sync, 0, 0, NULL, NULL, &r, NULL, 0);
      s->sync = 0;
      if (e)
        return e;
    }
    return 0;
  case EXECVE_PROC_EXE: {
    struct FileTable *old=s->files;
    s->files=copy_files(s,old,1);
    if(!s->files) { s->files=old; return -ENOMEM; }
    talloc_unlink(s,old);
    struct execve_proc_exe_state *info = (void *)d1;
    char name[256], prefix[128], guest[PATH_MAX];
    struct Reply r;
    ssize_t length = readlink(info->host_path, name, sizeof(name) - 1);
    if (length < 0)
      return 0;
    name[length] = 0;
    snprintf(prefix, sizeof(prefix), "/memfd:banana-vfs-%s-", s->broker->tag);
    if (strncmp(name, prefix, strlen(prefix)))
      return 0;
    uint64_t id = strtoull(name + strlen(prefix), NULL, 10);
    int error = rpc(s, 13, 1, id, 0, 0, NULL, NULL, &r, guest, sizeof(guest));
    if (error)
      return error;
    strcpy(info->guest_path, guest);
    info->substituted = true;
    return 0;
  }
  case READLINK_PROC_FD: {
    struct readlink_proc_fd_state *info = (void *)d1;
    Tracee owner = *t;
    owner.pid = info->pid;
    uint64_t id = descriptor(s, &owner, info->fd);
    if (id) {
      struct Reply r;
      char guest[PATH_MAX];
      int error = rpc(s, 13, 1, id, 0, 0, NULL, NULL, &r, guest, sizeof(guest));
      if (error)
        return error;
      if (snprintf(info->host_path, PATH_MAX, "%s%s", get_root(t), guest) >=
          PATH_MAX)
        return -ENAMETOOLONG;
      info->substituted = true;
    }
    return 0;
  }
  case GUEST_PATH: {
    char combined[PATH_MAX], body[PATH_MAX];
    struct Reply r;
    const char *user = (char *)d2;
    const char *base = (char *)d1;
    if (snprintf(combined, sizeof(combined), "%s/%s",
                 user[0] == '/' ? "" : base, user) >= PATH_MAX)
      return -ENAMETOOLONG;
    int e = owned(s, combined);
    if (e <= 0)
      return e;
    e = rpc(s, 2, 1, 0, 0, 0, combined, NULL, &r, body, sizeof(body));
    if (e)
      return e;
    if (r.directory) {
      const char *empty = getenv("BANANA_VFS_EMPTY");
      if (!empty)
        return -EINVAL;
      strcpy((char *)d1, empty);
    } else {
      uint64_t id = r.id, len = r.length;
      e = rpc(s, 3, 0, id, 0, len, NULL, NULL, &r, NULL, 0);
      if (e)
        return e;
      strcpy((char *)d1, body);
    }
    return 1;
  }
  default:
    return 0;
  }
}

/* PRoot's initial cwd and argv lookup canonicalize without translate_path. */
int banana_vfs_guest_path(Tracee *t, char *out, const char *input,
                          int directory) {
  Extension *x = get_extension(t, banana_vfs_callback);
  if (!x)
    return 0;
  struct State *s = x->config;
  char p[PATH_MAX];
  struct Reply r;
  if (snprintf(p, sizeof(p), "%s/%s", input[0] == '/' ? "" : t->fs->cwd,
               input) >= PATH_MAX)
    return -ENAMETOOLONG;
  int e = owned(s, p);
  if (e <= 0)
    return e;
  e = rpc(s, 1, 0, 0, 0, 0, p, NULL, &r, NULL, 0);
  if (e)
    return e;
  if (directory && !r.directory)
    return -ENOTDIR;
  e = rpc(s, 14, 0, 0, 0, 0, p, NULL, &r, out, PATH_MAX);
  return e ? e : 1;
}
