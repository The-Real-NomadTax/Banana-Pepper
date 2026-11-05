#!/bin/bash
# Runs only inside the private Bubblewrap test environment.
set -eu
export SDL_AUDIODRIVER=${SDL_AUDIODRIVER:-dummy} HOME=/tmp
export SDL12COMPAT_OPENGL_SCALING=0
if [[ ${ARCADE_JURASSIC_DESKTOP:-0} != 1 ]]; then
export DISPLAY=:0 LIBGL_ALWAYS_SOFTWARE=1 __GLX_VENDOR_LIBRARY_NAME=mesa
export MESA_DEBUG=context GALLIVM_PERF=noopt
export __EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json
mkdir -p /tmp/.X11-unix
/Xvfb :0 -screen 0 1360x768x24 -nolisten tcp -ac +iglx > /work/xvfb.log 2>&1 &
xpid=$!
trap 'kill "$xpid" 2>/dev/null || true' EXIT
for i in {1..50}; do [[ -S /tmp/.X11-unix/X0 ]] && break; sleep .1; done
fi
if [[ ${ARCADE_JURASSIC_LAUNCH:-0} == 1 ]]; then
    LD_PRELOAD=/shim.so /pm/game -f"${ARCADE_RESOLUTION:-1360x768}" > /work/game.log 2>&1 &
else
    gdb -batch -x /work/jurassic-explore.gdb --args /pm/game -f1024x768 > /work/gdb.log 2>&1 &
fi
pid=$!
seconds=${1:-60}
window=
for ((frame=10; frame<=seconds; frame+=10)); do
sleep 10
if [[ ${ARCADE_JURASSIC_DESKTOP:-0} != 1 ]]; then
    import -window root /work/screen-$frame.png 2>/work/screenshot.log || true
    if [[ -f /work/screen-$frame.png ]]; then cp /work/screen-$frame.png /work/screen.png; fi
fi
if [[ -z "$window" ]] || ! xdotool getwindowgeometry "$window" >/dev/null 2>&1; then
    window=$(xdotool search --onlyvisible --name 'Jurassic Park Arcade' | head -1) || true
fi
if [[ -n "$window" ]]; then
    xdotool getwindowname "$window" > /work/game-window-title.txt
    xdotool getwindowgeometry --shell "$window" > /work/window-geometry.txt
    import -window "$window" /work/window-$frame.png 2>> /work/screenshot.log || true
    if [[ ${ARCADE_JURASSIC_DESKTOP:-0} == 1 && -f /work/window-$frame.png ]]; then cp /work/window-$frame.png /work/screen-$frame.png; cp /work/window-$frame.png /work/screen.png; fi
fi
echo "Captured diagnostic frame at ${frame}s"
kill -0 "$pid" 2>/dev/null || break
done
if [[ -n "$window" ]]; then xdotool getwindowname "$window" > /work/windows.txt 2>/dev/null || true; else : > /work/windows.txt; fi
if [[ ${ARCADE_JURASSIC_LAUNCH:-0} != 1 ]]; then kill -INT "$pid" 2>/dev/null || true; fi
wait "$pid"
