/* A 32-bit Linux program for Water's dynamic linker test. It needs a library
 * the game ships (found through RPATH $ORIGIN/lib), whose constructor must
 * have run and whose data is copied into the program (copy relocation), and
 * it dlopens a plugin that binds back into that library. Exit status 42
 * means every check passed; another status names the failed check. */
#include <dlfcn.h>
#include <string.h>

extern int fixture_add(int a, int b);
extern int fixture_initialized;
extern const char fixture_name[8];

int main(void) {
    if (fixture_add(2, 3) != 5)
        return 10;
    if (fixture_initialized != 1)
        return 11;
    if (strlen(fixture_name) != 7)
        return 12;
    void *plugin = dlopen("libbanana_plugin.so", RTLD_NOW);
    if (!plugin)
        return 13;
    int (*twice)(int) = (int (*)(int))dlsym(plugin, "plugin_twice");
    if (!twice || twice(21) != 42)
        return 14;
    if (dlsym(plugin, "missing_symbol") != 0 || dlerror() == 0)
        return 15;
    return 42;
}
