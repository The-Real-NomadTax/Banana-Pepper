// Forward the virtual PulseAudio sink to the Kotlin AudioTrack transport.
#include <pulse/simple.h>
#include <pulse/error.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <stddef.h>
#include <stdint.h>
#include <signal.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>
#include <errno.h>

static int send_all(int fd, const void *data, size_t size) {
    const char *bytes = data;
    while (size) {
        ssize_t n = write(fd, bytes, size);
        if (n < 0 && errno == EINTR) continue;
        if (n <= 0) return -1;
        bytes += n;
        size -= n;
    }
    return 0;
}

int main(int argc, char **argv) {
    if (argc != 2 || strlen(argv[1]) > sizeof(((struct sockaddr_un *)0)->sun_path)-2) return 2;
    signal(SIGPIPE, SIG_IGN);
    int fd = socket(AF_UNIX, SOCK_STREAM, 0);
    struct sockaddr_un address = {.sun_family = AF_UNIX};
    size_t length = strlen(argv[1]);
    memcpy(address.sun_path + 1, argv[1], length);
    if (fd < 0 || connect(fd, (struct sockaddr *)&address,
            offsetof(struct sockaddr_un, sun_path) + 1 + length)) {
        perror("afaudio: Android socket");
        return 1;
    }
    pa_sample_spec spec = {.format = PA_SAMPLE_S16LE, .rate = 48000, .channels = 2};
    int error = 0;
    pa_simple *stream = pa_simple_new(NULL, "Banana Pepper", PA_STREAM_RECORD,
        "arcade.monitor", "Android output", &spec, NULL, NULL, &error);
    if (!stream) {
        fprintf(stderr, "afaudio: %s\n", pa_strerror(error));
        close(fd);
        return 1;
    }
    // Android and the supported ARM64 guest are little endian.
    uint32_t header[] = {0x31414641, 48000, 2};
    int result = send_all(fd, header, sizeof(header));
    char buffer[4096];
    while (!result) {
        if (pa_simple_read(stream, buffer, sizeof(buffer), &error) < 0) {
            fprintf(stderr, "afaudio: %s\n", pa_strerror(error));
            result = -1;
            break;
        }
        result = send_all(fd, buffer, sizeof(buffer));
    }
    pa_simple_free(stream);
    close(fd);
    return result ? 1 : 0;
}
