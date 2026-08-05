# Benches

Criterion benches over the frame pipeline, plus dhat allocation benches
that pin the alloc-free-per-frame posture. The rest is the profiling
manual.

## Targets

- **`criterion`** — every criterion driver. Select with criterion's
  filter, not a target name:

  ```sh
  cargo bench -p palantir --bench criterion -- damage
  cargo bench -p palantir --bench criterion -- 'cascade/hit_test$'
  ```

  Ids are namespaced by subsystem (`damage/workload`,
  `frame/cached_cpu`), so a bare subsystem name selects its whole set.

- **`alloc_free`, `alloc_resize`, `alloc_free_gpu`** — dhat, one target
  each. `dhat::Alloc` is a `#[global_allocator]`, which is per-binary,
  so they can't share a target with a timing bench.

One criterion target rather than eighteen because `[profile.bench]` is
fat-LTO with one codegen unit: every target links the whole dependency
graph and cargo links targets in parallel. Eighteen of those at once was
an OOM risk; four is a normal build.

**`frame` opts in via the environment**, since it shares the binary and
can't make demands of a run that only wanted `damage`. Unset mode ⇒ skip
notice and return.

```sh
PALANTIR_BENCH_MODE=cpu PALANTIR_BENCH_NOTE='after staging-belt rework' \
    cargo bench -p palantir --bench criterion -- '^frame/'
```

## Profiling

`scripts/bench-perf.sh`. Linux only — built on `perf`, needs `perf` +
`taskset`. Reads `/proc/cpuinfo` `vendor_id` and picks the PMU layout,
microarch metrics, and precise-sampling mechanism to match. Pins to one
core (`PIN_CPU`, default 2) and runs five passes:

| pass | Intel | AMD | output |
|---|---|---|---|
| counters | `cpu_core/…/` events¹ | `perf stat -d`² | `perf-stat.txt` |
| microarch | `-M TopdownL1` (TMA) | `-M branch_prediction,tlb`³ | `perf-micro.txt` |
| callgraph | `perf record` cycles, `dwarf,16384` (`CALLGRAPH=lbr` Intel-only) | same, dwarf⁴ | `perf.data`, `perf-report.txt` |
| precise-IP | `cycles/ppp` (PEBS) | `ibs_op//` (IBS) | `perf-ibs.txt` |
| data-source | `perf mem -t load --ldlat=50` | `perf mem` (no `ldlat` pre-Zen5) | `perf-mem.txt` |

All under `tmp/`. ¹ Explicit prefix required — bare `-e cycles`
auto-expands across `cpu_core` + `cpu_atom` on a hybrid and half-counts.
² Homogeneous cores; LLC reads `<not supported>`, it's an uncore PMU.
³ **Zen<4 has no slot-based topdown** — the single most important vendor
difference, and why the TMA drill below is Intel-only. Zen4+ adds
`Pipeline_Util_*`, auto-detected. ⁴ Zen3 has no usable LBR/BRS for
cycles, so `CALLGRAPH=lbr` silently falls back.

Prerequisites: `sudo sysctl kernel.perf_event_paranoid=-1` (IBS, raw
events, kernel symbols — the script warns if higher) and `sudo sysctl
kernel.nmi_watchdog=0` (the watchdog reserves one PMC, so without it
coverage never reads 100%).

```sh
scripts/bench-perf.sh                                  # frame bench, 5s
FILTER='damage/workload' scripts/bench-perf.sh
FILTER= scripts/bench-perf.sh                          # every driver
CALLGRAPH=lbr scripts/bench-perf.sh                    # Intel only
SKIP_MEM=1 SKIP_MICRO=1 SKIP_IBS=1 scripts/bench-perf.sh
```

Env: `BENCH` (default `criterion`), `FILTER` (regex, default `frame`;
empty for all), `FEATURES` (default `internals`), `CALLGRAPH`
(`dwarf`|`lbr`), `PIN_CPU` (2), `FREQ` (4000), `IBS_PERIOD` (250000),
`LDLAT` (50), `SKIP_MEM`, `SKIP_MICRO`, `SKIP_IBS`.

`[profile.bench]` already builds optimized + debuginfo, so symbolication
needs no extra flags. Use `--profile-time N` rather than criterion's
adaptive loop — a fixed measurement window is what makes sample counts
comparable between runs.

### Reading the output

Intel, in this order:

1. **`perf-micro.txt`** — which TMA bucket dominates?
   - **Retiring >50%** — healthy. Further wins are algorithmic (retire
     fewer instructions), not microarch tuning.
   - **Backend_bound >40%** — `memory_bound` dominant → `perf-mem.txt`;
     `core_bound` dominant → port pressure / dependency chains, drill
     with `perf annotate`.
   - **Frontend_bound >20%** — icache / uop-cache pressure. Look for
     excessive monomorphization or a hot loop spanning a 32 KiB line.
   - **Bad_speculation >10%** — mispredicts; confirm with
     `branch-misses` and `perf annotate` jumps.

   Each leaf prints a `Sampling events:` hint — feed it to
   `perf record -e <event>`.
2. **`perf-stat.txt`** — IPC. Raptor Cove peaks ~4-5, healthy >2.0,
   stalled <1.0. MPKI = `misses * 1000 / instructions`; dTLB-MPKI >1
   suggests huge pages.
3. **`perf-mem.txt`** — when memory-bound. High `Local_RAM` = working
   set spills LLC; high `L3` = spills L2; high `LFB` = the prefetcher
   is covering you, a cheap miss.
4. **`perf annotate -M intel <sym>`** on `perf.data` — the exact
   instruction.

Three things worth knowing before drawing conclusions:

- **IPC is a sanity check, not a target.** Low IPC in retiring-bound
  code means too many instructions emitted; low IPC in memory-bound
  code means cache stalls. Only TMA tells you which.
- **Cache miss counts without context are noise.** A 10% L1 miss rate is
  fine if those hit L2, catastrophic if they hit DRAM. `perf mem` is
  the only way to tell.
- **Page-faults in steady state** are the cheap "did we allocate?"
  proxy — non-zero after warmup usually means a `Vec::reserve` crossed
  a page. For attribution use the alloc benches with `DHAT_DUMP=1` and
  load `dhat-heap.json` at <https://nnethercote.github.io/dh_view/>.
  Never time those benches; dhat adds 10-30× allocator overhead.

### Drilling in (Intel)

TMA drills four levels: L1 bucket → memory sub-bucket → cache-level
sub-bucket → a specific event with source-line attribution.

```sh
cargo bench --bench criterion --features bench --no-run
BIN=$(ls -t target/release/deps/criterion-* | grep -v '\.d$' | head -1)
export PALANTIR_BENCH_MODE=cpu PALANTIR_BENCH_NOTE='drill note'
RUN=(taskset -c 0 perf stat)

"${RUN[@]}" -M TopdownL1          -- "$BIN" --bench cached_cpu --profile-time 4
"${RUN[@]}" -M tma_memory_bound_group -- "$BIN" --bench cached_cpu --profile-time 4
"${RUN[@]}" -M tma_l1_bound_group  -- "$BIN" --bench cached_cpu --profile-time 4
"${RUN[@]}" -M tma_store_bound_group -- "$BIN" --bench cached_cpu --profile-time 4
```

Each `tma_*_group` names the events its metrics derive from. Once one
leaf is clearly the driver, sample that event with PEBS — `:ppp` is
max-precision, so the IP lands on the offending instruction rather than
skidding past it, and LBR callgraph is essentially free where dwarf
would distort the very stalls being measured:

```sh
taskset -c 0 perf record -e cpu_core/LD_BLOCKS.STORE_FORWARD/ppp \
    --call-graph lbr -o tmp/perf-stfwd.data -- \
    "$BIN" --bench cached_cpu --profile-time 4
perf report -i tmp/perf-stfwd.data --stdio --no-children -g none --percent-limit 1.0
perf annotate -i tmp/perf-stfwd.data -M intel <sym>
```

**TMA leaves** (Raptor Cove):

| leaf | means | typical cause / fix |
|---|---|---|
| `store_fwd_blk` | load can't fast-path from an in-flight store, ~10-20 cyc | narrower load off a wider store, or the reverse. `Vec::push` / arena-bump (cursor stored then re-read), SoA pushes writing each column separately |
| `split_loads` / `split_stores` | access spans two cache lines | misaligned `#[repr(packed)]`, `bytemuck` off an unaligned buffer. Align the source/destination |
| `fb_full` | fill buffers full (~12), L1 can't dispatch more misses | bandwidth-bound, not latency-bound |
| `dtlb_load` | page walks | >1% MPKI → consider huge pages |
| `streaming_stores` | non-temporal stores | informational; ~0% unless the code uses `_mm_stream_*` |
| store-bound remainder | store buffer full | combine adjacent stores into wider writes (`copy_nonoverlapping` of a row vs field-by-field) |
| `l1_bound` | stalls but hits L1 | not capacity — usually store-fwd or split |
| `l2_bound` | spills L1 (~48 KiB/core) | acceptable for short hot loops |
| `l3_bound` | spills L2 (1.25 MiB) | tighter packing or blocking |
| `dram_bound` | L3 missed | the real locality problem; >5% warrants a `perf mem` layout investigation |

### Hybrid-CPU pitfalls (Raptor Lake)

- Two PMUs: `cpu_core/` (P-cores 0-15), `cpu_atom/` (E-cores 16-31). The
  script prefixes every event and pins with `taskset -c 0`. Don't strip
  the prefix — `-e cycles` reports per-PMU and looks halved.
- TMA groups only resolve on `cpu_core`; cpu_atom variants come back
  `<not counted>`, which is fine. **Don't pass `--cpu` to the topdown
  `perf stat`** — it tries to attach the cpu_atom variants to the named
  CPU and fails the whole group with "no supported events found."
  `taskset` alone is sufficient.
- 8 general counters per P-core. Adding events means splitting into
  several `perf stat` runs, not one fat `-e` list — multiplex scaling
  distorts short (<100 ms) runs.
- Thread Director can migrate a thread mid-run despite a single-core pin
  when other cores are idle. For multithreaded work use `--cpu-list 0-7`
  against `/sys/devices/cpu_core/cpus`.

### AMD (Zen)

No TMA, so let the precise IBS report and callgraph drive:

1. **`perf-ibs.txt`** — no-skid self-time leaderboard. Trust it over the
   cycles flat report, whose IP skids past the costly instruction.
2. **`perf-stat.txt`** — IPC >2.5 with low miss rates ⇒ retiring-bound
   (do less work); <1.0 ⇒ stalled, go to 4.
3. `perf annotate -i tmp/palantir-perf-ibs.data <sym>` — lands on the
   exact retiring op.
4. *Only if stalled:* **`perf-mem.txt`** buckets loads by level (`L2
   hit` / `L3 hit` / `Local RAM`). Lots of `RAM` = locality problem.
5. Per-dimension rates, **one metric group per run** — combining them
   oversubscribes the 6 PMCs and coverage drops to ~14%:

   ```sh
   taskset -c 2 perf stat -M branch_prediction -- "$BIN" --bench cached_cpu --profile-time 4
   taskset -c 2 perf stat -M tlb              -- "$BIN" ...
   taskset -c 2 perf stat -M l2_cache         -- "$BIN" ...
   taskset -c 2 perf stat -a -M l3_cache      -- "$BIN" ...   # uncore, needs -a
   ```

**Standing finding: the frame bench is retiring-bound.** Every CPU arm
runs at IPC ≈ 3.3 (Zen3+ peaks ~6), branch-mispredict <0.2%, ~3% L1-d
miss, <4% frontend-idle. The pipeline is busy retiring, not stalling, so
wins come from executing fewer instructions — which is why the O1
intrinsic-cache win came from *deleting* a sibling re-walk rather than
from microarchitecture tuning.

**Pitfalls** (Family 19h, verified on a Ryzen 7 6800U):

- Use bare event names — `cpu_core/…/` is Intel-hybrid-only.
- L3 / data-fabric counters are uncore (`amd_l3` / `amd_df`) and read
  `<not counted>` per-process; add `-a`.
- IBS knobs for hand-rolled `perf record -e ibs_op/…/`: `-c <period>`
  is the cycle period (`IBS_PERIOD`, default 250000 ≈ 35k samples/2 s);
  `cnt_ctl=1` switches to µop-count periods, better for finding high-CPI
  ops than where cycles pool; `l3missonly=1` / `ldlat=128..2048` are
  Zen4+/Zen5+ only and no-ops here.

### Other tools

`perf c2c` for false sharing (not wired in — the benches are
single-threaded). RenderDoc or Tracy (`profile-with-tracy`) for GPU
work. `iai-callgrind` for instruction counts when wall-clock variance
hides a small win.
