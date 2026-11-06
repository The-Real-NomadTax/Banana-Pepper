#!/bin/bash
# Runs INSIDE the Android rootfs (proot). The app binds:
#   /game      supplied Big Buck Hunter directory (read-only use)
#   /game/game hash-verified prepared runtime copy
#   /bbh3data  game assets, /bbh3user writable saves
#   /run/af    session dir: frame file, logs
# Environment from the app: AF_INPUT_SOCKET, AF_AUDIO_SOCKET (optional),
# AF_WIDTH/AF_HEIGHT (default 640x480), AF_APP_PID (exit when the app dies).
set -u
W=${AF_WIDTH:-640}
H=${AF_HEIGHT:-480}
S=/run/af
# A previous session can outlive a crashed app process; stop it first.
if [ -f "$S/pids" ]; then
    kill -9 $(cat "$S/pids") 2>/dev/null
    sleep 0.3
fi
rm -f /tmp/.X0-lock /tmp/.X11-unix/X0
echo $$ > "$S/pids"
export HOME=/tmp DISPLAY=:0 LIBGL_ALWAYS_SOFTWARE=1 GALLIUM_DRIVER=llvmpipe
mkdir -p /tmp/.X11-unix
Xvfb :0 -screen 0 "${W}x${H}x24" -nolisten tcp -ac +iglx -nocursor > "$S/xvfb.log" 2>&1 &
xpid=$!
echo $xpid >> "$S/pids"
gpid= bpid= wpid=
cleanup() { kill $gpid $bpid $xpid $wpid 2>/dev/null; rm -f "$S/pids"; }
trap cleanup EXIT INT TERM
for i in $(seq 1 100); do
    xset q >/dev/null 2>&1 && break
    kill -0 $xpid 2>/dev/null || { echo "X server failed; see xvfb.log"; exit 1; }
    sleep 0.1
done
afbridge "$S/frame.bin" "${AF_INPUT_SOCKET:-bananapepper.input}" 60 > "$S/bridge.log" 2>&1 &
bpid=$!
echo $bpid >> "$S/pids"

export BOX64_LOG=${BOX64_LOG:-1} BOX64_NOBANNER=0
export BOX64_LD_PRELOAD=/opt/arcade/libarcade_linux_shim.so
export ARCADE_BBH_ADAPTER=1 ARCADE_RESOLUTION="${W}x${H}"
if [ -n "${AF_AUDIO_SOCKET:-}" ]; then export ARCADE_BBH_AUDIO=1 ARCADE_AUDIO_SOCKET="$AF_AUDIO_SOCKET"; fi
cd /game
box64 /game/game > "$S/game.log" 2>&1 &
gpid=$!
echo $gpid >> "$S/pids"
if [ -n "${AF_APP_PID:-}" ]; then
    # Watchdog: nothing keeps running if Android kills the app.
    ( while kill -0 "$AF_APP_PID" 2>/dev/null; do sleep 1; done; kill -9 $gpid $bpid $xpid 2>/dev/null ) &
    wpid=$!
fi
wait $gpid
status=$?
echo "game exited with status $status" >> "$S/game.log"
exit $status
