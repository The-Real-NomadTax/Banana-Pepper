/* Descriptor delivery and POSIX open-description state for the VFS tracer.
 * Kept in the tracer: the Rust View owns content, mappings and persistence. */
#include "syscall/chain.h"
#include <sched.h>
#include <stddef.h>
#include <sys/syscall.h>

struct OpenFile { uint64_t id, offset; unsigned mode; };
struct FileEntry {
  int fd, cloexec;
  struct OpenFile *file;
  struct FileEntry *next;
};
struct FileTable { struct FileEntry *head; };

static struct FileEntry *file_entry(struct State *s, int fd) {
  for (struct FileEntry *e = s->files->head; e; e = e->next)
    if (e->fd == fd) return e;
  return NULL;
}
static void forget_file(struct State *s, int fd) {
  struct FileEntry **e = &s->files->head;
  while (*e) {
    if ((*e)->fd == fd) {
      struct FileEntry *old = *e;
      *e = old->next;
      talloc_free(old);
      return;
    }
    e = &(*e)->next;
  }
}
static int remember_file(struct State *s, int fd, struct OpenFile *file, int cloexec) {
  forget_file(s, fd);
  struct FileEntry *e = talloc_zero(s->files, struct FileEntry);
  if (!e) return -ENOMEM;
  e->fd = fd;
  e->cloexec = cloexec;
  e->file = talloc_reference(e, file);
  e->next = s->files->head;
  s->files->head = e;
  return 0;
}
static struct FileTable *copy_files(struct State *s, struct FileTable *parent, int exec) {
  struct FileTable *table = talloc_zero(s, struct FileTable);
  if (!table) return NULL;
  for (struct FileEntry *old = parent->head; old; old = old->next) {
    if (exec && old->cloexec) continue;
    struct FileEntry *e = talloc_zero(table, struct FileEntry);
    if (!e) { talloc_free(table); return NULL; }
    e->fd = old->fd;
    e->cloexec = old->cloexec;
    e->file = talloc_reference(e, old->file);
    e->next = table->head;
    table->head = e;
  }
  return table;
}
static int transfer_cleanup(struct State *s) {
  if (s->listener >= 0) close(s->listener);
  if (s->transfer_fd >= 0) close(s->transfer_fd);
  s->listener = s->transfer_fd = -1;
  return 0;
}
static int receive_fd(int socket) {
  char byte;
  struct iovec vector = {&byte, 1};
  union { struct cmsghdr align; char bytes[CMSG_SPACE(sizeof(int))]; } control;
  struct msghdr message = {.msg_iov=&vector, .msg_iovlen=1,
    .msg_control=control.bytes, .msg_controllen=sizeof(control.bytes)};
  ssize_t n;
  do { n=recvmsg(socket, &message, MSG_CMSG_CLOEXEC); } while(n<0 && errno==EINTR);
  if(n<0) return -errno;
  struct cmsghdr *c = CMSG_FIRSTHDR(&message);
  if(n!=1 || (message.msg_flags & MSG_CTRUNC) || !c ||
     c->cmsg_level!=SOL_SOCKET || c->cmsg_type!=SCM_RIGHTS ||
     c->cmsg_len!=CMSG_LEN(sizeof(int))) return -EPROTO;
  int fd;
  memcpy(&fd, CMSG_DATA(c), sizeof(fd));
  return fd;
}
static int deliver_fd(int socket, int fd) {
  char byte=0;
  struct iovec vector={&byte,1};
  union { struct cmsghdr align; char bytes[CMSG_SPACE(sizeof(int))]; } control={0};
  struct msghdr message={.msg_iov=&vector,.msg_iovlen=1,
    .msg_control=control.bytes,.msg_controllen=sizeof(control.bytes)};
  struct cmsghdr *c=CMSG_FIRSTHDR(&message);
  c->cmsg_level=SOL_SOCKET; c->cmsg_type=SCM_RIGHTS;
  c->cmsg_len=CMSG_LEN(sizeof(int));
  memcpy(CMSG_DATA(c), &fd, sizeof(fd));
  ssize_t n;
  do { n=sendmsg(socket,&message,MSG_NOSIGNAL); } while(n<0 && errno==EINTR);
  return n==1 ? 0 : (n<0 ? -errno : -EIO);
}
struct TransferPacket {
  struct sockaddr_un address;
  struct msghdr message;
  struct iovec vector;
  union { struct cmsghdr align; char bytes[CMSG_SPACE(sizeof(int))]; } control;
  char byte;
};
enum { FD_SOCKET=1, FD_CONNECT, FD_RECEIVE };
/* SO_PEERCRED reports the connecting thread's process (thread group), while a
 * tracee is one thread; compare against the tracee's thread group. */
static pid_t thread_group(pid_t tid) {
  char path[64], line[128];
  snprintf(path,sizeof(path),"/proc/%d/status",tid);
  FILE *f=fopen(path,"re");
  if(!f) return -1;
  pid_t group=-1;
  while(fgets(line,sizeof(line),f))
    if(!strncmp(line,"Tgid:",5)) { group=(pid_t)strtol(line+5,NULL,10); break; }
  fclose(f);
  return group;
}
static int transfer_finish(struct State *s, Tracee *t, long result) {
  transfer_cleanup(s);
  s->stage=0;
  if(s->socket_fd>=0) {
    int e=register_chained_syscall(t,PR_close,s->socket_fd,0,0,0,0,0);
    s->socket_fd=-1;
    if(e<0) return e;
  }
  force_chain_final_result(t,result);
  return 1;
}
static int begin_transfer(struct State *s, Tracee *t, uint64_t id, unsigned mode) {
  struct Reply r;
  int e=rpc(s,17,0,id,0,0,NULL,NULL,&r,NULL,0);
  if(e) return e;
  s->transfer_fd=receive_fd(s->broker->fd);
  if(s->transfer_fd<0) return s->transfer_fd;
  s->listener=socket(AF_UNIX,SOCK_SEQPACKET|SOCK_CLOEXEC|SOCK_NONBLOCK,0);
  if(s->listener<0) { e=-errno; transfer_cleanup(s); return e; }
  struct TransferPacket packet={0};
  packet.address.sun_family=AF_UNIX;
  unsigned char random[24];
  ssize_t n;
  // Bionic declares getrandom() only from API 28; the tracer targets API 26.
  do { n=syscall(__NR_getrandom,random,sizeof(random),0); } while(n<0 && errno==EINTR);
  if(n!=sizeof(random)) { transfer_cleanup(s); return -EIO; }
  memcpy(packet.address.sun_path+1,random,sizeof(random));
  socklen_t size=offsetof(struct sockaddr_un,sun_path)+1+sizeof(random);
  if(bind(s->listener,(void*)&packet.address,size) || listen(s->listener,1)) {
    e=-errno; transfer_cleanup(s); return e;
  }
  s->scratch=alloc_mem(t,sizeof(packet));
  if(!s->scratch) { transfer_cleanup(s); return -EFAULT; }
  packet.vector.iov_base=(void*)(s->scratch+offsetof(struct TransferPacket,byte));
  packet.vector.iov_len=1;
  packet.message.msg_iov=(void*)(s->scratch+offsetof(struct TransferPacket,vector));
  packet.message.msg_iovlen=1;
  packet.message.msg_control=(void*)(s->scratch+offsetof(struct TransferPacket,control));
  packet.message.msg_controllen=sizeof(packet.control);
  if(write_data(t,s->scratch,&packet,sizeof(packet))<0) {
    transfer_cleanup(s); return -EFAULT;
  }
  s->address_size=size;
  s->open_id=id; s->open_mode=mode;
  s->stage=FD_SOCKET; s->socket_fd=-1;
  set_sysnum(t,PR_socket);
  poke_reg(t,SYSARG_1,AF_UNIX);
  poke_reg(t,SYSARG_2,SOCK_SEQPACKET|SOCK_CLOEXEC);
  poke_reg(t,SYSARG_3,0);
  t->sysexit_pending=true; t->restart_how=PTRACE_SYSCALL;
  return 1;
}
static int transfer_step(struct State *s, Tracee *t) {
  long result=(long)peek_reg(t,CURRENT,SYSARG_RESULT);
  if(result<0) return transfer_finish(s,t,result);
  switch(s->stage) {
  case FD_SOCKET:
    s->socket_fd=result;
    s->stage=FD_CONNECT;
    return register_chained_syscall(t,PR_connect,s->socket_fd,
      s->scratch+offsetof(struct TransferPacket,address),s->address_size,0,0,0)<0
      ? transfer_finish(s,t,-ENOMEM) : 1;
  case FD_CONNECT: {
    int peer=accept4(s->listener,NULL,NULL,SOCK_CLOEXEC|SOCK_NONBLOCK);
    if(peer<0) return transfer_finish(s,t,-errno);
    struct ucred credentials;
    socklen_t length=sizeof(credentials);
    int e=getsockopt(peer,SOL_SOCKET,SO_PEERCRED,&credentials,&length);
    if(e || credentials.uid!=getuid() || credentials.pid!=thread_group(t->pid)) {
      close(peer); return transfer_finish(s,t,-EACCES);
    }
    e=deliver_fd(peer,s->transfer_fd);
    close(peer); transfer_cleanup(s);
    if(e) return transfer_finish(s,t,e);
    s->stage=FD_RECEIVE;
    return register_chained_syscall(t,PR_recvmsg,s->socket_fd,
      s->scratch+offsetof(struct TransferPacket,message),
      s->open_mode&O_CLOEXEC ? MSG_CMSG_CLOEXEC : 0,0,0,0)<0
      ? transfer_finish(s,t,-ENOMEM) : 1;
  }
  case FD_RECEIVE: {
    struct TransferPacket packet;
    if(result!=1 || read_data(t,&packet,s->scratch,sizeof(packet))<0)
      return transfer_finish(s,t,-EIO);
    struct cmsghdr *c=(void*)&packet.control;
    if((packet.message.msg_flags&MSG_CTRUNC) || c->cmsg_level!=SOL_SOCKET ||
       c->cmsg_type!=SCM_RIGHTS || c->cmsg_len!=CMSG_LEN(sizeof(int)))
      return transfer_finish(s,t,-EPROTO);
    int fd;
    memcpy(&fd,CMSG_DATA(c),sizeof(fd));
    struct OpenFile *file=talloc_zero(s,struct OpenFile);
    if(!file) {
      register_chained_syscall(t,PR_close,fd,0,0,0,0,0);
      return transfer_finish(s,t,-ENOMEM);
    }
    file->id=s->open_id; file->mode=s->open_mode&~O_CLOEXEC;
    int e=remember_file(s,fd,file,!!(s->open_mode&O_CLOEXEC));
    talloc_unlink(s,file);
    if(e) register_chained_syscall(t,PR_close,fd,0,0,0,0,0);
    return transfer_finish(s,t,e ? e : fd);
  }
  default: return -EPROTO;
  }
}
