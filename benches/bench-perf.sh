#!/usr/bin/env bash
# Profile the criterion bench with perf in five passes — counters,
# microarch metrics, callgraph, precise IP, memory data source — into
# tmp/palantir-perf*. benches/profiling.md says how to read them.
#
#   benches/bench-perf.sh                          # the frame driver
#   DRIVER='damage cascade' benches/bench-perf.sh  # exact driver names
#   DRIVER= benches/bench-perf.sh                  # every default driver
#   SKIP='micro mem' benches/bench-perf.sh         # of micro, precise, mem
#   benches/bench-perf.sh 'workload$'              # further args go to the bench
#
# Env: DRIVER (frame), ARMS (cpu), PROFILE_TIME (5 s per benchmark),
# FEATURES (added to `bench`), PIN_CPU (2), CALLGRAPH (dwarf, or lbr on
# Intel), SKIP.

set -uo pipefail
cd "$(dirname "$0")/.."
mkdir -p tmp
OUT=tmp/palantir-perf

# `-`, not `:-`: an empty DRIVER means every default driver.
DRIVERS="${DRIVER-frame}"
PIN_CPU="${PIN_CPU:-2}"
FEATURES="bench${FEATURES:+,$FEATURES}"
skipped() { [[ " ${SKIP:-} " == *" $1 "* ]]; }
note() { echo "    NOTE: $*" >&2; }
failed() { echo "    ($1 failed)"; }

# `--bench` makes the runner parse argv itself. A fixed window keeps
# sample counts comparable between captures, and a profiled run records
# no results row, so the frame driver needs no `--note`.
BENCH_ARGS=(--bench --arms "${ARMS:-cpu}" --profile-time "${PROFILE_TIME:-5}")
for d in $DRIVERS; do BENCH_ARGS+=(--driver "$d"); done
BENCH_ARGS+=("$@")

for tool in perf taskset setarch; do
    command -v "$tool" >/dev/null || { echo "error: $tool not installed" >&2; exit 1; }
done

case "$(awk -F': ' '/^vendor_id/{print $2; exit}' /proc/cpuinfo)" in
    AuthenticAMD) ARCH=amd ;;
    GenuineIntel) ARCH=intel ;;
    *) ARCH=generic ;;
esac
echo "==> CPU: $(awk -F': ' '/^model name/{print $2; exit}' /proc/cpuinfo) [$ARCH]"

# Each of these thins a pass without failing it.
[ "$(cat /proc/sys/kernel/perf_event_paranoid)" -gt -1 ] &&
    note "precise IP and mem need: sudo sysctl kernel.perf_event_paranoid=-1"
[ "$(cat /proc/sys/kernel/nmi_watchdog 2>/dev/null || echo 0)" != 0 ] &&
    note "the NMI watchdog holds a counter: sudo sysctl kernel.nmi_watchdog=0"
CPUFREQ=/sys/devices/system/cpu/cpu$PIN_CPU/cpufreq
cat "$CPUFREQ/scaling_governor" "$CPUFREQ/energy_performance_preference" 2>/dev/null |
    grep -qvx performance &&
    note "cpu$PIN_CPU governor or EPP is not 'performance' — the clock will vary"

# The bench gets PIN_CPU, perf every other CPU but PIN_CPU's SMT sibling.
expand_cpus() {
    local part
    for part in ${1//,/ }; do
        case "$part" in
            *-*) seq "${part%-*}" "${part#*-}" ;;
            *) echo "$part" ;;
        esac
    done
}
SIBLINGS=$(expand_cpus "$(cat "/sys/devices/system/cpu/cpu$PIN_CPU/topology/thread_siblings_list")")
PERF_CPUS=$(expand_cpus "$(cat /sys/devices/system/cpu/online)" | grep -vxF "$SIBLINGS" | paste -sd, -)
[ -n "$PERF_CPUS" ] || { echo "error: no CPU left for perf" >&2; exit 1; }
for sib in $SIBLINGS; do
    [ "$sib" != "$PIN_CPU" ] && note "keep cpu$sib idle — it shares a core with cpu$PIN_CPU"
done

# The environment repeats palantir's `[profile.bench]` debug settings, with
# the same values: cargo ignores that profile inside an enclosing workspace,
# and the environment keeps the symbols and line tables perf reads there.
# Standalone the profile cargo resolves is unchanged, so `cargo bench` and
# this script build one binary and neither rebuilds after the other. Keep
# the values in step with `Cargo.toml`.
echo "==> Building (features: $FEATURES)"
if ! LOG=$(CARGO_PROFILE_BENCH_DEBUG=line-tables-only CARGO_PROFILE_BENCH_STRIP=none \
    CARGO_PROFILE_BENCH_SPLIT_DEBUGINFO=off \
    cargo bench --bench criterion --features "$FEATURES" --no-run 2>&1); then
    echo "$LOG" >&2
    exit 1
fi
# Cargo names the binary on a fresh build too, relative to the workspace
# root.
BIN=$(dirname "$(cargo locate-project --workspace --message-format plain)")/$(
    echo "$LOG" | sed -n 's/.*Executable .*(\(.*criterion-[0-9a-f]*\))$/\1/p' | tail -1)
[ -f "$BIN" ] || { echo "error: cargo named no criterion binary" >&2; exit 1; }
echo "    $BIN on cpu$PIN_CPU, perf on $PERF_CPUS, drivers: ${DRIVERS:-<default>}"

rm -f "$OUT"*

# `setarch -R` fixes the address layout, so alignment does not vary
# between runs.
run() {
    taskset -c "$PERF_CPUS" "$@" -- taskset -c "$PIN_CPU" setarch -R "$BIN" "${BENCH_ARGS[@]}" \
        >/dev/null 2>&1
}

# perf does not demangle Rust's v0 symbols.
if command -v rustfilt >/dev/null; then
    demangle() { rustfilt; }
else
    note "no rustfilt on PATH — symbols stay mangled"
    demangle() { cat; }
fi
report() {
    perf report -i "$1" --stdio --no-children -g none --percent-limit 1.0 2>/dev/null |
        demangle >"$2"
}

# Intel hybrid: a bare event counts on both PMUs, each its own share.
CYCLES=cycles
[ "$ARCH" = intel ] && CYCLES=cpu_core/cycles/

echo "==> counters"
if [ "$ARCH" = intel ]; then
    run perf stat -o "$OUT-stat.txt" -e task-clock,context-switches,page-faults \
        -e "cpu_core/cycles/,cpu_core/instructions/,cpu_core/branches/,cpu_core/branch-misses/,cpu_core/cache-references/,cpu_core/cache-misses/,cpu_core/L1-dcache-load-misses/,cpu_core/dTLB-load-misses/"
else
    run perf stat -d -o "$OUT-stat.txt"
fi || failed counters

if ! skipped micro; then
    # Two small groups, so AMD's six counters do not multiplex.
    METRICS=branch_prediction,tlb
    [ "$ARCH" = intel ] && METRICS=TopdownL1
    [ "$ARCH" = amd ] && perf list metricgroups 2>/dev/null | grep -qiE 'pipeline_util|topdown' &&
        METRICS=Pipeline_Util_Level1
    echo "==> microarch ($METRICS)"
    run perf stat -M "$METRICS" -o "$OUT-micro.txt" || failed microarch
fi

# DWARF unwinds from a copy of the stack, which must hold all of it: at
# 16 KiB almost no sample reached `main`. 64 KiB a sample is why the rate
# is low and the file compressed.
CG=(--call-graph dwarf,65528)
if [ "${CALLGRAPH:-dwarf}" = lbr ]; then
    [ "$ARCH" = intel ] && CG=(--call-graph lbr) || note "lbr is Intel-only — using dwarf"
fi
echo "==> callgraph (${CG[*]})"
run perf record -z -F 499 "${CG[@]}" -e "$CYCLES" -o "$OUT.data" &&
    report "$OUT.data" "$OUT-report.txt" || failed callgraph

HAVE_IBS=$([ -d /sys/bus/event_source/devices/ibs_op ] && echo 1)
if ! skipped precise; then
    # Cycle sampling skids past the costly instruction; IBS and PEBS do
    # not.
    case "$ARCH" in
        amd) PRECISE=(${HAVE_IBS:+-e ibs_op// -c 250000}) ;;
        intel) PRECISE=(-e "${CYCLES}ppp" -F 4000) ;;
        *) PRECISE=() ;;
    esac
    if [ ${#PRECISE[@]} -eq 0 ]; then
        echo "==> (no precise IP on this CPU)"
    else
        echo "==> precise IP (${PRECISE[*]})"
        run perf record "${PRECISE[@]}" -o "$OUT-ibs.data" &&
            report "$OUT-ibs.data" "$OUT-ibs.txt" || failed "precise IP"
    fi
fi

if ! skipped mem; then
    # A load-latency filter on AMD needs Zen5's IBS.
    case "$ARCH" in
        amd) MEM=(${HAVE_IBS:+record}) ;;
        intel) MEM=(record -t load --ldlat=50) ;;
        *) MEM=() ;;
    esac
    if [ ${#MEM[@]} -eq 0 ]; then
        echo "==> (no perf mem on this CPU)"
    else
        echo "==> memory data source"
        run perf mem "${MEM[@]}" -o "$OUT-mem.data" &&
            perf mem report -i "$OUT-mem.data" --stdio --sort=mem,sym,dso --percent-limit 1.0 \
                2>/dev/null | demangle >"$OUT-mem.txt" || failed "perf mem"
    fi
fi

# `top <file> <title> <first line> <lines>`
top() { [ -f "$1" ] && { echo; echo "==> $2"; sed -n "/$3/,\$p" "$1" | head -"$4"; }; }
top "$OUT-report.txt" "Self time (callgraph pass)" '^# Samples' 28
top "$OUT-stat.txt" "Counters" 'Performance counter stats' 9999
top "$OUT-ibs.txt" "Self time (precise IP)" '^# Samples' 16
top "$OUT-micro.txt" "Microarch" 'Performance counter stats' 40
top "$OUT-mem.txt" "Memory data source" '^# Overhead' 30

cat <<EOF

Reports  : $OUT-{report,stat,micro,ibs,mem}.txt
Callgraph: perf report -i $OUT.data
Annotate : perf annotate -i $OUT-ibs.data <symbol>
Reading  : benches/profiling.md
EOF
