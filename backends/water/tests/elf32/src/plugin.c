/* A plugin the program dlopens; it binds to the shipped library. */
extern int fixture_add(int a, int b);

int plugin_twice(int x) {
    return fixture_add(x, x);
}
