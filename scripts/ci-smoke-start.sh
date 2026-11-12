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
# Linux: run under Xvfb (an X server), and never let winit look for a Wayland socket from the
# runner's environment. macOS: the window server is always there, so run the binary directly.
display=""
if [ "$(uname -s)" = Linux ]; then
    unset WAYLAND_DISPLAY WAYLAND_SOCKET || true
    display="xvfb-run -a -s \"-screen 0 1600x1000x24\""
fi
# A time limit: GNU timeout on Linux, gtimeout (coreutils) if installed on macOS, else perl.
if command -v timeout >/dev/null 2>&1; then limit="timeout 300"
elif command -v gtimeout >/dev/null 2>&1; then limit="gtimeout 300"
else limit="perl -e 'alarm shift; exec @ARGV' 300"
fi
status=0
eval "$limit $display \"\$exe\" --demo --screenshot \"\$out/screenshot.png\"" \
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
