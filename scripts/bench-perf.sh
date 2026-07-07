#!/usr/bin/env bash
# Build a criterion bench with debug symbols, wipe old perf data, and
# record a fresh profile alongside hardware-counter aggregates.
#
# **Vendor-aware.** The PMU layout, microarchitectural metrics, and
# precise-sampling mechanism differ between Intel and AMD, so the script
# detects `vendor_id` and picks the right path:
#
#   Intel (e.g. i9-13980HX hybrid)      AMD (e.g. Ryzen 7 6800U, Zen3+)
#   ────────────────────────────────    ────────────────────────────────
#   cpu_core/.../ PMU prefix, pin P-core sole `cpu` PMU (homogeneous cores)
#   perf stat -M TopdownL1 (Intel TMA)  perf stat -M <AMD metric groups>
#   precise IP via cycles:ppp (PEBS)    precise IP via IBS (ibs_op//)
#   perf mem -t load --ldlat=50 (PEBS)  perf mem (IBS; no ldlat on Zen<5)
#   --call-graph lbr available          lbr/BRS unavailable → dwarf only
#
# The cycles + dwarf-callgraph record pass (the workhorse flat/inclusive
# report) is identical on both.
#
# Outputs (all in tmp/, gitignored):
#   tmp/aperture-perf.data         - perf record output (cycles, callgraph)
#   tmp/aperture-perf-report.txt   - flat top-functions report (self time)
#   tmp/aperture-perf-stat.txt     - perf stat counters (IPC, cache, branch)
#   tmp/aperture-perf-micro.txt    - microarch metrics (Intel TMA / AMD groups)
#   tmp/aperture-perf-ibs.txt      - precise-IP report (IBS / PEBS), no skid
#   tmp/aperture-perf-mem.txt      - load/store data-source report (cache levels)
#
# Usage:
#   scripts/bench-perf.sh                              # default: frame bench, --profile-time 5
#   scripts/bench-perf.sh --profile-time 2             # override criterion args
#   BENCH=frame FILTER=cached_cpu scripts/bench-perf.sh
#   CALLGRAPH=lbr scripts/bench-perf.sh                # Intel only; AMD falls back to dwarf
#   SKIP_MEM=1 scripts/bench-perf.sh                   # skip the data-source pass
#   SKIP_MICRO=1 scripts/bench-perf.sh                 # skip the microarch-metrics pass
#   SKIP_IBS=1 scripts/bench-perf.sh                   # skip the precise-IP pass
#
# Env:
#   BENCH       bench target from Cargo.toml (default: frame)
#   FILTER      criterion filter, prepended to bench args (default: empty = all)
#   FEATURES    cargo features, comma-separated (default: internals)
#   CALLGRAPH   dwarf (default) or lbr (Intel only — ~native overhead, 32 frames)
#   PIN_CPU     core to pin to (default: 2 — avoids CPU0's IRQ load)
#   FREQ        cycles sampling frequency for the callgraph pass (default: 4000)
#   IBS_PERIOD  AMD IBS op sample period in cycles (default: 250000)
#   LDLAT       Intel PEBS load-latency cutoff in cycles (default: 50; AMD ignores)
#   SKIP_MEM / SKIP_MICRO / SKIP_IBS   set non-empty to skip that pass
#
# The frame bench refuses to run without APERTURE_BENCH_MODE + _NOTE; export
# them before invoking (e.g. `APERTURE_BENCH_MODE=cpu APERTURE_BENCH_NOTE=x`).
#
# Reading order (top-down):
#   1. tmp/aperture-perf-micro.txt — where's the bottleneck class?
#      Intel TMA: retiring / frontend / backend / bad-spec buckets.
#      AMD Zen3 (no slot-based topdown): read the cache/TLB/branch group
#      counters directly (Zen4+ exposes a real topdown — see note below).
#      A retiring-bound / high-IPC workload (IPC > 2.5, low miss rates)
#      wins only from doing *fewer* instructions, not microarch tuning.
#   2. tmp/aperture-perf-stat.txt — IPC = insn/cycles; cache & TLB MPKI
#      (= misses * 1000 / instructions).
#   3. tmp/aperture-perf-ibs.txt — precise (no-skid) leaf IPs; feed the
#      hottest symbol to `perf annotate` for the exact instruction.
#   4. tmp/aperture-perf-mem.txt — which loads stall, at which level.
#   5. tmp/aperture-perf-report.txt + `perf report -i tmp/aperture-perf.data`
#      — callgraph context (callers/callees) for the top self-time symbols.
#
# For allocations (the project's "alloc-free per frame after warmup"
# claim), use the alloc_free / alloc_resize benches with DHAT_DUMP=1 —
# perf only sees CPU time inside the allocator, not allocation counts.

set -uo pipefail

cd "$(dirname "$0")/.."
mkdir -p tmp

PERF_DATA=tmp/aperture-perf.data
PERF_REPORT=tmp/aperture-perf-report.txt
PERF_STAT=tmp/aperture-perf-stat.txt
PERF_MICRO=tmp/aperture-perf-micro.txt
PERF_IBS_DATA=tmp/aperture-perf-ibs.data
PERF_IBS=tmp/aperture-perf-ibs.txt
PERF_MEM_DATA=tmp/aperture-perf-mem.data
PERF_MEM=tmp/aperture-perf-mem.txt

BENCH_NAME="${BENCH:-frame}"
FILTER_ARG="${FILTER:-}"
FEATURES_ARG="${FEATURES:-internals}"
CALLGRAPH_MODE="${CALLGRAPH:-dwarf}"
PIN_CPU="${PIN_CPU:-2}"
PERF_FREQ="${FREQ:-4000}"
IBS_PERIOD="${IBS_PERIOD:-250000}"
LDLAT_CYCLES="${LDLAT:-50}"

EXTRA_ARGS=("$@")
if [ ${#EXTRA_ARGS[@]} -eq 0 ]; then
    EXTRA_ARGS=(--profile-time 5)
fi
BENCH_ARGS=(--bench)
[ -n "$FILTER_ARG" ] && BENCH_ARGS+=("$FILTER_ARG")
BENCH_ARGS+=("${EXTRA_ARGS[@]}")

for tool in perf taskset; do
    command -v "$tool" >/dev/null 2>&1 || { echo "error: $tool not installed" >&2; exit 1; }
done

# ── Vendor + capability detection ────────────────────────────────────
VENDOR=$(awk -F': ' '/^vendor_id/{print $2; exit}' /proc/cpuinfo)
case "$VENDOR" in
    AuthenticAMD) ARCH=amd ;;
    GenuineIntel) ARCH=intel ;;
    *) ARCH=generic; echo "warning: unknown vendor '$VENDOR' — using generic events" >&2 ;;
esac

PARANOID=$(cat /proc/sys/kernel/perf_event_paranoid 2>/dev/null || echo 99)
HAVE_IBS=0
[ -d /sys/bus/event_source/devices/ibs_op ] && HAVE_IBS=1

echo "==> CPU: $(awk -F': ' '/^model name/{print $2; exit}' /proc/cpuinfo) [$ARCH]"
echo "    perf_event_paranoid=$PARANOID  (need <=2 for user sampling, <=-1 for raw/IBS/kernel)"
if [ "$PARANOID" -gt 1 ] 2>/dev/null; then
    echo "    NOTE: paranoid > 1 disables some passes. Lower it:" >&2
    echo "          sudo sysctl kernel.perf_event_paranoid=-1" >&2
fi
# The NMI watchdog steals one general PMC; disable it for full counter
# coverage (informational — we don't change it here).
if [ "$(cat /proc/sys/kernel/nmi_watchdog 2>/dev/null || echo 0)" != "0" ]; then
    echo "    NOTE: nmi_watchdog on — one PMC reserved (counters may multiplex)."
    echo "          For 100% coverage: sudo sysctl kernel.nmi_watchdog=0"
fi
GOV=$(cat /sys/devices/system/cpu/cpu${PIN_CPU}/cpufreq/scaling_governor 2>/dev/null || echo unknown)
[ "$GOV" != "performance" ] && echo "    NOTE: governor=$GOV (not 'performance') — frequency scaling adds variance."

# ── Build ────────────────────────────────────────────────────────────
echo "==> Building bench '$BENCH_NAME' with debug symbols"
CARGO_BUILD_ARGS=(--bench "$BENCH_NAME")
[ -n "$FEATURES_ARG" ] && CARGO_BUILD_ARGS+=(--features "$FEATURES_ARG")
CARGO_PROFILE_BENCH_DEBUG=line-tables-only \
    cargo bench "${CARGO_BUILD_ARGS[@]}" --no-run 2>&1 | tail -3

# Criterion writes to the workspace target; aperture is a git submodule so
# its package dir isn't the workspace root — search up for target/release.
BENCH_BIN=""
for d in target ../target; do
    cand=$(ls -t "$d/release/deps/${BENCH_NAME}"-* 2>/dev/null | grep -vE '\.(d|so)$' | head -1)
    [ -n "$cand" ] && { BENCH_BIN=$cand; break; }
done
[ -x "$BENCH_BIN" ] || { echo "error: could not locate built bench binary for '$BENCH_NAME'" >&2; exit 1; }
echo "    binary: $BENCH_BIN"
echo "    pinned to CPU $PIN_CPU   callgraph: $CALLGRAPH_MODE"
[ -n "$FILTER_ARG" ] && echo "    filter: $FILTER_ARG"

rm -f "$PERF_DATA" "$PERF_REPORT" "$PERF_STAT" "$PERF_MICRO" \
      "$PERF_IBS_DATA" "$PERF_IBS" "$PERF_MEM_DATA" "$PERF_MEM" "$PERF_DATA.old"

run() { taskset -c "$PIN_CPU" "$@" "$BENCH_BIN" "${BENCH_ARGS[@]}"; }

# ── perf stat: hardware counters ─────────────────────────────────────
# AMD has a single homogeneous `cpu` PMU; Intel hybrid needs the explicit
# `cpu_core/.../` prefix or generic `-e cycles` auto-expands across
# cpu_core + cpu_atom and reports half-counts on a pinned run.
echo "==> perf stat (hardware counters)"
case "$ARCH" in
  intel)
    HW="cpu_core/cycles/,cpu_core/instructions/,cpu_core/branches/,cpu_core/branch-misses/,cpu_core/cache-references/,cpu_core/cache-misses/,cpu_core/L1-dcache-load-misses/,cpu_core/dTLB-load-misses/"
    run perf stat -e "$HW" -e task-clock,context-switches,page-faults \
        -o "$PERF_STAT" >/dev/null 2>&1 || true
    ;;
  *)
    # `-d` (detailed) adds L1-dcache + LLC to the default set; AMD reports
    # LLC as <not supported> (it's an uncore PMU — see the micro pass) but
    # the L1 + IPC + branch lines all resolve.
    run perf stat -d -o "$PERF_STAT" >/dev/null 2>&1 || true
    ;;
esac

# ── Microarchitectural metrics ───────────────────────────────────────
if [ -z "${SKIP_MICRO:-}" ]; then
  echo "==> perf stat -M (microarch metrics)"
  case "$ARCH" in
    intel)
      # Don't pass --cpu on hybrid: perf tries to attach cpu_atom event
      # variants to the named CPU and the whole group fails. taskset pins.
      run perf stat -M TopdownL1 -o "$PERF_MICRO" >/dev/null 2>&1 \
        || echo "    (TopdownL1 unavailable — kernel too old or PMU denied)"
      ;;
    *)
      # AMD core metric groups (per-process). l3_cache / data_fabric are
      # *uncore* (amd_l3 / amd_df) and need -a (system-wide), so they're
      # omitted here. Zen4+ also exposes a real slot-based topdown
      # (Pipeline_Util_*) — add it if `perf list metricgroups` lists it.
      # Keep the default lean (two small core groups) so the ~6 PMCs don't
      # oversubscribe. Zen4+ adds a real slot-based topdown — prefer it.
      AMD_GROUPS="branch_prediction,tlb"
      perf list metricgroups 2>/dev/null | grep -qiE 'pipeline_util|topdown' \
        && AMD_GROUPS="Pipeline_Util_Level1"
      run perf stat -M "$AMD_GROUPS" -o "$PERF_MICRO" >/dev/null 2>&1 \
        || echo "    (AMD metric groups unavailable)"
      echo "    groups: $AMD_GROUPS"
      echo "    more (run one at a time for clean counts): l2_cache, decoder, data_fabric"
      echo "    uncore (need -a, system-wide): perf stat -a -M l3_cache,data_fabric"
      ;;
  esac
fi

# ── perf record: cycles + callgraph (the workhorse) ──────────────────
# DWARF unwinds .eh_frame from a per-sample stack dump — full depth, works
# on Rust release builds, ~5-10x overhead. LBR (Intel, 32 frames, near
# native) needs no frame pointers; AMD Zen3 BRS isn't wired for cycles, so
# lbr silently degrades — force dwarf there.
CG_EVENT="cycles"
[ "$ARCH" = intel ] && CG_EVENT="cpu_core/cycles/"
case "$CALLGRAPH_MODE" in
  lbr)
    if [ "$ARCH" = intel ]; then CG=(--call-graph lbr); else
      echo "    (lbr unsupported on $ARCH — using dwarf)"; CG=(--call-graph dwarf,16384); fi ;;
  *) CG=(--call-graph dwarf,16384) ;;
esac
echo "==> perf record (-F $PERF_FREQ ${CG[*]} -e $CG_EVENT)"
run perf record -F "$PERF_FREQ" "${CG[@]}" -e "$CG_EVENT" -o "$PERF_DATA" >/dev/null 2>&1 \
  || echo "    (record failed — check paranoid level)"
[ -f "$PERF_DATA" ] && perf report -i "$PERF_DATA" --stdio --no-children -g none \
  --percent-limit 1.0 >"$PERF_REPORT" 2>/dev/null

# ── Precise-IP pass (no skid): AMD IBS / Intel PEBS ──────────────────
# Regular cycles sampling skids the recorded IP past the costly
# instruction; IBS (AMD) and PEBS (`:ppp`, Intel) tag the exact retiring
# op. Use this report + `perf annotate` to land on the real instruction.
# No callgraph here — the leaf IP is the point; the dwarf pass above has
# the call context.
if [ -z "${SKIP_IBS:-}" ]; then
  case "$ARCH" in
    amd)
      if [ "$HAVE_IBS" = 1 ]; then
        echo "==> perf record (IBS precise, ibs_op// -c $IBS_PERIOD)"
        run perf record -e ibs_op// -c "$IBS_PERIOD" -o "$PERF_IBS_DATA" >/dev/null 2>&1 \
          && perf report -i "$PERF_IBS_DATA" --stdio --no-children -g none \
             --percent-limit 1.0 >"$PERF_IBS" 2>/dev/null \
          || echo "    (IBS record failed — needs paranoid <= -1 / CAP_PERFMON)"
      else
        echo "==> (IBS unavailable: no ibs_op PMU)"
      fi
      ;;
    intel)
      echo "==> perf record (PEBS precise, cpu_core/cycles/ppp -F $PERF_FREQ)"
      run perf record -F "$PERF_FREQ" -e cpu_core/cycles/ppp -o "$PERF_IBS_DATA" >/dev/null 2>&1 \
        && perf report -i "$PERF_IBS_DATA" --stdio --no-children -g none \
           --percent-limit 1.0 >"$PERF_IBS" 2>/dev/null \
        || echo "    (PEBS record failed)"
      ;;
  esac
fi

# ── perf mem: load/store data-source (cache-level attribution) ───────
# AMD routes perf mem through IBS Op; Intel through PEBS load-latency.
# AMD ldlat filtering needs the ibs_op/caps/ldlat capability (Zen5+) — on
# Zen3/4 it's ignored, so we don't pass it there.
if [ -z "${SKIP_MEM:-}" ]; then
  echo "==> perf mem record (data-source sampling)"
  case "$ARCH" in
    amd)
      MEM_OK=0
      [ "$HAVE_IBS" = 1 ] && run perf mem record -o "$PERF_MEM_DATA" >/dev/null 2>&1 && MEM_OK=1 ;;
    *)
      run perf mem record -t load --ldlat="$LDLAT_CYCLES" -o "$PERF_MEM_DATA" >/dev/null 2>&1 && MEM_OK=1 || MEM_OK=0 ;;
  esac
  if [ "${MEM_OK:-0}" = 1 ]; then
    perf mem report -i "$PERF_MEM_DATA" --stdio --sort=mem,sym,dso \
      --percent-limit 1.0 >"$PERF_MEM" 2>/dev/null || echo "    (perf mem report failed)"
  else
    echo "    (perf mem unavailable — needs IBS/PEBS + paranoid <= 0)"
  fi
fi

# ── Summary to stdout ────────────────────────────────────────────────
echo
echo "==> Top self-time (cycles, callgraph pass):"
[ -f "$PERF_REPORT" ] && sed -n '/^# Samples/,/^$/p' "$PERF_REPORT" | head -28
echo
echo "==> Hardware counters:"
[ -f "$PERF_STAT" ] && sed -n '/Performance counter stats/,$p' "$PERF_STAT"
if [ -f "$PERF_IBS" ]; then
  echo; echo "==> Precise-IP top (no skid):"; sed -n '/^# Samples/,/^$/p' "$PERF_IBS" | head -16
fi
if [ -f "$PERF_MICRO" ]; then
  echo; echo "==> Microarch metrics:"; sed -n '/Performance counter stats/,$p' "$PERF_MICRO" | head -40
fi
if [ -f "$PERF_MEM" ]; then
  echo; echo "==> Memory data-source (top):"; head -30 "$PERF_MEM"
fi

echo
echo "Flat/self report : $PERF_REPORT"
echo "Counters         : $PERF_STAT"
echo "Microarch        : $PERF_MICRO"
echo "Precise-IP       : $PERF_IBS"
echo "Mem data-source  : $PERF_MEM"
echo "Callgraph (TUI)  : perf report -i $PERF_DATA"
echo "Annotate symbol  : perf annotate -i ${PERF_IBS_DATA} <symbol>   # precise, lands on the instruction"
