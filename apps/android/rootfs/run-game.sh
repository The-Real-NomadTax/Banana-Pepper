#!/bin/bash
# Generic X11/frame/audio transport. Game policy is supplied by the Rust plan.
set -u
S=/run/af
export HOME=${HOME:-/tmp} DISPLAY=:0 LIBGL_ALWAYS_SOFTWARE=1 GALLIUM_DRIVER=llvmpipe
export PATH=/opt/arcade:$PATH
export BOX64_LOG=1 BOX64_NOBANNER=0 BOX64_ANDROID_NOEXEC=1
export ARCADE_AUDIO_SOCKET=${AF_AUDIO_SOCKET:-}
[ -n "$ARCADE_AUDIO_SOCKET" ] || export ARCADE_BBH_AUDIO=0
mkdir -p /tmp/.X11-unix
rm -f /tmp/.X0-lock /tmp/.X11-unix/X0
xpid= bpid= gpid= wpid= ppid= apid=
cleanup() {
    kill $gpid $bpid $xpid $apid $ppid 2>/dev/null || true
    # When the app is gone, leave its watchdog alive to force-stop the group.
    if [ -z "${AF_APP_PID:-}" ] || kill -0 "$AF_APP_PID" 2>/dev/null; then
        kill -KILL $wpid 2>/dev/null || true
    fi
}
trap cleanup EXIT INT TERM
if [ "${AF_PULSE_AUDIO:-0}" = 1 ]; then
    export PULSE_SERVER=unix:/tmp/pulse-native
    mkdir -p /tmp/pulse-home /tmp/pulse-runtime
    chmod 700 /tmp/pulse-home /tmp/pulse-runtime
    rm -f /tmp/pulse-native
    HOME=/tmp/pulse-home XDG_RUNTIME_DIR=/tmp/pulse-runtime pulseaudio -n \
        --daemonize=no --disable-shm=yes --use-pid-file=no --exit-idle-time=-1 \
        --high-priority=no --realtime=no --log-target=stderr \
        --load="module-native-protocol-unix socket=/tmp/pulse-native auth-anonymous=1" \
        --load="module-null-sink sink_name=arcade rate=48000 channels=2" \
        > "$S/pulse.log" 2>&1 &
    ppid=$!
    for i in $(seq 1 100); do
        [ -S /tmp/pulse-native ] && break
        kill -0 "$ppid" 2>/dev/null || { echo 'Audio server failed; see pulse.log'; exit 1; }
        sleep 0.1
    done
    [ -S /tmp/pulse-native ] || { echo 'Audio server timed out'; exit 1; }
    if [ -n "${AF_AUDIO_SOCKET:-}" ]; then
        afaudio "$AF_AUDIO_SOCKET" > "$S/audio.log" 2>&1 &
        apid=$!
    fi
fi
resolution=${ARCADE_RESOLUTION:-640x480}
if [[ ! "$resolution" =~ ^([0-9]{3,4})x([0-9]{3,4})$ ]] \
    || (( 10#${BASH_REMATCH[1]} > 1920 || 10#${BASH_REMATCH[2]} > 1080 )); then
    echo 'Invalid runtime display size'; exit 1
fi
Xvfb :0 -screen 0 "${resolution}x24" -nolisten tcp -nolisten local -ac +iglx -nocursor > "$S/xvfb.log" 2>&1 &
xpid=$!
ready=0
for i in $(seq 1 100); do
    if xset q >/dev/null 2>&1; then ready=1; break; fi
    kill -0 "$xpid" 2>/dev/null || { echo 'X server failed; see xvfb.log'; exit 1; }
    sleep 0.1
done
[ "$ready" = 1 ] || { echo 'X server timed out'; exit 1; }
afbridge "$S/frame.bin" "$AF_INPUT_SOCKET" "${AF_FRAME_FPS:-60}" > "$S/bridge.log" 2>&1 &
bpid=$!
"$@" > "$S/game.log" 2>&1 &
gpid=$!
if [ -n "${AF_APP_PID:-}" ]; then
    (
        trap '' TERM
        while kill -0 "$AF_APP_PID" 2>/dev/null; do sleep 1; done
        group=$(cat "$S/host-session.pid" 2>/dev/null || true)
        if [[ "$group" =~ ^[0-9]+$ ]] && (( group > 1 )); then
            # Wine can start its own sessions. They still belong to this proot
            # tracer. Leave Bash (including this watchdog) for the group kill.
            for status in /proc/[0-9]*/status; do
                if grep -q "^TracerPid:[[:space:]]*$group$" "$status" 2>/dev/null \
                    && ! grep -q '^Name:[[:space:]]*bash$' "$status" 2>/dev/null; then
                    target=${status#/proc/}; target=${target%/status}
                    kill -KILL "$target" 2>/dev/null || true
                fi
            done
            kill -KILL -- "-$group" 2>/dev/null || true
        else
            kill "$gpid" "$bpid" "$xpid" "$apid" "$ppid" 2>/dev/null || true
        fi
    ) &
    wpid=$!
fi
wait "$gpid"
status=$?
echo "game exited with status $status" >> "$S/game.log"
exit "$status"
