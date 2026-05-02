#!/usr/bin/env bash
# Build the layout bench with debug symbols, wipe old perf data, and
# record a fresh profile.
#
# Outputs:
#   /tmp/palantir-perf.data         - perf record output
#   /tmp/palantir-perf-report.txt   - flat top-functions report
#
# Usage:
#   scripts/bench-perf.sh                   # default: --profile-time 5
#   scripts/bench-perf.sh --profile-time 2  # pass-through extra args

set -euo pipefail

cd "$(dirname "$0")/.."

PERF_DATA=/tmp/palantir-perf.data
PERF_REPORT=/tmp/palantir-perf-report.txt
EXTRA_ARGS=("$@")
if [ ${#EXTRA_ARGS[@]} -eq 0 ]; then
    EXTRA_ARGS=(--profile-time 5)
fi

if ! command -v perf >/dev/null 2>&1; then
    echo "error: perf not installed (try: sudo pacman -S perf)" >&2
    exit 1
fi

echo "==> Building bench with debug symbols"
CARGO_PROFILE_BENCH_DEBUG=line-tables-only \
    cargo bench --bench layout --no-run 2>&1 \
    | tail -3

BENCH=$(ls -t target/release/deps/layout-* | grep -v '\.d$' | head -1)
if [ -z "$BENCH" ] || [ ! -x "$BENCH" ]; then
    echo "error: could not locate built bench binary" >&2
    exit 1
fi
echo "    binary: $BENCH"

echo "==> Removing old perf data"
rm -f "$PERF_DATA" "$PERF_REPORT" "$PERF_DATA.old"

echo "==> Recording (perf record -F 1999 --call-graph dwarf)"
perf record -F 1999 --call-graph dwarf -o "$PERF_DATA" -- \
    "$BENCH" --bench "${EXTRA_ARGS[@]}"

echo "==> Writing flat report to $PERF_REPORT"
perf report -i "$PERF_DATA" --stdio --no-children -g none --percent-limit 1.0 \
    > "$PERF_REPORT"

echo
echo "==> Top hotspots:"
sed -n '/^# Samples.*cpu_core/,/^$/p' "$PERF_REPORT" | head -25

echo
echo "Full report: $PERF_REPORT"
echo "Raw data:    $PERF_DATA"
echo "Interactive: perf report -i $PERF_DATA"
