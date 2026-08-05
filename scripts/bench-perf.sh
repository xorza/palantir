#!/usr/bin/env bash
# Profile a criterion bench with perf: flat callgraph report plus
# hardware-counter, microarch, precise-IP, and data-source passes.
#
# Vendor-aware — Intel needs the `cpu_core/…/` PMU prefix and gets TMA +
# PEBS; AMD has one homogeneous PMU and gets metric groups + IBS. The
# script detects `vendor_id` and picks the path.
#
#   scripts/bench-perf.sh                            # frame bench, 5s
#   DRIVER=damage scripts/bench-perf.sh              # a different driver
#   DRIVER='damage cascade' scripts/bench-perf.sh    # several
#   DRIVER= scripts/bench-perf.sh                    # every default driver
#   DRIVER=damage FILTER='workload$' scripts/bench-perf.sh
#   SKIP_MEM=1 SKIP_MICRO=1 SKIP_IBS=1 scripts/bench-perf.sh
#   scripts/bench-perf.sh --profile-time 2           # extra bench args
#
# Env: BENCH (target, default criterion — every criterion driver shares
# it, so pick one with DRIVER), DRIVER (space-separated driver names,
# default `frame`; empty = every non-opt-in driver), FILTER (criterion
# regex *within* the selected drivers, default none), ARMS
# (cpu|gpu|both, default cpu — a CPU sampler has nothing to say about
# the GPU arms), NOTE (frame-bench results caption — needed only on a
# recording run, which profiling is not), FEATURES (extra
# cargo features; `bench` is always on, every target requires it),
# CALLGRAPH (dwarf|lbr, Intel only), PIN_CPU (2), FREQ (4000),
# IBS_PERIOD (250000), LDLAT (50), SKIP_MEM / SKIP_MICRO / SKIP_IBS.
#
# DRIVER is exact selection, not a regex: an unnamed driver never runs,
# so it costs nothing and stays out of the profile. FILTER is criterion's
# own id regex and gates only the timing loop — the driver's setup has
# already run by then, which is why it is the wrong tool for picking one.
#
# Outputs land in tmp/ and are listed on exit. **benches/AGENTS.md is
# the manual** — which file to read in what order, what the numbers
# mean, and the Intel/AMD drill recipes.

set -uo pipefail

cd "$(dirname "$0")/.."
mkdir -p tmp

PERF_DATA=tmp/palantir-perf.data
PERF_REPORT=tmp/palantir-perf-report.txt
PERF_STAT=tmp/palantir-perf-stat.txt
PERF_MICRO=tmp/palantir-perf-micro.txt
PERF_IBS_DATA=tmp/palantir-perf-ibs.data
PERF_IBS=tmp/palantir-perf-ibs.txt
PERF_MEM_DATA=tmp/palantir-perf-mem.data
PERF_MEM=tmp/palantir-perf-mem.txt

BENCH_NAME="${BENCH:-criterion}"
# `-` not `:-`: an explicitly empty DRIVER means "every default driver",
# and must not fall back to the default the way an unset one does.
DRIVER_ARG="${DRIVER-frame}"
FILTER_ARG="${FILTER-}"
ARMS_ARG="${ARMS:-cpu}"
# `bench` is not optional — every target carries
# `required-features = ["bench"]` and cargo refuses the target without
# it. So it is always passed and FEATURES *adds* to it; naming some
# other feature must not silently leave nothing to profile.
FEATURES_ARG="bench${FEATURES:+,$FEATURES}"
CALLGRAPH_MODE="${CALLGRAPH:-dwarf}"
PIN_CPU="${PIN_CPU:-2}"
PERF_FREQ="${FREQ:-4000}"
IBS_PERIOD="${IBS_PERIOD:-250000}"
LDLAT_CYCLES="${LDLAT:-50}"

EXTRA_ARGS=("$@")
[ ${#EXTRA_ARGS[@]} -eq 0 ] && EXTRA_ARGS=(--profile-time 5)
# `--bench` is what tells the runner argv is ours rather than criterion's
# — without it the binary delegates and runs everything once in test
# mode, which profiles nothing. See src/bench/mod.rs.
BENCH_ARGS=(--bench --arms "$ARMS_ARG")
for d in $DRIVER_ARG; do
    BENCH_ARGS+=(--driver "$d")
done
[ -n "$FILTER_ARG" ] && BENCH_ARGS+=("$FILTER_ARG")
[ -n "${NOTE:-}" ] && BENCH_ARGS+=(--note "$NOTE")
BENCH_ARGS+=("${EXTRA_ARGS[@]}")

for tool in perf taskset; do
    command -v "$tool" >/dev/null 2>&1 || { echo "error: $tool not installed" >&2; exit 1; }
done

# The frame bench appends a captioned row to benches/results/ — but only
# on a recording run, and profiling writes no estimates to record. So the
# note is needed only if the caller replaced the default profile pass
# with real sampling. It panics for a missing one after the build, hence
# checking here: minutes of cargo before an avoidable panic is a bad
# trade. Matched without a trailing space to catch `--profile-time=N` too.
case " $DRIVER_ARG " in
    *" frame "*)
        case " ${BENCH_ARGS[*]} " in
            *" --profile-time"*) ;;
            *) [ -n "${NOTE:-}" ] || {
                   echo "error: a recording frame-bench run needs a caption for its results row." >&2
                   echo "       NOTE='after staging-belt rework' $0 ${EXTRA_ARGS[*]}" >&2
                   exit 1
               } ;;
        esac ;;
esac

# ── Vendor + capability detection ────────────────────────────────────
case "$(awk -F': ' '/^vendor_id/{print $2; exit}' /proc/cpuinfo)" in
    AuthenticAMD) ARCH=amd ;;
    GenuineIntel) ARCH=intel ;;
    *) ARCH=generic; echo "warning: unknown CPU vendor — using generic events" >&2 ;;
esac
PARANOID=$(cat /proc/sys/kernel/perf_event_paranoid 2>/dev/null || echo 99)
HAVE_IBS=0
[ -d /sys/bus/event_source/devices/ibs_op ] && HAVE_IBS=1

echo "==> CPU: $(awk -F': ' '/^model name/{print $2; exit}' /proc/cpuinfo) [$ARCH]"
# Each of these silently degrades a pass rather than failing it, so say
# so up front instead of leaving a thin report to explain itself.
[ "$PARANOID" -gt 1 ] 2>/dev/null &&
    echo "    NOTE: perf_event_paranoid=$PARANOID disables some passes — sudo sysctl kernel.perf_event_paranoid=-1" >&2
[ "$(cat /proc/sys/kernel/nmi_watchdog 2>/dev/null || echo 0)" != "0" ] &&
    echo "    NOTE: nmi_watchdog on — reserves one PMC, counters may multiplex (sudo sysctl kernel.nmi_watchdog=0)"
GOV=$(cat "/sys/devices/system/cpu/cpu${PIN_CPU}/cpufreq/scaling_governor" 2>/dev/null || echo unknown)
[ "$GOV" != "performance" ] &&
    echo "    NOTE: governor=$GOV — frequency scaling adds variance."

# ── Build ────────────────────────────────────────────────────────────
# STRIP=none is load-bearing when palantir is built inside an enclosing
# workspace: cargo ignores `[profile.*]` from non-root packages, so
# palantir's own `strip = "none"` never applies and the host workspace's
# bench profile can strip the symtab. DEBUG=2 (not line-tables-only)
# carries the inline records `perf --inline` needs — under `lto = "fat"`
# most of the frame pipeline is inlined and invisible without them.
echo "==> Building '$BENCH_NAME' with debug symbols (features: $FEATURES_ARG)"
# Checked, not assumed: this shell has no `set -e`, so an unchecked
# failure would fall through to the fallback below and profile whatever
# stale binary is lying in target/ — a plausible report of the wrong code.
if ! BUILD_LOG=$(CARGO_PROFILE_BENCH_STRIP=none CARGO_PROFILE_BENCH_DEBUG=2 \
    cargo bench --bench "$BENCH_NAME" --features "$FEATURES_ARG" --no-run 2>&1); then
    echo "$BUILD_LOG" >&2
    echo "error: bench build failed — refusing to profile a stale binary" >&2
    exit 1
fi
echo "$BUILD_LOG" | tail -3

# Prefer the path cargo just printed over an mtime guess: a newer binary
# from a different feature set otherwise wins the `ls -t` race.
BENCH_BIN=$(echo "$BUILD_LOG" \
    | sed -n "s/.*Executable .*(\(.*${BENCH_NAME}-[0-9a-f]*\))$/\1/p" | tail -1)
if [ ! -x "$BENCH_BIN" ]; then
    # No `Executable` line means everything was already fresh. The build
    # succeeded (checked above), so a binary exists — find it. Criterion
    # writes to the workspace target, and palantir is a submodule whose
    # package dir isn't the workspace root, so search upward too.
    BENCH_BIN=""
    for d in target ../target; do
        cand=$(ls -t "$d/release/deps/${BENCH_NAME}"-* 2>/dev/null | grep -vE '\.(d|so)$' | head -1)
        [ -n "$cand" ] && { BENCH_BIN=$cand; break; }
    done
fi
[ -x "$BENCH_BIN" ] || { echo "error: no built binary for '$BENCH_NAME'" >&2; exit 1; }
nm "$BENCH_BIN" >/dev/null 2>&1 ||
    echo "    WARNING: '$BENCH_BIN' has no symbol table — perf will report raw addresses.
             An enclosing workspace's [profile.bench] is overriding strip/debug." >&2
echo "    binary: $BENCH_BIN"
echo "    pinned to CPU $PIN_CPU   callgraph: $CALLGRAPH_MODE   arms: $ARMS_ARG"
echo "    drivers: ${DRIVER_ARG:-<all default>}${FILTER_ARG:+   filter: $FILTER_ARG}"

rm -f "$PERF_DATA" "$PERF_REPORT" "$PERF_STAT" "$PERF_MICRO" \
      "$PERF_IBS_DATA" "$PERF_IBS" "$PERF_MEM_DATA" "$PERF_MEM" "$PERF_DATA.old"

run() { taskset -c "$PIN_CPU" "$@" "$BENCH_BIN" "${BENCH_ARGS[@]}"; }

# perf demangles legacy Rust symbols but not the v0 scheme rustc emits
# now, so every palantir frame would read as `_RNvMNtNt…`. rustfilt
# handles both; without it the reports stay mangled but usable.
if command -v rustfilt >/dev/null 2>&1; then
    demangle() { rustfilt; }
else
    echo "    NOTE: rustfilt not on PATH — symbols stay v0-mangled (cargo install rustfilt)"
    demangle() { cat; }
fi

# Flat, no-callgraph, >=1% report. Same shape for every sampling pass.
report_to() {
    perf report -i "$1" --stdio --no-children -g none --percent-limit 1.0 2>/dev/null \
        | demangle >"$2"
}

# ── perf stat: hardware counters ─────────────────────────────────────
# Intel hybrid needs the explicit `cpu_core/…/` prefix or generic
# `-e cycles` auto-expands across cpu_core + cpu_atom and half-counts on
# a pinned run. AMD's `-d` adds L1-dcache + LLC to the default set (LLC
# reads <not supported> — it's an uncore PMU).
echo "==> perf stat (hardware counters)"
if [ "$ARCH" = intel ]; then
    run perf stat -o "$PERF_STAT" -e task-clock,context-switches,page-faults \
        -e "cpu_core/cycles/,cpu_core/instructions/,cpu_core/branches/,cpu_core/branch-misses/,cpu_core/cache-references/,cpu_core/cache-misses/,cpu_core/L1-dcache-load-misses/,cpu_core/dTLB-load-misses/" \
        >/dev/null 2>&1 || true
else
    run perf stat -d -o "$PERF_STAT" >/dev/null 2>&1 || true
fi

# ── Microarchitectural metrics ───────────────────────────────────────
if [ -z "${SKIP_MICRO:-}" ]; then
    echo "==> perf stat -M (microarch metrics)"
    if [ "$ARCH" = intel ]; then
        # Don't pass --cpu on hybrid: perf tries to attach cpu_atom event
        # variants to the named CPU and the whole group fails. taskset pins.
        run perf stat -M TopdownL1 -o "$PERF_MICRO" >/dev/null 2>&1 ||
            echo "    (TopdownL1 unavailable — kernel too old or PMU denied)"
    else
        # Two small core groups by default so the ~6 PMCs don't
        # oversubscribe; Zen4+ replaces them with a real slot-based
        # topdown when it advertises one. Uncore groups (l3_cache,
        # data_fabric) need -a — see benches/AGENTS.md.
        AMD_GROUPS="branch_prediction,tlb"
        perf list metricgroups 2>/dev/null | grep -qiE 'pipeline_util|topdown' &&
            AMD_GROUPS="Pipeline_Util_Level1"
        run perf stat -M "$AMD_GROUPS" -o "$PERF_MICRO" >/dev/null 2>&1 ||
            echo "    (AMD metric groups unavailable)"
        echo "    groups: $AMD_GROUPS"
    fi
fi

# ── perf record: cycles + callgraph (the workhorse) ──────────────────
# DWARF unwinds .eh_frame from a per-sample stack dump — full depth,
# works on Rust release builds, ~5-10x overhead. LBR (Intel, 32 frames,
# near native) needs no frame pointers; AMD Zen3 BRS isn't wired for
# cycles, so lbr silently degrades — force dwarf there.
CG_EVENT=cycles
[ "$ARCH" = intel ] && CG_EVENT="cpu_core/cycles/"
CG=(--call-graph dwarf,16384)
if [ "$CALLGRAPH_MODE" = lbr ]; then
    if [ "$ARCH" = intel ]; then
        CG=(--call-graph lbr)
    else
        echo "    (lbr unsupported on $ARCH — using dwarf)"
    fi
fi
echo "==> perf record (-F $PERF_FREQ ${CG[*]} -e $CG_EVENT)"
run perf record -F "$PERF_FREQ" "${CG[@]}" -e "$CG_EVENT" -o "$PERF_DATA" >/dev/null 2>&1 ||
    echo "    (record failed — check paranoid level)"
[ -f "$PERF_DATA" ] && report_to "$PERF_DATA" "$PERF_REPORT"

# ── Precise-IP pass (no skid) ────────────────────────────────────────
# Regular cycles sampling skids the recorded IP past the costly
# instruction; IBS (AMD) and PEBS (`:ppp`, Intel) tag the exact retiring
# op. No callgraph — the leaf IP is the point, and the dwarf pass above
# already has the call context.
PRECISE=()
case "$ARCH" in
    amd)   [ "$HAVE_IBS" = 1 ] && PRECISE=(-e ibs_op// -c "$IBS_PERIOD") ;;
    intel) PRECISE=(-e cpu_core/cycles/ppp -F "$PERF_FREQ") ;;
esac
if [ -z "${SKIP_IBS:-}" ]; then
    if [ ${#PRECISE[@]} -eq 0 ]; then
        echo "==> (precise IP unavailable on $ARCH)"
    else
        echo "==> perf record (precise IP: ${PRECISE[*]})"
        run perf record "${PRECISE[@]}" -o "$PERF_IBS_DATA" >/dev/null 2>&1 &&
            report_to "$PERF_IBS_DATA" "$PERF_IBS" ||
            echo "    (precise-IP record failed — needs paranoid <= -1 / CAP_PERFMON)"
    fi
fi

# ── perf mem: load/store data-source (cache-level attribution) ───────
# AMD routes perf mem through IBS Op, Intel through PEBS load-latency.
# AMD ldlat filtering needs the ibs_op/caps/ldlat capability (Zen5+), so
# it isn't passed there.
if [ -z "${SKIP_MEM:-}" ]; then
    echo "==> perf mem record (data-source sampling)"
    if [ "$ARCH" = amd ] && [ "$HAVE_IBS" != 1 ]; then
        echo "    (perf mem unavailable — no IBS)"
    else
        if [ "$ARCH" = amd ]; then
            run perf mem record -o "$PERF_MEM_DATA" >/dev/null 2>&1
        else
            run perf mem record -t load --ldlat="$LDLAT_CYCLES" \
                -o "$PERF_MEM_DATA" >/dev/null 2>&1
        fi
        if [ -f "$PERF_MEM_DATA" ]; then
            perf mem report -i "$PERF_MEM_DATA" --stdio --sort=mem,sym,dso \
                --percent-limit 1.0 2>/dev/null | demangle >"$PERF_MEM" ||
                echo "    (perf mem report failed)"
        else
            echo "    (perf mem unavailable — needs IBS/PEBS + paranoid <= 0)"
        fi
    fi
fi

# ── Summary ──────────────────────────────────────────────────────────
samples() { sed -n '/^# Samples/,/^$/p' "$1" | head -"$2"; }
counters() { sed -n '/Performance counter stats/,$p' "$1" | head -"$2"; }

[ -f "$PERF_REPORT" ] && { echo; echo "==> Top self-time (cycles, callgraph pass):"; samples "$PERF_REPORT" 28; }
[ -f "$PERF_STAT" ]   && { echo; echo "==> Hardware counters:";                      counters "$PERF_STAT" 9999; }
[ -f "$PERF_IBS" ]    && { echo; echo "==> Precise-IP top (no skid):";               samples "$PERF_IBS" 16; }
[ -f "$PERF_MICRO" ]  && { echo; echo "==> Microarch metrics:";                      counters "$PERF_MICRO" 40; }
[ -f "$PERF_MEM" ]    && { echo; echo "==> Memory data-source (top):";               head -30 "$PERF_MEM"; }

cat <<EOF

Flat/self report : $PERF_REPORT
Counters         : $PERF_STAT
Microarch        : $PERF_MICRO
Precise-IP       : $PERF_IBS
Mem data-source  : $PERF_MEM
Callgraph (TUI)  : perf report -i $PERF_DATA
Annotate symbol  : perf annotate -i $PERF_IBS_DATA <symbol>
How to read it   : benches/AGENTS.md
EOF
