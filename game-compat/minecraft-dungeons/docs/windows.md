# Minecraft Dungeons Arcade on Windows

Windows Linux games use Water. Minecraft's supplied ELF64 build requires dynamic linking, thread/TLS support and graphics capabilities beyond the current static ELF64 worker. The backend reports those missing capabilities and does not launch the build.

Further support belongs in Water's reusable mechanisms, with build-specific reconstruction, cabinet devices, input mappings and captured-record handling in `game-compat/minecraft-dungeons`.

The registered plan and native Linux adapter remain available. See linux.md for Linux setup and seed-capture.md for captured-record handling. No Linux guest host, title-specific executable, extracted game tree or runtime scripts are shipped in the Windows package.
