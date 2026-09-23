# Reading a profile

How to read what `benches/bench-perf.sh` writes to `tmp/`, and how to drill
past it by hand. Running the benches and the script is in `benches/AGENTS.md`.

## Intel

Read the outputs in this order:

1. **`tmp/palantir-perf-micro.txt`** — which TMA bucket dominates?
   - **Retiring >50%** — healthy. Further wins are algorithmic, not
     microarch tuning.
   - **Backend_bound >40%** — `memory_bound` → `palantir-perf-mem.txt`;
     `core_bound` → port pressure / dependency chains, `perf annotate`.
   - **Frontend_bound >20%** — icache / uop-cache pressure. Look for
     excessive monomorphization or a loop spanning a 32 KiB line.
   - **Bad_speculation >10%** — mispredicts; confirm with
     `branch-misses`.

   Each leaf prints a `Sampling events:` hint — feed it to
   `perf record -e <event>`.
2. **`tmp/palantir-perf-stat.txt`** — IPC. Raptor Cove peaks ~4-5, healthy
   >2.0, stalled <1.0. MPKI = `misses * 1000 / instructions`; dTLB-MPKI >1
   suggests huge pages.
3. **`tmp/palantir-perf-mem.txt`** — when memory-bound. High `Local_RAM` =
   spills LLC, `L3` = spills L2, `LFB` = the prefetcher is covering you.
4. **`perf annotate -i tmp/palantir-perf-ibs.data -M intel <sym>`** — the
   PEBS capture, so the IP has not skidded.

Before drawing conclusions:

- **IPC is a sanity check, not a target.** Low IPC means too many
  instructions in retiring-bound code and cache stalls in memory-bound
  code. Only TMA says which.
- **Miss counts without context are noise.** A 10% L1 miss rate is fine
  if those hit L2, catastrophic if they hit DRAM. `perf mem` tells you.
- **Page-faults in steady state** are the cheap "did we allocate?"
  proxy — non-zero after warmup usually means a `Vec::reserve` crossed a
  page. `tests/alloc` is where that gets attributed.

### Drilling in

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
  Don't strip the prefix — bare `-e cycles` expands across both, so each
  PMU reports its own share and the count looks halved.
- **The first `perf report` table is `cpu_atom`** — startup and I/O, not
  the workload.
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

## AMD (Zen)

No TMA, so the precise IBS report and callgraph drive:

1. **`tmp/palantir-perf-ibs.txt`** — no-skid self-time. Trust it over the
   cycles flat report, whose IP skids past the costly instruction.
2. **`tmp/palantir-perf-stat.txt`** — IPC >2.5 with low miss rates ⇒
   retiring-bound; <1.0 ⇒ stalled, go to 4.
3. `perf annotate -i tmp/palantir-perf-ibs.data <sym>`.
4. *Only if stalled:* **`tmp/palantir-perf-mem.txt`** buckets loads by
   level. Lots of `Local RAM` = locality problem.
5. Per-dimension rates, **one metric group per run** — combining them
   oversubscribes the 6 PMCs and coverage drops to ~14%:

   ```sh
   taskset -c 2 perf stat -M branch_prediction -- "$BIN" "${ARGS[@]}"
   taskset -c 2 perf stat -M tlb               -- "$BIN" ...
   taskset -c 2 perf stat -M l2_cache          -- "$BIN" ...
   taskset -c 2 perf stat -a -M l3_cache       -- "$BIN" ...   # uncore, needs -a
   ```

**Pitfalls** (Family 19h, verified on a Ryzen 7 6800U):

- Bare event names — `cpu_core/…/` is Intel-hybrid-only.
- L3 / data-fabric counters are uncore (`amd_l3` / `amd_df`) and read
  `<not counted>` per-process; add `-a`.
- IBS knobs for a hand-rolled `-e ibs_op/…/`: `-c <period>` is the cycle
  period (default 250000 ≈ 35k samples/2 s); `cnt_ctl=1` switches to
  µop-count periods, better for high-CPI ops than for where cycles pool;
  `l3missonly=1` / `ldlat=128..2048` are Zen4+/Zen5+ only.

## Other tools

`perf c2c` for false sharing (not wired in — the benches are
single-threaded). RenderDoc for GPU work; Tracy (`profile-with-tracy`)
for the CPU zones around it — wgpu's own internal zones stay dark,
because reaching them means the `profiling` facade and a second Tracy
client. `iai-callgrind` for instruction counts when wall-clock variance
hides a small win.

**Tracy frame sets.** Each window marks its own — `window 0`, `window 1`,
… — because windows paint on independent schedules and no single frame
spans them. The *main* set, the one driving the FPS readout, is marked
only while exactly one window is open; with a second open it falls quiet
by design and the per-window sets carry everything. Pick the set in
Tracy's frame-set dropdown.
