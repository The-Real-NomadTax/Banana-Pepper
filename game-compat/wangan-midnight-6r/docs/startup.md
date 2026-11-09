# Wangan Midnight Maximum Tune 6R

Supported build: `wmn6r.exe`, SHA-256 `bf9fded65f5d029deaed5c3b5d06e8035dae70eee2e03c51e3adc2b1444ae2be` (64-bit, Direct3D 11). Other builds are rejected before anything is patched.

## Backend

Linux hosts run the game under Wine (win64 prefix, wined3d). Windows hosts are not supported yet: the native backend's view shim is 32-bit only, so the plan fails with that error rather than running unpatched.

The Wine backend maps the game's drives to the session view:

| Drive | View folder | Cabinet use |
| --- | --- | --- |
| `D:` | game root | the game's own folder |
| `F:`, `G:` | `TP` | operator settings (`setting.lua.gz`), credits, backups (primary and mirror) |
| `J:` | `TP` | debug logs (`frame_log.csv.gz`, `assert_log.csv.gz`) |

The `TP` folder must exist in the game folder. Writes land in the entry's overlay, never in the source. Wine sessions run without the system D-Bus, so the host's removable disks never get drive letters inside a session.

## Executable and devices

The plan serves an in-memory `wmn6r.exe` whose HASP (`hasp_windows_x64_28756.dll`) and USB-finder (`nbamUsbFinder.dll`) imports name `bngrw.dll`. The hook DLL (`hook/`, artifact `arcade_wmmt6r`) is placed at `bngrw.dll` and serves all three:

- **HASP dongle**: login/read/write/decrypt on a dongle memory held in the process. The 64-byte identity block at `0xD00` holds the cabinet serial (`280803400001`: twelve digits starting with the title code `2808`, a board digit (0 = any), a cabinet digit (1 = terminal, 3 to 5 = driving cabinet; 3 here), and six digits starting with 40 or 99, the rules the game's check applies), then at `0x3E` the 8-bit sum of bytes `0..0x3E` and at `0x3F` its complement (the layout the game's dongle check verifies).
- **USB finder**: reports the USB memory key's serial (`284211480001`, NUL-terminated): twelve digits starting `2842`, then 1, then 0/1/2, then 48 or 99, the rules the game's check applies.
- **Card reader** (`BngRw*`): every call fails (no reader).
- **JVS I/O board** on COM3: the exe's own serial-port imports are redirected for that port only (`hook/src/board.rs`, unit-tested on the host). Two players with 16 switches, two coin slots, eight 16-bit analog channels. Namco's serial pass-through (`0x70` with `16`/`05`/`15`) accepts sends and never receives anything. Other COM ports report as absent.

Controls (`src/controls.rs`): wheel on analog 0, gas 1, brake 2; switches in player one's bytes; the six-speed shifter as gate sensors in the second byte (rows `0x80`/`0x40`, columns `0x20`/`0x10`).

## Authentication service

On a cabinet, `AMAuthd.exe` is a COM local server (class `{4603BB03-058D-43D9-B96F-639BE908C1ED}`, interface `{045A5150-D2B3-4590-A38B-C1158678E1AC}`). The hook answers `CoCreateInstance` for that class with an in-process object (`hook/src/hook/auth.rs`); every method succeeds. Method numbers count after IUnknown:

- 6 (state, 0xC8 bytes): ALL.Net authentication finished (`+0x00` = 0xF), succeeded (`+0x04` = 0) and complete (`+0x08` = 2), sequence 1 at `+0x0C`; Mucha likewise at `+0x18`/`+0x1C`/`+0x20`; authentication type ALL.Net at `+0xB8` (0 makes the game run offline).
- 7 (configuration, 0xA8 bytes): the PCB serial at `+0x20` (must equal the dongle serial, else E1905), the cabinet's address at `+0x30`, gateway at `+0x40`, network type 1 at `+0x94`.
- 11 (location, 0x8A2 bytes): ALL.Net's game server URI at `+0x000`. The game takes it only with the `https` scheme (else it keeps `https://localhost`); drive cabinets then send that traffic through their terminal.

## Cabinet network

The game plans a private cabinet network in its own constants: the drive cabinet at 192.168.92.11, the Wangan terminal at 192.168.92.20, the content router 192.168.92.254 (any other gateway is E0502 CONTENT ROUTER MISMATCH). The host's network is untouched; the hook shows the game that plan (`hook/src/hook/network.rs`):

- `GetIpForwardTable`: the default route's next hop is 192.168.92.254.
- `GetIpAddrTable` and `getsockname`: the interface carrying the default route (else the first non-loopback one) has 192.168.92.11/24. The game finds its own address from a UDP socket's local endpoint and looks it up in the address table; `GetIfEntry` on that interface is its cable check (E0505 NETWORK DISCONNECT if it fails).
- `bind` to a 192.168.92.x address binds every address on the same port; `setsockopt` multicast options naming a 192.168.92.x interface use the default interface.

## Wangan terminal

Cabinets of a group and their terminal share UDP port 50765 (multicast 225.0.0.1). A datagram is a 4-byte header (flags, sender node, little-endian length of header and payload), a `wm.protobuf.Message` (schema embedded in the exe) and a big-endian CRC-32 (MSB first, polynomial 0x04C11DB7, initial value all ones, no final XOR). `hook/src/terminal.rs` builds the format and is unit-tested against a heartbeat the cabinet sent.

The boot sequencer waits in its terminal state ("connecting to the Wangan terminal") on a five-entry peer table, keyed by sender node and address: nodes 0 to 3 are drive cabinets and node 4 is the terminal. A peer is usable (status 1) only when its datagrams come from a private address (10/8, 172.16/12, 192.168/16, 169.254/16; another address is 3, a second address for one node is 2). The cabinet's own slot (its node, 0) needs its own heartbeat back, which multicast loopback may not deliver on the host, so each link multicast the game sends is also sent to its own port on loopback.

The hook runs an emulated terminal: every 100 ms it sends, as node 4 to 127.0.0.1:50765,

- `heart_beat` with the terminal serial `280801400002` (cabinet digit 1 = terminal; digit 4 must match the cabinet's);
- `batch_setting_state` 3 (the game's `State` enum: 1 checking, 2 NG, 3 OK; drive cabinets wait while their terminal reports 1);
- `batch_setting`: free play and the cabinet's default costs, the cabinet's own software revision (field 19, which must match; the game computes it at boot) and `sync_time` (field 20, Unix seconds, which must be non-zero and advance);
- `information` (system mode 1).

Received datagrams appear to come from the planned addresses (`WSARecvFrom` and its completion): from the cabinet's own port on loopback or from the host's address, 192.168.92.11; from any other loopback port, the terminal's 192.168.92.20.

## Game server

A drive cabinet sends its game server traffic to its terminal (TCP 192.168.92.20:50765) over TLS 1.2, and accepts the terminal only with a certificate chained to the game's CA file `data_jp/network/certs/v388-ca-cert.pem` (the real terminal's is RSA, `CN=terminal.wm4`, no host name check). The emulation has its own CA:

- `hook/src/pki.rs` builds a fixed P-256 CA and terminal certificate (`CN=terminal.wm4`), deterministically, so the plan serves the CA at that path as an in-memory view file (the source file is untouched) and the hook presents the matching certificate. The keys only make the local emulation trusted; they are not secrets.
- `hook/src/tls.rs` is a TLS 1.2 server for one suite the cabinet's OpenSSL 1.0.2 offers, ECDHE-ECDSA-AES128-GCM-SHA256 (no resumption, renegotiation or client certificates). `hook/tests/tls.rs` checks it against an `openssl s_client` that trusts only the emulation CA.
- The hook routes TCP connections to the terminal's address to loopback (`connect`, and the `ConnectEx` the game gets from `WSAIoctl`), where `hook/src/hook/server.rs` listens on 127.0.0.1:50765 and serves HTTP/1.1 over that TLS.
- `hook/src/game_server.rs` answers `POST /method/<name>` with the `wm.protobuf.<Name>Response` (schemas embedded in the exe; proto2 `required` fields must all be present) and `GET /resource/<name>` lists. `register_system_info` gets region 1 and place `JPN0000`; `place_list` holds that one shop; other lists are empty; any other method gets `error` 0 only, which suffices for the responses whose only required field is the error code.

A successful `register_system_info` is what keeps the cabinet online: the game refreshes its last server contact (`operation_W6R.bin.gz` in `F:`/`G:`) only after one, and stops with E0552 OFFLINE UPTIME ERROR once that is 36 hours or 5 days (less an hour) old.

## Movies

Attract mode plays WMV3/WMA movies (`data_jp/movie/*.wmv`) through a Media Foundation source reader and asks for RGB32 frames. Windows' WMV decoder outputs RGB32 itself; Wine's source decodes to YUV only and converts to RGB32 only when the reader has video processing enabled, so without it `SetCurrentMediaType` fails (MF_E_TOPO_CODEC_NOT_FOUND) and the game stops on an assertion. The hook sets `MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING` on the readers the game creates (`hook/src/hook/media.rs`). Decoding needs the host's GStreamer `avdec_wmv3` (gst-libav).

## Startup diagnostics

`arcade-desktop --action wmmt6r probe SOURCE` runs for up to ten minutes with:

- the game's own log at every level (one branch in its logger, patched only for the probe), reported by Wine as `OutputDebugString` lines in `game.log`;
- `[wmmt6r] sequencer state …` lines from the hook, following the game's boot sequencer (`game::InfoSystem::Impl::process`), and `peers …` lines with its link peer table;
- the dongle, network-manager and link fields the sequencer waits on, the network manager's strings (server URI, CA path, terminal address), the first auth method calls, name lookups, socket destinations, failed I/O completions and each game server request;
- thread stacks (exe return addresses) when the sequencer stalls for 20 s, and `ARCADE_WMMT6R_BREAK=addr,…` breakpoints that log registers and callers;
- crash reports with the exe return addresses on the faulting stack.

## Status

Verified on Linux (Wine 11.21, headless gamescope; nothing on the user's desktop), with inputs sent to the private display:

- the patched image loads and the hook installs; the JVS board is found, addressed and initialised (`JVIO: initialized`), including the serial pass-through polls;
- the game loads its operator settings from `F:`, starts sound, compiles its shaders, goes fullscreen and loads the message database;
- the boot sequencer passes the HASP dongle, USB memory key, dongle/PCB serial match, ALL.Net and Mucha authentication, the network checks (cable, gateway, shop router), the content router check and the Wangan terminal wait;
- the cabinet completes TLS with the emulated terminal; `register_system_info` succeeds ("server: success"), then `place_list`, `file_list`, `ranking` and `crown_list`; no E0525/E0552;
- attract mode runs with its movies (FREE PLAY, "1P play available");
- the gas pedal starts a game; without a card the guest path works: card question (NO), car and colour selection, story mode chapter 1 (Yokohama, Minato Mirai) loads and the race runs with the HUD, map, rival and story dialogue.

Not seen yet: a finished race and its result requests (`save_game_result` gets `error` 0 only, which its schema allows), steering and shifting with a real controller, audio output, saves, card play (no card reader is emulated), terminal mode. Boot takes about four minutes on this host (shader compilation dominates).
