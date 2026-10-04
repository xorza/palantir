# Reading a profile

How to read what `benches/bench-perf.sh` writes to `tmp/`, and how to drill
past it by hand.

## Both vendors

- **IPC is a sanity check, not a target.** Low IPC means too many
  instructions in retiring-bound code and cache stalls in memory-bound
  code. Only TMA, or on AMD the miss rates, say which.
- **A miss count needs its level.** A 10% L1 miss rate is fine if those
  hit L2, and very bad if they reach DRAM. `perf mem` tells you.
- **Page faults after warmup** usually mean a `Vec::reserve` crossed a page.
  `tests/alloc` attributes them.

## Intel (Raptor Lake)

Read in this order:

1. **`palantir-perf-micro.txt`** — which TMA bucket dominates?
   - **Retiring >50%** — healthy. Further wins are algorithmic.
   - **Backend_bound >40%** — `memory_bound` → `palantir-perf-mem.txt`;
     `core_bound` → port pressure or dependency chains, `perf annotate`.
   - **Frontend_bound >20%** — icache or uop-cache pressure: excess
     monomorphization, or a hot loop spread over too much code.
   - **Bad_speculation >10%** — mispredicts; confirm with `branch-misses`.

   Each leaf prints a `Sampling events:` hint for `perf record -e`.
2. **`palantir-perf-stat.txt`** — Raptor Cove IPC peaks at ~4-5, >2.0 is
   healthy, <1.0 stalled. dTLB misses above 1 per 1000 instructions
   suggest huge pages.
3. **`palantir-perf-mem.txt`** — when memory-bound. `Local_RAM` spills LLC,
   `L3` spills L2, `LFB` is the prefetcher covering you.
4. **`perf annotate -i tmp/palantir-perf-ibs.data -M intel <sym>`** — PEBS,
   so the IP has not skidded.

Drill one level at a time — bucket, memory sub-bucket, cache level, then one
event with source lines (setup in [By hand](#by-hand)):

```sh
RUN=(taskset -c 16-31 perf stat)   # perf on the E-cores
"${RUN[@]}" -M TopdownL1              "${BENCH[@]}"
"${RUN[@]}" -M tma_memory_bound_group "${BENCH[@]}"
"${RUN[@]}" -M tma_l1_bound_group     "${BENCH[@]}"
"${RUN[@]}" -M tma_store_bound_group  "${BENCH[@]}"

# Sample the winning event with PEBS (`ppp`: no skid) and LBR.
taskset -c 16-31 perf record -e cpu_core/LD_BLOCKS.STORE_FORWARD/ppp \
    --call-graph lbr -o tmp/perf-stfwd.data "${BENCH[@]}"
perf annotate -i tmp/perf-stfwd.data -M intel <sym>
```

| TMA leaf | means | cause / fix |
|---|---|---|
| `store_fwd_blk` | a load cannot forward from an in-flight store, ~10-20 cycles | a narrow load off a wide store or the reverse — `Vec::push` or an arena bump (a cursor stored then read back), SoA pushes that write columns separately, a struct stored field by field then hashed as bytes |
| `split_loads` / `split_stores` | the access spans two cache lines | `#[repr(packed)]`, `bytemuck` off an unaligned buffer — align it |
| `fb_full` | fill buffers full (~12) | bandwidth-bound, not latency-bound |
| `dtlb_load` | page walks | huge pages |
| store-bound remainder | store buffer full | wider writes (`copy_nonoverlapping` of a row, not field by field) |
| `l1_bound` | stalls that hit L1 | usually store forwarding or splits, not capacity |
| `l2_bound` | spills L1 (~48 KiB a core) | fine for short hot loops |
| `l3_bound` | spills L2 (1.25 MiB) | tighter packing or blocking |
| `dram_bound` | misses L3 | the real locality problem; >5% wants a `perf mem` pass |

Hybrid pitfalls:

- Two PMUs: `cpu_core/` (P-cores 0-15) and `cpu_atom/` (E-cores 16-31). A
  bare `-e cycles` counts on both, each its own share, and `perf report`
  shows the `cpu_atom` table first. Keep the prefix.
- TMA groups resolve only on `cpu_core`. Do not pass `--cpu` to a TMA
  `perf stat`: perf attaches the `cpu_atom` variants to that CPU and the
  group fails with "no supported events found". `taskset` is enough.
- 8 general counters a P-core. More events means more `perf stat` runs,
  not a longer `-e` list: multiplexing distorts short runs.

## AMD (Zen 3+, Ryzen 7 6800U)

No TMA. Read in this order:

1. **`palantir-perf-ibs.txt`** — self time without skid. Trust it over
   `palantir-perf-report.txt`, whose IPs skid past the costly instruction.
2. **`palantir-perf-stat.txt`** — IPC >2.5 with low miss rates is
   retiring-bound; <1.0 is stalled, go to 4.
3. `perf annotate -i tmp/palantir-perf-ibs.data <sym>`.
4. *Only if stalled:* **`palantir-perf-mem.txt`** — loads by level. Much
   `Local RAM` is a locality problem.
5. One metric group a run; more oversubscribe the six counters, and
   coverage drops to ~14%:

   ```sh
   RUN=(taskset -c 0,1,4-15 perf stat)   # perf off cpu2 and its sibling
   "${RUN[@]}" -M branch_prediction "${BENCH[@]}"
   "${RUN[@]}" -M tlb               "${BENCH[@]}"
   "${RUN[@]}" -M l2_cache          "${BENCH[@]}"
   "${RUN[@]}" -a -M l3_cache       "${BENCH[@]}"   # uncore: needs -a
   ```

Pitfalls:

- Event names are bare — `cpu_core/…/` is Intel-only.
- L3 and data-fabric counters (`amd_l3`, `amd_df`) are uncore and read
  `<not counted>` for one process. Add `-a`.
- `ibs_op//` knobs: `-c` is the period in cycles (250000 ≈ 18k samples a
  second at 4.7 GHz). `cnt_ctl=1` counts µops instead, better for high-CPI
  ops than for where cycles pool. `l3missonly=1` is Zen4+, `ldlat=` Zen5+.

## By hand

Run the script first — it already knows the vendor, the events and the
pinning. For a capture it does not make, use this setup, which keeps the
rules in `benches/AGENTS.md`:

```sh
BIN=$(cargo bench --bench criterion --features bench --no-run 2>&1 |
    sed -n 's/.*Executable .*(\(.*\))$/\1/p')
BENCH=(-- taskset -c 2 setarch -R "$BIN" --bench -d frame --arms cpu --profile-time 4)
```

Then put `perf` on the other cores, not on cpu2 or its sibling.

- **`perf report -g graph` over a whole capture does not finish** (10 min
  on a 16 MB file). For "who calls X", filter `perf script` stacks that
  contain X and count the frame above it. Use `perf report -g none` for
  flat self time.
- **DWARF unwinding needs the whole stack in each sample.** With
  `dwarf,16384`, 2 of 37 475 `cascade` samples reached `main`; with
  `dwarf,65528`, 13 915 of 14 052 did. At 64 KiB a sample, keep the rate
  low (`-F 499`) and compress (`-z`): 28 s then writes ~1.3 MB.
- **Startup takes ~1 s** (the font database), with `fontdb::parse_face_info`
  near the top. `-D <ms>` skips it.

## Other tools

- `perf c2c` for false sharing — not wired in, the benches are
  single-threaded.
- RenderDoc for GPU work. Tracy (`profile-with-tracy`) for the CPU zones
  around it; wgpu's own zones stay dark, because they need the
  `profiling` facade and a second Tracy client. Each window marks its own
  frame set (`window 0`, `window 1`, …); the main set, which drives the
  FPS readout, is marked only while one window is open. Pick the set in
  Tracy's frame-set dropdown.
- `gungraun` (formerly `iai-callgrind`) counts instructions, for a win
  smaller than the wall-clock noise.
