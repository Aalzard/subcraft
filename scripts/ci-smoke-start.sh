#!/bin/sh
# Start SubCraft once, headless, the way a user starts it, and fail unless it probes the GPU,
# opens its window, draws a frame, writes a screenshot and exits cleanly. Runs under Xvfb with
# software rendering (lavapipe / llvmpipe), i.e. a Linux machine without a usable GPU.
# Usage: scripts/ci-smoke-start.sh <subcraft binary> <output folder>
# The output folder receives screenshot.png, stdout/stderr and the data folder (startup.log,
# logs/, crashes/) so a failed run can be inspected from the CI artifact.
set -eu
exe="$1"
out="$2"
mkdir -p "$out"
export SUBCRAFT_DATA="$out/data"
export SUBCRAFT_LIBRARY="$out/library"
# Xvfb is an X server: never let winit look for a Wayland socket from the runner's environment.
unset WAYLAND_DISPLAY WAYLAND_SOCKET || true
status=0
timeout 300 xvfb-run -a -s "-screen 0 1600x1000x24" "$exe" --demo --screenshot "$out/screenshot.png" \
    > "$out/stdout.txt" 2> "$out/stderr.txt" || status=$?
echo "exit status: $status"
echo "--- stderr (tail)"; tail -n 40 "$out/stderr.txt" || true
echo "--- startup.log (tail)"; tail -n 40 "$out/data/startup.log" 2>/dev/null || echo "(no startup.log)"
if ls "$out/data/crashes"/* >/dev/null 2>&1; then
    echo "--- crash report"; cat "$out/data/crashes"/*
    echo "SubCraft wrote a crash report" >&2
    exit 1
fi
[ "$status" -eq 0 ] || { echo "SubCraft exited with status $status" >&2; exit 1; }
[ -s "$out/screenshot.png" ] || { echo "no screenshot was written" >&2; exit 1; }
sig="$(head -c 8 "$out/screenshot.png" | od -An -tx1 | tr -d ' \n')"
[ "$sig" = "89504e470d0a1a0a" ] || { echo "screenshot.png is not a PNG (signature $sig)" >&2; exit 1; }
echo "SubCraft started, drew a frame and exited cleanly."
