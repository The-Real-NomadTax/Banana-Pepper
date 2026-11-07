# Water

Water Adapts The ELF Runtime translates Linux guest interfaces into Windows APIs. It executes guest x86 code in a dedicated process and provides reusable ELF loading, C-library, graphics, audio and filesystem mechanisms.

`water-x86.exe` and `water-x64.exe` are generic architecture-specific workers. The x86 worker loads a compatibility DLL through the versioned `AdapterV1` interface. Size/version checks run before guest loading. Title-specific patches, input mappings, cabinet devices and diagnostic input sequences live under `game-compat/`.

The filesystem is the same composed view used by Native and Wine. Water has no separate root/work write policy and creates no game copies. Its path translator confines guest paths to the mounted namespace; game writes reach the persistent overlay.

The ELF64 implementation supports a constrained static ABI. Dynamic linking, PIE relocation, TLS, broader threading and graphics requirements are reported as missing capabilities. Support grows per game and build; selecting Water does not imply arbitrary Linux compatibility.

Build through `just build`. Worker executables and adapter DLLs are selected from crate-owned artifact declarations. Normal launches use the shared session API, which owns process lifetimes and timeouts. `cargo test -p banana-water` runs synthetic ELF64 loader and execution checks.
