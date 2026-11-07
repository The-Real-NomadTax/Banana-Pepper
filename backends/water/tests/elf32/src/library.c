/* A library a game ships next to its program. */
#include <string.h>

int fixture_initialized = 0;
const char fixture_name[8] = "fixture";

__attribute__((constructor)) static void start(void) {
    fixture_initialized = 1;
}

int fixture_add(int a, int b) {
    return a + b + (int)strlen("");
}
