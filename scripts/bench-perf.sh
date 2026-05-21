#!/usr/bin/env bash
# Build a criterion bench with debug symbols, wipe old perf data, and
# record a fresh profile alongside hardware-counter aggregates.
#
# Pinned to a single P-core (CPU 0) on the i9-13980HX hybrid layout so
# E-cores don't skew counters or sampling. All hardware events use the
# `cpu_core/.../` PMU prefix — generic `-e cycles` would auto-expand
# across cpu_core + cpu_atom and report half-counts on a pinned run.
#
# Outputs (all in tmp/, gitignored):
#   tmp/palantir-perf.data         - perf record output (cycles, callgraph)
#   tmp/palantir-perf-report.txt   - flat top-functions report
#   tmp/palantir-perf-stat.txt     - perf stat counters (IPC, cache, branch)
#   tmp/palantir-perf-topdown.txt  - TMA L1 microarchitectural buckets
#   tmp/palantir-perf-mem.txt      - load-latency sample report (cache levels)
#
# Usage:
#   scripts/bench-perf.sh                              # default: frame bench, --profile-time 5
#   scripts/bench-perf.sh --profile-time 2             # override criterion args
#   BENCH=frame FILTER=frame/post_record scripts/bench-perf.sh
#   CALLGRAPH=lbr scripts/bench-perf.sh                # LBR (32-entry, low overhead)
#   SKIP_MEM=1 scripts/bench-perf.sh                   # skip perf mem pass
#   SKIP_TOPDOWN=1 scripts/bench-perf.sh               # skip TMA pass
#
# Env:
#   BENCH      bench target name from Cargo.toml (default: frame)
#   FILTER     criterion filter, prepended to bench args (default: empty = all)
#   FEATURES   cargo features, comma-separated (default: internals)
#   CALLGRAPH  dwarf (default, full depth, ~5-10x overhead) or lbr (32 frames, ~native)
#   LDLAT      perf mem load-latency cycles cutoff (default 50)
#   SKIP_MEM   set non-empty to skip the `perf mem` load-latency pass
#   SKIP_TOPDOWN  set non-empty to skip the `perf stat -M TopdownL1` pass
#
# Workflow (top-down per Intel TMA cookbook + perfwiki):
#   1. Read tmp/palantir-perf-topdown.txt. The dominant bucket
#      (frontend / backend / bad_spec / retiring) decides where to drill.
#      Healthy retiring >50%; backend_bound >40% with memory_bound
#      dominant → cache/DRAM; core_bound → port pressure / dep chains;
#      frontend_bound >20% → icache/uop-cache; bad_speculation >10%
#      → branch mispredicts. Each TMA leaf prints a "Sampling events:"
#      hint — feed it into `perf record -e <event>` + perf annotate.
#   2. Read tmp/palantir-perf-stat.txt for IPC + cache/branch/TLB MPKI.
#      IPC <1.0 on a P-core means stalled; >2.0 healthy; 4-5 peak.
#   3. Read tmp/palantir-perf-mem.txt to attribute load stalls to
#      cache levels (L1/L2/L3/LFB/Local-RAM). Source-line resolved.
#   4. Drill: perf report -i tmp/palantir-perf.data (TUI), then
#      perf annotate -i tmp/palantir-perf.data <symbol> for hot insns.
#
# For allocations (the project's "alloc-free per frame after warmup"
# claim), use the `alloc_free` / `alloc_free_gpu` benches with
# DHAT_DUMP=1 — perf only sees CPU time inside the allocator, not
# allocation counts.

set -euo pipefail

cd "$(dirname "$0")/.."
mkdir -p tmp

PERF_DATA=tmp/palantir-perf.data
PERF_REPORT=tmp/palantir-perf-report.txt
PERF_STAT=tmp/palantir-perf-stat.txt
PERF_TOPDOWN=tmp/palantir-perf-topdown.txt
PERF_MEM_DATA=tmp/palantir-perf-mem.data
PERF_MEM=tmp/palantir-perf-mem.txt

BENCH_NAME="${BENCH:-frame}"
FILTER_ARG="${FILTER:-}"
FEATURES_ARG="${FEATURES:-internals}"
CALLGRAPH_MODE="${CALLGRAPH:-dwarf}"
LDLAT_CYCLES="${LDLAT:-50}"
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
PERF_FREQ=5000

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
CARGO_BUILD_ARGS=(--bench "$BENCH_NAME")
if [ -n "$FEATURES_ARG" ]; then
    CARGO_BUILD_ARGS+=(--features "$FEATURES_ARG")
    echo "    features: $FEATURES_ARG"
fi
CARGO_PROFILE_BENCH_DEBUG=line-tables-only \
    cargo bench "${CARGO_BUILD_ARGS[@]}" --no-run 2>&1 \
    | tail -3

BENCH_BIN=$(ls -t "target/release/deps/${BENCH_NAME}"-* 2>/dev/null | grep -v '\.d$' | head -1)
if [ -z "$BENCH_BIN" ] || [ ! -x "$BENCH_BIN" ]; then
    echo "error: could not locate built bench binary for '$BENCH_NAME'" >&2
    exit 1
fi
echo "    binary: $BENCH_BIN"
echo "    pinned to CPU $PIN_CPU (P-core, cpu_core PMU)"
echo "    callgraph: $CALLGRAPH_MODE"
if [ -n "$FILTER_ARG" ]; then
    echo "    filter: $FILTER_ARG"
fi

echo "==> Removing old perf data"
rm -f "$PERF_DATA" "$PERF_REPORT" "$PERF_STAT" "$PERF_TOPDOWN" \
      "$PERF_MEM_DATA" "$PERF_MEM" "$PERF_DATA.old"

# Hardware events, P-core PMU explicit. Keep under 8 general counters
# to avoid multiplexing — measurement coverage [%] in the output should
# read 100.00%. dTLB-load-misses is the cheapest "working set too big
# for TLB reach" signal; cache-misses + L1-dcache-load-misses together
# bracket L2 vs DRAM stalls.
HW_EVENTS="cpu_core/cycles/,cpu_core/instructions/,cpu_core/branches/,cpu_core/branch-misses/,cpu_core/cache-references/,cpu_core/cache-misses/,cpu_core/L1-dcache-load-misses/,cpu_core/dTLB-load-misses/"
# Software counters in a separate group so they don't displace HW slots.
SW_EVENTS="task-clock,context-switches,cpu-migrations,page-faults"

echo "==> perf stat (hardware counters, cpu_core PMU)"
taskset -c "$PIN_CPU" \
    perf stat -e "$HW_EVENTS" -e "$SW_EVENTS" -o "$PERF_STAT" -- \
    "$BENCH_BIN" "${BENCH_ARGS[@]}" >/dev/null 2>&1 || true

if [ -z "${SKIP_TOPDOWN:-}" ]; then
    # Don't pass --cpu here: on hybrid CPUs perf tries to attach the
    # cpu_atom variants of the topdown events to whatever --cpu names,
    # and on a P-core target that fails the whole group with "no
    # supported events found." taskset alone pins; the cpu_atom rows
    # come back as "<not counted>" and the cpu_core metrics resolve.
    echo "==> perf stat -M TopdownL1 (TMA microarchitectural buckets)"
    taskset -c "$PIN_CPU" \
        perf stat -M TopdownL1 -o "$PERF_TOPDOWN" -- \
        "$BENCH_BIN" "${BENCH_ARGS[@]}" >/dev/null 2>&1 || \
        echo "    (TopdownL1 metric group unavailable — kernel too old or PMU access denied)"
fi

# Record options. LBR is 32 frames deep, near-native overhead, no need
# for frame pointers — great when the call stack stays shallow. DWARF
# walks .eh_frame from a stack-dump per sample (16 KiB here) — gives
# full depth and Just Works on Rust release builds, costs 5-10x.
case "$CALLGRAPH_MODE" in
    lbr)
        CG_ARG=(--call-graph lbr)
        ;;
    dwarf|*)
        CG_ARG=(--call-graph dwarf,16384)
        ;;
esac

echo "==> perf record (-F $PERF_FREQ ${CG_ARG[*]})"
taskset -c "$PIN_CPU" \
    perf record -F "$PERF_FREQ" "${CG_ARG[@]}" \
    -e cpu_core/cycles/ -o "$PERF_DATA" -- \
    "$BENCH_BIN" "${BENCH_ARGS[@]}"

echo "==> Writing flat report to $PERF_REPORT"
perf report -i "$PERF_DATA" --stdio --no-children -g none --percent-limit 1.0 \
    > "$PERF_REPORT"

if [ -z "${SKIP_MEM:-}" ]; then
    # perf mem uses PEBS load-latency sampling. ldlat=50 means "only
    # capture loads that took >=50 cycles" — filters L1 hits, keeps
    # L2/L3/DRAM misses. Sorted by symbol so callsites group together.
    echo "==> perf mem record (load-latency PEBS, ldlat=$LDLAT_CYCLES)"
    if taskset -c "$PIN_CPU" \
        perf mem record -t load --ldlat="$LDLAT_CYCLES" \
        -o "$PERF_MEM_DATA" -- \
        "$BENCH_BIN" "${BENCH_ARGS[@]}" >/dev/null 2>&1; then
        perf mem report -i "$PERF_MEM_DATA" --stdio \
            --sort=mem,sym,dso --percent-limit 1.0 > "$PERF_MEM" 2>/dev/null || \
            echo "    (perf mem report failed)"
    else
        echo "    (perf mem record unavailable — needs PEBS support + kernel.perf_event_paranoid <= 1)"
    fi
fi

echo
echo "==> Top hotspots:"
sed -n '/^# Samples/,/^$/p' "$PERF_REPORT" | head -30

echo
echo "==> Hardware counters:"
sed -n '/Performance counter stats/,$p' "$PERF_STAT"

if [ -f "$PERF_TOPDOWN" ]; then
    echo
    echo "==> Topdown L1:"
    sed -n '/Performance counter stats/,$p' "$PERF_TOPDOWN"
fi

if [ -f "$PERF_MEM" ]; then
    echo
    echo "==> Memory load-latency (top by cache level):"
    head -40 "$PERF_MEM"
fi

echo
echo "Hot-paths report : $PERF_REPORT"
echo "Counter report   : $PERF_STAT"
echo "Topdown report   : $PERF_TOPDOWN"
echo "Mem-latency rpt  : $PERF_MEM"
echo "Raw data         : $PERF_DATA"
echo "Interactive      : perf report -i $PERF_DATA"
echo "Annotate symbol  : perf annotate -i $PERF_DATA -M intel <symbol>"
