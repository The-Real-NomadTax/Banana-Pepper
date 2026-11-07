# Development tools

These tools are for building on Windows/WSL and capturing user-supplied cabinet
records. The GDB files retain standalone historical investigation commands.
They are not installed or shipped with the application.

Registered game actions live in Rust under `game-compat/`. Native isolation,
Wine prefix management and WSL process dispatch live in `backends/`; compiled
session supervision lives in `banana-runtime`. Do not add interpreted game
launchers here or embed them in a Rust binary.
