#!/bin/bash
# Runs INSIDE Bubblewrap. No host display socket or home directory is mounted.
set -eu
mkdir -p /tmp/.X11-unix
export DISPLAY=:0 HOME=/tmp LIBGL_ALWAYS_SOFTWARE=1 __GLX_VENDOR_LIBRARY_NAME=mesa
export __EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json
/work/Xvfb :0 -screen 0 1024x768x24 -nolisten tcp -ac +iglx > /work/xvfb.log 2>&1 &
xpid=$!
trap 'kill "$xpid" 2>/dev/null || true' EXIT
for i in {1..50}; do
    [[ -S /tmp/.X11-unix/X0 ]] && break
    kill -0 "$xpid" || { echo 'Private X server failed; see xvfb.log' >&2; exit 1; }
    sleep 0.1
done
# LD_PRELOAD is applied only to the 32-bit game, never to the 64-bit host tools.
timeout -k 1s 45s strace -f -s 160 -o /work/startup.strace \
    -e trace=open,openat,connect,ioctl,write,execve \
    -E ARCADE_BBH_ADAPTER=1 -E LD_PRELOAD=/work/libarcade_linux_shim.so /game/game \
    > /work/stdout.log 2> /work/stderr.log &
gpid=$!
sleep 2
window=$(xdotool search --name 'Banana pepper - Big Buck Hunter Pro' | head -1)
xdotool key --window "$window" t
sleep 1
import -window root /work/tracking.png
xdotool key --window "$window" 1
sleep 1
import -window root /work/after-start.png
xdotool key --window "$window" 1
sleep 2
import -window root /work/after-start2.png
xdotool key --window "$window" 5
sleep 1
xdotool key --window "$window" 1
sleep 3
import -window root /work/game-select.png
xdotool mousemove --window "$window" 160 220 click --window "$window" 1
sleep 2
import -window root /work/after-shot.png
xdotool mousemove --window "$window" 160 220 click --window "$window" 1
sleep 3
import -window root /work/mode-select.png
xdotool mousemove --window "$window" 160 140 click --window "$window" 1
sleep 3
import -window root /work/adventure-select.png
xdotool mousemove --window "$window" 160 180 click --window "$window" 1
sleep 3
import -window root /work/adventure-intro.png
xdotool mousemove --window "$window" 160 180 click --window "$window" 1
sleep 3
import -window root /work/records.png
xdotool mousemove --window "$window" 160 180 click --window "$window" 1
sleep 3
xdotool search --name 'Banana pepper - Big Buck Hunter Pro' > /work/windows.txt || true
# Sampling several frames avoids presenting a capture taken during a redraw.
best=0
for frame in {1..5}; do
    if import -window root /work/frame.png 2>> /work/screenshot.log; then
        size=$(stat -c %s /work/frame.png)
        if (( size > best )); then cp /work/frame.png /work/screen.png; best=$size; fi
    fi
    sleep 0.1
done
rm -f /work/frame.png
sleep 5
xdotool mousemove --window "$window" 320 240 click --window "$window" 1
sleep 2
import -window root /work/later-screen.png 2>> /work/screenshot.log || true
cp /work/screen.png /work/instructions.png
if [[ -s /work/later-screen.png ]]; then cp /work/later-screen.png /work/site-select.png; fi
xdotool mousemove --window "$window" 120 150 click --window "$window" 1
sleep 4
import -window root /work/hunt.png 2>> /work/screenshot.log || true
if [[ -s /work/hunt.png ]]; then cp /work/hunt.png /work/screen.png; fi
# Timeout is expected for a running game. Preserve its real status separately.
set +e
wait "$gpid"
result=$?
printf '%s\n' "$result" > /work/game-exit-code.txt
exit 0
