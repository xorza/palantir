#!/usr/bin/env bash
# Profile the criterion bench with perf in five passes — counters,
# microarch metrics, callgraph, precise IP, memory data source — into
# tmp/palantir-perf*. benches/profiling.md says how to read them.
#
#   benches/bench-perf.sh                               # the CPU frame cases
#   FILTER='^integration/damage/' benches/bench-perf.sh # criterion's regex over case ids
#   FILTER='/cpu/' benches/bench-perf.sh                # every case that opens no device
#   SKIP='micro mem' benches/bench-perf.sh              # of counters, micro, callgraph, precise, mem
#   benches/bench-perf.sh --exact                       # further args go to the bench
#
# Env: FILTER (^integration/frame/cpu/), PROFILE_TIME (5 s per case),
# FEATURES (beside `internals`, which a bench build has), PIN_CPU (2), CALLGRAPH (dwarf, or lbr on
# Intel), SKIP.

set -uo pipefail
cd "$(dirname "$0")/.."

case "${1:-}" in
    -h | --help)
        sed -n '2,/^$/s/^# \{0,1\}//p' "$0"
        exit 0
        ;;
esac

die() {
    echo "error: $*" >&2
    exit 1
}
note() { echo "    NOTE: $*" >&2; }

FILTER="${FILTER:-^integration/frame/cpu/}"
PIN_CPU="${PIN_CPU:-2}"
FEATURES="${FEATURES:-}"
PROFILE_TIME="${PROFILE_TIME:-5}"
CALLGRAPH="${CALLGRAPH:-dwarf}"
PASSES=(counters micro callgraph precise mem)

# Every setting is checked before the build, so a typo costs no fat-LTO link.
for pass in ${SKIP:-}; do
    [[ " ${PASSES[*]} " == *" $pass "* ]] || die "SKIP names '$pass', not one of: ${PASSES[*]}"
done
skipped() { [[ " ${SKIP:-} " == *" $1 "* ]]; }
[[ "$CALLGRAPH" == dwarf || "$CALLGRAPH" == lbr ]] || die "CALLGRAPH is '$CALLGRAPH', not dwarf or lbr"
[[ "$PIN_CPU" =~ ^[0-9]+$ && -r /sys/devices/system/cpu/cpu$PIN_CPU/topology/thread_siblings_list ]] ||
    die "PIN_CPU is '$PIN_CPU', not an online CPU"
[[ "$PROFILE_TIME" =~ ^[0-9]+([.][0-9]+)?$ ]] || die "PROFILE_TIME is '$PROFILE_TIME', not seconds"
for tool in perf taskset setarch; do
    command -v "$tool" >/dev/null || die "$tool not installed"
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
[ -n "$PERF_CPUS" ] || die "no CPU left for perf"
for sib in $SIBLINGS; do
    [ "$sib" != "$PIN_CPU" ] && note "keep cpu$sib idle — it shares a core with cpu$PIN_CPU"
done

# The `bench` profile keeps the line tables and symbols perf reads, so this
# builds the binary `cargo bench` builds and neither rebuilds after the other.
# Its JSON artifact names the binary by an absolute path, on a fresh build too
# and under any build directory; cargo's human output does neither.
echo "==> Building (features: internals${FEATURES:+,$FEATURES})"
BUILD_LOG=$(mktemp)
trap 'rm -f "$BUILD_LOG"' EXIT
ARTIFACTS=$(cargo bench --bench palantir ${FEATURES:+--features "$FEATURES"} --no-run \
    --message-format=json-render-diagnostics 2>"$BUILD_LOG") || {
    cat "$BUILD_LOG" >&2
    exit 1
}
BIN=$(grep '"kind":\["bench"\]' <<<"$ARTIFACTS" | sed -n 's/.*"executable":"\([^"]*\)".*/\1/p' | tail -1)
[ -x "$BIN" ] || die "cargo named no bench binary"

# A filter that matches nothing would leave every capture empty, and arguments
# the bench refuses would fail every pass.
LIST=$("$BIN" --bench --list "$FILTER" "$@" 2>&1) ||
    die "the bench refused its arguments:"$'\n'"$(tail -n 5 <<<"$LIST")"
CASES=$(grep -c ': benchmark$' <<<"$LIST")
[ "$CASES" -gt 0 ] || die "FILTER '$FILTER' matches no case; list them with: $BIN --bench --list"
echo "    $BIN on cpu$PIN_CPU, perf on $PERF_CPUS"
echo "    $CASES cases of $FILTER, $PROFILE_TIME s each a pass"

mkdir -p tmp
OUT=tmp/palantir-perf
BENCH_LOG=$OUT-bench.log
rm -f "$OUT"*

# A fixed window keeps sample counts comparable between captures. A `cpu`
# case opens no device, so the capture holds no driver threads.
BENCH_ARGS=(--bench --profile-time "$PROFILE_TIME" "$FILTER" "$@")

# `run <pass> <perf command…>`. `setarch -R` fixes the address layout, so
# alignment does not vary between runs. The bench writes to BENCH_LOG, whose
# end a failed pass shows.
run() {
    echo "==> $1" >>"$BENCH_LOG"
    shift
    taskset -c "$PERF_CPUS" "$@" -- taskset -c "$PIN_CPU" setarch -R "$BIN" "${BENCH_ARGS[@]}" \
        >>"$BENCH_LOG" 2>&1
}
failed() {
    echo "    ($1 failed; the end of $BENCH_LOG:)"
    tail -n 5 "$BENCH_LOG" | sed 's/^/      /'
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

if ! skipped counters; then
    echo "==> counters"
    if [ "$ARCH" = intel ]; then
        run counters perf stat -o "$OUT-stat.txt" -e task-clock,context-switches,page-faults \
            -e "cpu_core/cycles/,cpu_core/instructions/,cpu_core/branches/,cpu_core/branch-misses/,cpu_core/cache-references/,cpu_core/cache-misses/,cpu_core/L1-dcache-load-misses/,cpu_core/dTLB-load-misses/"
    else
        run counters perf stat -d -o "$OUT-stat.txt"
    fi || failed counters
fi

if ! skipped micro; then
    # Two small groups, so AMD's six counters do not multiplex.
    METRICS=branch_prediction,tlb
    [ "$ARCH" = intel ] && METRICS=TopdownL1
    [ "$ARCH" = amd ] && perf list metricgroups 2>/dev/null | grep -qiE 'pipeline_util|topdown' &&
        METRICS=Pipeline_Util_Level1
    echo "==> microarch ($METRICS)"
    run microarch perf stat -M "$METRICS" -o "$OUT-micro.txt" || failed microarch
fi

if ! skipped callgraph; then
    # DWARF unwinds from a copy of the stack, which must hold all of it: at
    # 16 KiB almost no sample reached `main`. 64 KiB a sample is why the rate
    # is low and the file compressed.
    CG=(--call-graph dwarf,65528)
    if [ "$CALLGRAPH" = lbr ]; then
        [ "$ARCH" = intel ] && CG=(--call-graph lbr) || note "lbr is Intel-only — using dwarf"
    fi
    echo "==> callgraph (${CG[*]})"
    run callgraph perf record -z -F 499 "${CG[@]}" -e "$CYCLES" -o "$OUT.data" &&
        report "$OUT.data" "$OUT-report.txt" || failed callgraph
fi

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
        run "precise IP" perf record "${PRECISE[@]}" -o "$OUT-ibs.data" &&
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
        run "perf mem" perf mem "${MEM[@]}" -o "$OUT-mem.data" &&
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
Bench log: $BENCH_LOG
Callgraph: perf report -i $OUT.data
Annotate : perf annotate -i $OUT-ibs.data <symbol>
Reading  : benches/profiling.md
EOF
