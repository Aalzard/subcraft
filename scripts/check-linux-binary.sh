#!/bin/sh
# Print the glibc symbol versions a Linux binary needs (the oldest glibc that can run it) and the
# shared libraries it loads at start, and fail when the newest glibc version is above <max>.
# Usage: scripts/check-linux-binary.sh <binary> [max glibc version, e.g. 2.31]
set -eu
bin="$1"
max="${2:-}"
need="$(objdump -T "$bin" | grep -o 'GLIBC_[0-9][0-9.]*' | sed 's/GLIBC_//' | sort -uV)"
newest="$(printf '%s\n' "$need" | tail -n 1)"
echo "glibc symbol versions referenced: $(printf '%s' "$need" | tr '\n' ' ')"
echo "oldest glibc that can run $bin: $newest"
echo "shared libraries loaded at start:"
ldd "$bin" || true
if [ -n "$max" ] && [ "$(printf '%s\n%s\n' "$newest" "$max" | sort -V | tail -n 1)" != "$max" ]; then
    echo "$bin needs glibc $newest, newer than the allowed $max" >&2
    exit 1
fi
