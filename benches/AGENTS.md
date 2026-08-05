# Benches

Criterion benches over the frame pipeline, plus dhat allocation benches
that pin the alloc-free-per-frame posture. The rest is the profiling
manual — how to invoke things is in `scripts/bench-perf.sh`'s header.

## Running them

Two targets, each `harness = false` with its own `main`: `criterion`
holds every timing driver, `alloc` every dhat one. That split — and only
that one — is forced by `dhat::Alloc` being a `#[global_allocator]`:
process-wide, and a 10-30x tax on any timing sharing the binary. Within
each harness it is one target because `[profile.bench]` is fat-LTO with
one codegen unit, and twenty parallel links of the whole dependency
graph was an OOM risk.

`criterion` takes `--driver` (exact, repeatable) and `--list-drivers`;
`--arms` picks a half of the pipeline, and a bare positional is
criterion's own regex over benchmark ids. `alloc` takes no selection at
all — two steps, both gated, both run:

```sh
cargo bench -p palantir --bench criterion -- --list-drivers
cargo bench -p palantir --bench criterion -- -d damage
cargo bench -p palantir --bench criterion -- --arms cpu
cargo bench -p palantir --bench criterion -- 'cascade/hit_test$'
cargo bench -p palantir --bench criterion -- -d frame --arms cpu --note 'after belt rework'
cargo bench -p palantir --bench alloc               # both steps
cargo bench -p palantir --bench alloc -- --dump     # + dhat-heap.json
```

`--help` lists the rest — `--profile-time`, `--sample-size`,
`--measurement-time`, `--warm-up-time`, `--save-baseline`, `--noplot`,
and the frame bench's `--size`, `--scale`, `--machine`, `--note`.

**Every input is a flag.** No bench reads an environment variable of its
own: one parser per target, and what it resolves is handed down in a
`Run`. A knob that has to reach a driver goes on the CLI — never into
the environment, where nothing declares it and `--help` can't list it.

**`alloc` gates, `tests/alloc` attributes.** The bench answers the two
questions only it can — does our per-frame code allocate at full scale,
and has the wgpu driver floor drifted. Everything finer lives in
`cargo test --test alloc`: ~20 fixtures, per-frame budgets, backtrace
capture, under a second, no allocator tax. Reach for that one first when
a number moves; add a step here only for something it structurally
cannot see.

**`frame` is opt-in** — `-d frame`. It is the one driver kept out of a
bare run: the full matrix is ~90 s and appends a results row to
`benches/results/<machine>.txt` that demands a `--note`.

`--driver` is exact: an unselected driver never runs, so it costs
nothing and stays out of the profile. That is why the runner lives in
`src/bench/` and owns `main` rather than using `criterion_main!` — see
its module doc for the mechanism.

## Profiling

`scripts/bench-perf.sh` — Linux only, needs `perf` + `taskset`. Reads
`vendor_id` and picks the PMU layout, metrics, and precise-sampling
mechanism to match, pins to one core, and runs five passes:

| pass | Intel | AMD | output |
|---|---|---|---|
| counters | `cpu_core/…/` events¹ | `perf stat -d`² | `perf-stat.txt` |
| microarch | `-M TopdownL1` (TMA) | `-M branch_prediction,tlb`³ | `perf-micro.txt` |
| callgraph | `perf record` cycles, `dwarf,16384`⁴ | same | `perf.data`, `perf-report.txt` |
| precise-IP | `cycles/ppp` (PEBS) | `ibs_op//` (IBS) | `perf-ibs.txt` |
| data-source | `perf mem -t load --ldlat=50` | `perf mem` (no `ldlat` pre-Zen5) | `perf-mem.txt` |

All under `tmp/`. ¹ Explicit prefix required — bare `-e cycles`
auto-expands across `cpu_core` + `cpu_atom` and half-counts. ² LLC reads
`<not supported>`; it's an uncore PMU. ³ **Zen<4 has no slot-based
topdown**, which is why the TMA drill below is Intel-only; Zen4+ adds
`Pipeline_Util_*`, auto-detected. ⁴ `CALLGRAPH=lbr` is Intel-only —
Zen3's BRS isn't wired for cycles and silently falls back.

Needs `sudo sysctl kernel.perf_event_paranoid=-1` (IBS, raw events,
kernel symbols) and `kernel.nmi_watchdog=0` (the watchdog reserves a
PMC, so coverage never reads 100% with it on). The script warns on both.

Two config facts it leans on: `[profile.bench]` already builds
optimized + debuginfo, so symbolication needs no extra flags; and
`--profile-time N` beats criterion's adaptive loop, because a fixed
window makes sample counts comparable across runs.

### Reading the output

Intel, in this order:

1. **`perf-micro.txt`** — which TMA bucket dominates?
   - **Retiring >50%** — healthy. Further wins are algorithmic, not
     microarch tuning.
   - **Backend_bound >40%** — `memory_bound` → `perf-mem.txt`;
     `core_bound` → port pressure / dependency chains, `perf annotate`.
   - **Frontend_bound >20%** — icache / uop-cache pressure. Look for
     excessive monomorphization or a loop spanning a 32 KiB line.
   - **Bad_speculation >10%** — mispredicts; confirm with
     `branch-misses`.

   Each leaf prints a `Sampling events:` hint — feed it to
   `perf record -e <event>`.
2. **`perf-stat.txt`** — IPC. Raptor Cove peaks ~4-5, healthy >2.0,
   stalled <1.0. MPKI = `misses * 1000 / instructions`; dTLB-MPKI >1
   suggests huge pages.
3. **`perf-mem.txt`** — when memory-bound. High `Local_RAM` = spills
   LLC, `L3` = spills L2, `LFB` = the prefetcher is covering you.
4. **`perf annotate -M intel <sym>`** on `perf.data`.

Before drawing conclusions:

- **IPC is a sanity check, not a target.** Low IPC means too many
  instructions in retiring-bound code and cache stalls in memory-bound
  code. Only TMA says which.
- **Miss counts without context are noise.** A 10% L1 miss rate is fine
  if those hit L2, catastrophic if they hit DRAM. `perf mem` tells you.
- **Page-faults in steady state** are the cheap "did we allocate?"
  proxy — non-zero after warmup usually means a `Vec::reserve` crossed a
  page. For attribution run `--bench alloc -- --dump` and load
  `dhat-heap.json` at <https://nnethercote.github.io/dh_view/>.
  Never time those; dhat adds 10-30× allocator overhead.

### Drilling in (Intel)

L1 bucket → memory sub-bucket → cache-level sub-bucket → one event with
source-line attribution.

```sh
cargo bench --bench criterion --features bench --no-run
BIN=$(ls -t target/release/deps/criterion-* | grep -v '\.d$' | head -1)
ARGS=(--bench -d frame --arms cpu --note 'drill note' --profile-time 4)
RUN=(taskset -c 0 perf stat)

"${RUN[@]}" -M TopdownL1              -- "$BIN" "${ARGS[@]}"
"${RUN[@]}" -M tma_memory_bound_group -- "$BIN" "${ARGS[@]}"
"${RUN[@]}" -M tma_l1_bound_group     -- "$BIN" "${ARGS[@]}"
"${RUN[@]}" -M tma_store_bound_group  -- "$BIN" "${ARGS[@]}"
```

Each `tma_*_group` names the events it derives from. Sample the winning
one with PEBS — `:ppp` lands the IP on the offending instruction instead
of skidding past it, and LBR is near-free where dwarf would distort the
stalls being measured:

```sh
taskset -c 0 perf record -e cpu_core/LD_BLOCKS.STORE_FORWARD/ppp \
    --call-graph lbr -o tmp/perf-stfwd.data -- "$BIN" "${ARGS[@]}"
perf report -i tmp/perf-stfwd.data --stdio --no-children -g none --percent-limit 1.0
perf annotate -i tmp/perf-stfwd.data -M intel <sym>
```

**TMA leaves** (Raptor Cove):

| leaf | means | cause / fix |
|---|---|---|
| `store_fwd_blk` | load can't forward from an in-flight store, ~10-20 cyc | narrow load off a wide store or the reverse — `Vec::push` / arena-bump (cursor stored then re-read), SoA pushes writing columns separately |
| `split_loads` / `split_stores` | access spans two cache lines | misaligned `#[repr(packed)]`, `bytemuck` off an unaligned buffer — align it |
| `fb_full` | fill buffers full (~12) | bandwidth-bound, not latency-bound |
| `dtlb_load` | page walks | >1% MPKI → consider huge pages |
| `streaming_stores` | non-temporal stores | informational; ~0% without `_mm_stream_*` |
| store-bound remainder | store buffer full | widen the writes (`copy_nonoverlapping` of a row vs field-by-field) |
| `l1_bound` | stalls but hits L1 | not capacity — usually store-fwd or split |
| `l2_bound` | spills L1 (~48 KiB/core) | fine for short hot loops |
| `l3_bound` | spills L2 (1.25 MiB) | tighter packing or blocking |
| `dram_bound` | L3 missed | the real locality problem; >5% warrants a `perf mem` layout pass |

### Hybrid-CPU pitfalls (Raptor Lake)

- Two PMUs: `cpu_core/` (P-cores 0-15), `cpu_atom/` (E-cores 16-31).
  Don't strip the prefix — `-e cycles` reports per-PMU and looks halved.
- TMA groups resolve only on `cpu_core`; cpu_atom variants read
  `<not counted>`, which is fine. **Don't pass `--cpu` to the topdown
  `perf stat`** — it tries to attach the cpu_atom variants to that CPU
  and fails the group with "no supported events found". `taskset` alone
  is enough.
- 8 general counters per P-core. More events means several `perf stat`
  runs, not one fat `-e` list — multiplex scaling distorts short runs.
- Thread Director can migrate a thread despite a single-core pin when
  other cores are idle. Multithreaded work wants `--cpu-list 0-7`
  against `/sys/devices/cpu_core/cpus`.

### AMD (Zen)

No TMA, so the precise IBS report and callgraph drive:

1. **`perf-ibs.txt`** — no-skid self-time. Trust it over the cycles flat
   report, whose IP skids past the costly instruction.
2. **`perf-stat.txt`** — IPC >2.5 with low miss rates ⇒ retiring-bound;
   <1.0 ⇒ stalled, go to 4.
3. `perf annotate -i tmp/palantir-perf-ibs.data <sym>`.
4. *Only if stalled:* **`perf-mem.txt`** buckets loads by level. Lots of
   `Local RAM` = locality problem.
5. Per-dimension rates, **one metric group per run** — combining them
   oversubscribes the 6 PMCs and coverage drops to ~14%:

   ```sh
   taskset -c 2 perf stat -M branch_prediction -- "$BIN" "${ARGS[@]}"
   taskset -c 2 perf stat -M tlb               -- "$BIN" ...
   taskset -c 2 perf stat -M l2_cache          -- "$BIN" ...
   taskset -c 2 perf stat -a -M l3_cache       -- "$BIN" ...   # uncore, needs -a
   ```

**Standing finding: the frame bench is retiring-bound** — IPC ≈ 3.3
(Zen3+ peaks ~6), mispredict <0.2%, ~3% L1-d miss, <4% frontend-idle.
Busy retiring, not stalling, so wins come from executing fewer
instructions: the O1 intrinsic-cache win came from *deleting* a sibling
re-walk, not from microarchitecture tuning.

**Pitfalls** (Family 19h, verified on a Ryzen 7 6800U):

- Bare event names — `cpu_core/…/` is Intel-hybrid-only.
- L3 / data-fabric counters are uncore (`amd_l3` / `amd_df`) and read
  `<not counted>` per-process; add `-a`.
- IBS knobs for a hand-rolled `-e ibs_op/…/`: `-c <period>` is the cycle
  period (default 250000 ≈ 35k samples/2 s); `cnt_ctl=1` switches to
  µop-count periods, better for high-CPI ops than for where cycles pool;
  `l3missonly=1` / `ldlat=128..2048` are Zen4+/Zen5+ only.

### Other tools

`perf c2c` for false sharing (not wired in — the benches are
single-threaded). RenderDoc or Tracy (`profile-with-tracy`) for GPU
work. `iai-callgrind` for instruction counts when wall-clock variance
hides a small win.
