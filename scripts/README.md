# Development tools

These tools build on Windows or capture user-supplied cabinet records.
The GDB files retain standalone historical investigation commands.
They are not installed or shipped with the application.

Registered compatibility plans and title hooks live in Rust under `game-compat/`.
Native isolation, Wine prefix management and Water execution live in `backends/`;
shared session orchestration lives in `banana-runtime`. Do not add interpreted
game launchers here or embed them in a Rust binary.
