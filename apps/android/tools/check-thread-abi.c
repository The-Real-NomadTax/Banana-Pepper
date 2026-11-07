// Build with gcc -m32; run with the Android Box32 binary to check output widths.
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

int main(void) {
    pthread_attr_t attributes;
    struct {
        uint32_t before;
        void *address;
        uint32_t after_address;
        size_t size;
        uint32_t after_size;
        size_t guard;
        uint32_t after_guard;
    } output;
    memset(&output, 0xa5, sizeof(output));
    if (pthread_attr_init(&attributes)) return 1;
    int error = pthread_attr_getstack(&attributes, &output.address, &output.size);
    error |= pthread_attr_getguardsize(&attributes, &output.guard);
    pthread_attr_destroy(&attributes);
    if (error || output.before != 0xa5a5a5a5 || output.after_address != 0xa5a5a5a5
            || output.after_size != 0xa5a5a5a5 || output.after_guard != 0xa5a5a5a5) {
        fprintf(stderr, "Thread attribute output overwrote guest storage\n");
        return 1;
    }
    puts("THREAD_ATTRIBUTE_ABI_OK");
    return 0;
}
