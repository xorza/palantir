#!/usr/bin/env bash
# Build a criterion bench with debug symbols, wipe old perf data, and
# record a fresh profile alongside hardware-counter aggregates.
#
# Pinned to a single P-core (CPU 0) on the i9-13980HX hybrid layout so
# E-cores don't skew counters or sampling.
#
# Outputs:
#   /tmp/palantir-perf.data         - perf record output (cycles, dwarf callgraph)
#   /tmp/palantir-perf-report.txt   - flat top-functions report
#   /tmp/palantir-perf-stat.txt     - perf stat counters (IPC, cache, branch, faults)
#
# Usage:
#   scripts/bench-perf.sh                              # default: layout bench, --profile-time 5
#   scripts/bench-perf.sh --profile-time 2             # override criterion args
#   BENCH=frame FILTER=frame/end_frame_resizing scripts/bench-perf.sh
#
# Env:
#   BENCH   bench target name from Cargo.toml (default: layout)
#   FILTER  criterion filter, prepended to bench args (default: empty = all)
#
# For allocations (the project's "alloc-free per frame after warmup" claim),
# use dhat instead — perf isn't well-suited:
#   cargo install dhat-heap   (then wire dhat::Profiler in the bench)
# or run heaptrack against the bench binary directly.

set -euo pipefail

cd "$(dirname "$0")/.."

PERF_DATA=/tmp/palantir-perf.data
PERF_REPORT=/tmp/palantir-perf-report.txt
PERF_STAT=/tmp/palantir-perf-stat.txt
BENCH_NAME="${BENCH:-layout}"
FILTER_ARG="${FILTER:-}"
EXTRA_ARGS=("$@")
if [ ${#EXTRA_ARGS[@]} -eq 0 ]; then
    EXTRA_ARGS=(--profile-time 5)
fi
BENCH_ARGS=(--bench)
if [ -n "$FILTER_ARG" ]; then
    BENCH_ARGS+=("$FILTER_ARG")
fi
BENCH_ARGS+=("${EXTRA_ARGS[@]}")

# Sampling frequency. Cap is /proc/sys/kernel/perf_event_max_sample_rate
# (50000 on this box). 4999 gives ~2.5x the previous data density without
# tripping the throttle. Raise via sysctl if you need more.
PERF_FREQ=4999

# Pin to P-core 0. cpu_core covers 0-15; cpu_atom covers 16-31.
PIN_CPU=0

if ! command -v perf >/dev/null 2>&1; then
    echo "error: perf not installed (try: sudo pacman -S perf)" >&2
    exit 1
fi

if ! command -v taskset >/dev/null 2>&1; then
    echo "error: taskset not installed (util-linux)" >&2
    exit 1
fi

echo "==> Building bench '$BENCH_NAME' with debug symbols"
CARGO_PROFILE_BENCH_DEBUG=line-tables-only \
    cargo bench --bench "$BENCH_NAME" --no-run 2>&1 \
    | tail -3

BENCH_BIN=$(ls -t "target/release/deps/${BENCH_NAME}"-* 2>/dev/null | grep -v '\.d$' | head -1)
if [ -z "$BENCH_BIN" ] || [ ! -x "$BENCH_BIN" ]; then
    echo "error: could not locate built bench binary for '$BENCH_NAME'" >&2
    exit 1
fi
echo "    binary: $BENCH_BIN"
echo "    pinned to CPU $PIN_CPU (P-core)"
if [ -n "$FILTER_ARG" ]; then
    echo "    filter: $FILTER_ARG"
fi

echo "==> Removing old perf data"
rm -f "$PERF_DATA" "$PERF_REPORT" "$PERF_STAT" "$PERF_DATA.old"

# perf stat events. Two groups: a wide hardware-counter group, and a
# software-counter group for context switches and page faults. Page
# faults are the cheapest "did we allocate?" proxy without instrumenting
# the allocator — non-zero page-faults during steady state means new
# pages got mapped (typically Vec::reserve crossing a page boundary).
HW_EVENTS="cycles,instructions,branches,branch-misses,cache-references,cache-misses,L1-dcache-loads,L1-dcache-load-misses,dTLB-loads,dTLB-load-misses"
SW_EVENTS="task-clock,context-switches,cpu-migrations,page-faults,minor-faults,major-faults"

echo "==> perf stat (hardware counters)"
taskset -c "$PIN_CPU" \
    perf stat -e "$HW_EVENTS" -e "$SW_EVENTS" -o "$PERF_STAT" -- \
    "$BENCH_BIN" "${BENCH_ARGS[@]}" >/dev/null 2>&1 || true

echo "==> perf record (-F $PERF_FREQ --call-graph dwarf,16384)"
taskset -c "$PIN_CPU" \
    perf record -F "$PERF_FREQ" --call-graph dwarf,16384 -o "$PERF_DATA" -- \
    "$BENCH_BIN" "${BENCH_ARGS[@]}"

echo "==> Writing flat report to $PERF_REPORT"
perf report -i "$PERF_DATA" --stdio --no-children -g none --percent-limit 1.0 \
    > "$PERF_REPORT"

echo
echo "==> Top hotspots:"
sed -n '/^# Samples.*cpu_core/,/^$/p' "$PERF_REPORT" | head -30

echo
echo "==> Hardware counters:"
# Strip the boilerplate header and summary, keep just the counter lines.
sed -n '/Performance counter stats/,$p' "$PERF_STAT"

echo
echo "Hot-paths report : $PERF_REPORT"
echo "Counter report   : $PERF_STAT"
echo "Raw data         : $PERF_DATA"
echo "Interactive      : perf report -i $PERF_DATA"
echo "Annotate symbol  : perf annotate -i $PERF_DATA <symbol>"
