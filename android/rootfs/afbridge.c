// afbridge: runs inside the Android rootfs next to Xvfb.
//  * Copies the X screen into a shared memory-mapped frame file that the
//    Android app draws (header + RGBA pixels).
//  * Accepts input commands on an abstract Unix socket (the app connects with
//    LocalSocket) and injects them with XTest, so the game sees ordinary X11
//    pointer/keyboard events. Abstract sockets avoid filesystem FIFOs, which
//    Android's SELinux policy denies in some app/shell directories.
//
// Usage: afbridge <frame-file> <socket-name> [fps]
// Input lines:  m X Y        pointer to X,Y (screen pixels)
//               b N 1|0      button N press/release
//               k KEYSYM 1|0 key press/release (X keysym name, e.g. t, 1, Escape)
#include <X11/Xlib.h>
#include <X11/Xutil.h>
#include <X11/extensions/XShm.h>
#include <X11/extensions/XTest.h>
#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <pthread.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ipc.h>
#include <sys/mman.h>
#include <sys/shm.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/un.h>
#include <time.h>
#include <unistd.h>

struct header {
    uint32_t magic;   // 'AFFB'
    uint32_t width;
    uint32_t height;
    uint32_t seq;     // incremented after each complete frame
    uint32_t alive;   // bridge heartbeat (frames attempted)
    uint32_t pad[3];
};
#define HEADER_SIZE 32
#define MAGIC 0x42464641u

static Display *input_display;
static pthread_mutex_t lock = PTHREAD_MUTEX_INITIALIZER;

static void handle_line(const char *line) {
    char name[64] = {0};
    int a = 0, b = 0;
    pthread_mutex_lock(&lock);
    if (sscanf(line, "m %d %d", &a, &b) == 2) {
        XTestFakeMotionEvent(input_display, 0, a, b, CurrentTime);
    } else if (sscanf(line, "b %d %d", &a, &b) == 2) {
        XTestFakeButtonEvent(input_display, a, b, CurrentTime);
    } else if (sscanf(line, "k %63s %d", name, &b) == 2) {
        KeySym sym = XStringToKeysym(name);
        KeyCode code = sym ? XKeysymToKeycode(input_display, sym) : 0;
        if (code) XTestFakeKeyEvent(input_display, code, b, CurrentTime);
        else fprintf(stderr, "afbridge: unknown key %s\n", name);
    }
    XFlush(input_display);
    pthread_mutex_unlock(&lock);
}

static void *client_thread(void *arg) {
    FILE *in = fdopen((int)(intptr_t)arg, "r");
    char line[256];
    while (in && fgets(line, sizeof line, in)) handle_line(line);
    if (in) fclose(in);
    return NULL;
}

static void *input_thread(void *arg) {
    const char *name = arg;
    int server = socket(AF_UNIX, SOCK_STREAM, 0);
    struct sockaddr_un addr = {.sun_family = AF_UNIX};
    size_t len = strlen(name);
    if (len > sizeof addr.sun_path - 2) len = sizeof addr.sun_path - 2;
    memcpy(addr.sun_path + 1, name, len); // leading NUL: abstract namespace
    socklen_t addr_len = offsetof(struct sockaddr_un, sun_path) + 1 + len;
    if (server < 0 || bind(server, (struct sockaddr *)&addr, addr_len) != 0 || listen(server, 4) != 0) {
        perror("afbridge: input socket");
        return NULL;
    }
    for (;;) {
        int client = accept(server, NULL, NULL);
        if (client < 0) { if (errno == EINTR) continue; perror("afbridge: accept"); sleep(1); continue; }
        pthread_t t;
        pthread_create(&t, NULL, client_thread, (void *)(intptr_t)client);
        pthread_detach(t);
    }
    return NULL;
}

int main(int argc, char **argv) {
    if (argc < 3) {
        fprintf(stderr, "usage: afbridge <frame-file> <socket-name> [fps]\n");
        return 2;
    }
    int fps = argc > 3 ? atoi(argv[3]) : 60;
    if (fps < 1 || fps > 120) fps = 60;
    XInitThreads();
    Display *dpy = NULL;
    for (int i = 0; i < 100 && !(dpy = XOpenDisplay(NULL)); i++) usleep(100000);
    input_display = XOpenDisplay(NULL);
    if (!dpy || !input_display) { fprintf(stderr, "afbridge: no X display\n"); return 1; }
    int ev, er, maj, min;
    if (!XTestQueryExtension(input_display, &ev, &er, &maj, &min))
        fprintf(stderr, "afbridge: XTest unavailable; input disabled\n");

    Window root = DefaultRootWindow(dpy);
    XWindowAttributes attrs;
    XGetWindowAttributes(dpy, root, &attrs);
    int w = attrs.width, h = attrs.height;

    pthread_t thread;
    pthread_create(&thread, NULL, input_thread, argv[2]);

    size_t size = HEADER_SIZE + (size_t)w * h * 4;
    int fd = open(argv[1], O_RDWR | O_CREAT, 0644);
    if (fd < 0 || ftruncate(fd, size) != 0) { perror("afbridge: frame file"); return 1; }
    uint8_t *map = mmap(NULL, size, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
    if (map == MAP_FAILED) { perror("afbridge: mmap"); return 1; }
    struct header *hdr = (struct header *)map;
    hdr->magic = MAGIC; hdr->width = w; hdr->height = h;
    uint32_t *pixels = (uint32_t *)(map + HEADER_SIZE);

    XShmSegmentInfo shm = {0};
    XImage *image = NULL;
    int use_shm = XShmQueryExtension(dpy);
    if (use_shm) {
        image = XShmCreateImage(dpy, attrs.visual, attrs.depth, ZPixmap, NULL, &shm, w, h);
        shm.shmid = shmget(IPC_PRIVATE, image->bytes_per_line * image->height, IPC_CREAT | 0600);
        if (shm.shmid < 0) {
            use_shm = 0;
            XDestroyImage(image);
            image = NULL;
        } else {
            shm.shmaddr = image->data = shmat(shm.shmid, NULL, 0);
            shm.readOnly = False;
            XShmAttach(dpy, &shm);
            XSync(dpy, False);
            shmctl(shm.shmid, IPC_RMID, NULL);
        }
    }
    fprintf(stderr, "afbridge: %dx%d depth %d shm=%d fps=%d\n", w, h, attrs.depth, use_shm, fps);

    struct timespec next;
    clock_gettime(CLOCK_MONOTONIC, &next);
    long period = 1000000000L / fps;
    for (;;) {
        hdr->alive++;
        XImage *frame = image;
        if (use_shm) {
            if (!XShmGetImage(dpy, root, image, 0, 0, AllPlanes)) frame = NULL;
        } else {
            frame = XGetImage(dpy, root, 0, 0, w, h, AllPlanes, ZPixmap);
        }
        if (frame && frame->bits_per_pixel == 32) {
            for (int y = 0; y < h; y++) {
                const uint32_t *src = (const uint32_t *)(frame->data + (size_t)y * frame->bytes_per_line);
                uint32_t *dst = pixels + (size_t)y * w;
                for (int x = 0; x < w; x++) {
                    uint32_t p = src[x]; // 0x00RRGGBB
                    // Android ARGB_8888 buffers are RGBA in memory order.
                    dst[x] = 0xff000000u | ((p & 0xff) << 16) | (p & 0xff00) | ((p >> 16) & 0xff);
                }
            }
            __atomic_add_fetch(&hdr->seq, 1, __ATOMIC_RELEASE);
        }
        if (!use_shm && frame) XDestroyImage(frame);
        next.tv_nsec += period;
        while (next.tv_nsec >= 1000000000L) { next.tv_nsec -= 1000000000L; next.tv_sec++; }
        struct timespec now;
        clock_gettime(CLOCK_MONOTONIC, &now);
        if (now.tv_sec > next.tv_sec + 1) next = now; // fell far behind; resync
        clock_nanosleep(CLOCK_MONOTONIC, TIMER_ABSTIME, &next, NULL);
    }
}
