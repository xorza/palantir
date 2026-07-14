# Benches

Criterion benches for the layout/measure/frame/cascade/damage pipeline.

Each `*.rs` file is a criterion target; cases inside are named like
`<group>/<case>` (e.g. `frame/cached_cpu`, `frame/partial_gpu`,
`damage/full`, `caches/measure/cached`). Filter at run-time with a
criterion regex.

## Running

```sh
APERTURE_BENCH_MODE=both APERTURE_BENCH_NOTE='baseline' cargo bench --bench frame  # all arms
APERTURE_BENCH_MODE=cpu  APERTURE_BENCH_NOTE='note' cargo bench --bench frame      # CPU arms only
APERTURE_BENCH_MODE=gpu  APERTURE_BENCH_NOTE='note' cargo bench --bench frame -- 'cached_gpu'  # filter
cargo bench --bench caches --features internals        # gated benches
```

`frame` refuses to run without both:
- `APERTURE_BENCH_NOTE` — non-empty context string. Inlined into the
  per-run header in `benches/results/<machine>.txt`
  (`=== <utc> — [<mode>] <note> ===`) so each appended row carries
  context for why it was measured.
- `APERTURE_BENCH_MODE` — one of `cpu`, `gpu`, `both`. Selects which of
  the two benches run; `both` is the full ~90 s matrix, `cpu`/`gpu`
  alone is ~45 s. Forces every invocation to be an explicit decision
  rather than defaulting to the full matrix.

### `frame` is two benches in one file

`frame.rs` defines two independent criterion groups (`cpu_benches`,
`gpu_benches`) plus a `results_group` finalizer that prepends the
per-machine row last. `APERTURE_BENCH_MODE` gates each group wholesale,
so **`MODE=cpu` runs zero GPU code** — no adapter / device request, no
`write_stats` — which is the point: a `perf` / `samply` capture of the
CPU bench is uncontaminated by driver activity.

- **`frame/*_cpu`** — aperture's CPU pipeline measured on a **bare `Ui`
  + standalone `Frontend` with no `wgpu::Device` at all** (`CpuHarness`,
  same deviceless path as `alloc_free`). Each iter runs record →
  measure → arrange → cascade → damage and then, when the frame
  produces a render plan, encode + compose — then acks the present
  (`Ui::mark_frame_submitted`) so `classify_frame` matches a real
  host. **Driving the CPU arms through `WindowRenderer::frame_offscreen` + a poll
  was the old shape and was wrong**: a non-blocking `device.poll`
  charges each iter a driver ioctl, and on `RenderPlan::Skip` the host
  does a GPU backbuffer copy — together ~20 % NVIDIA/kernel self-time on
  `cached_cpu` and ~50 % on `resizing_cpu` (multi-MB backbuffer
  realloc per size, `ensure_backbuffer → create_texture`), swamping the
  aperture cost. Time is advanced from a real `Instant` like
  `WindowRenderer::cpu_frame` so wake cadence matches production.
- **`frame/*_gpu`** — the full public path: `WindowRenderer::frame_offscreen`
  against an offscreen `wgpu::Texture` + `PollType::Wait`. Wall time
  covers the whole CPU + GPU pipeline. The per-frame `write_stats` dump
  (upload counts, GPU pass timings) lives here since it's inherently GPU.

Arms (both benches): `cached` (steady state, MeasureCache hits, damage
`Skip`), `partial` (mutates one footer counter → small `Partial` rect),
`resizing` (rotates four surface sizes to bust `available_q`),
`scrolling` (shifts a `Panel::transform` so only the cascade walk
changes). **Every CPU arm runs the full pipeline including encode +
compose** so the numbers are apples-to-apples: a `Skip` frame produces
no render plan, so `CpuHarness::frame` substitutes a `Full` plan for the
encoder (the `cached_cpu` arm thus measures a whole-tree repaint cost,
not a no-op). `partial` keeps its small `Partial` region — the
partial-encode path is its real workload. `cpu_partial` asserts the
`Partial` invariant (deviceless) before timing so a fixture change that
collapses damage to `Full` fails loudly instead of measuring the wrong
thing.

Feature gating (see `[[bench]]` entries in `Cargo.toml`): **every bench
requires `--features internals` except `alloc_free_gpu`** — they construct
via `Ui::default()` / `Ui::for_test*()`, all gated behind `internals`.
(`alloc_resize` also needs it for `Ui::for_test_text()`'s real cosmic-text
— see below.) `alloc_free_gpu` is the exception: it drives only the public
`OffscreenHost` headless render path + the public widget fixture, so it
builds and runs with no features.

`cargo bench --no-run` without features builds only `alloc_free_gpu`; the
rest need `--features internals`.

## Allocation invariants (three benches)

Three benches share the `support/frame_fixture.rs` workload (see
below). Two pin a floor and fail; one only measures.

- **`alloc_free`** — aperture CPU pipeline only (record → measure →
  arrange → cascade → encode), no GPU. **Strict zero** — any non-zero
  block delta over 256 steady-state frames fails. This pins the
  load-bearing AGENTS.md invariant.
- **`alloc_free_gpu`** — same fixture, plus the wgpu submission path
  via `WindowRenderer::frame_offscreen` against an offscreen target with a GPU
  poll between frames. Baselined: every wgpu submission fundamentally
  allocates (`CommandEncoder` Arc, `CommandBuffer` Arc, queue Vec push,
  hal scratch). Current floor ~27 blocks/frame, all attributed to
  `wgpu_core` / `wgpu_hal` (verified via `DHAT_DUMP=1` + dh_view).
  Gate trips above `RENDER_BLOCKS_PER_FRAME_MAX` (35) — a regression
  is either an aperture bug or a wgpu/cosmic-text version drift.
- **`alloc_resize`** — same CPU pipeline as `alloc_free`, but rotates
  the `Display` size each frame to bust the measure / text-shaping
  caches the way `frame/resizing_cpu` does. **Not
  strict-zero — measures, doesn't assert.** Two arms: `pool-rotation`
  (cycles four sizes, matching `frame/resizing_cpu`) and
  `continuous-drag` (a unique width every frame, modelling a
  window-edge drag with no cache hits possible). Prints blocks/frame +
  bytes/frame per arm; use it to find which call sites still allocate
  on the resize path. **Builds `Ui::for_test_text()` (real cosmic-text),
  hence `required-features = ["internals"]`** — with the `Ui::default()`
  mono fallback the paint count is constant across sizes, so the damage
  `PaintSnapArena` reuses arena slots in place and the bench
  reports a false 0. This was a real blind spot: until 2026-05 the bench
  used the fallback and reported 0 blocks/frame while the live arm
  reallocated ~1.3 MB/frame.

```sh
cargo bench --bench alloc_free --features internals         # strict CPU invariant
cargo bench --bench alloc_free_gpu                          # GPU baseline gate (no features)
cargo bench --bench alloc_resize --features internals       # resize-path measurement
DHAT_DUMP=1 cargo bench --bench alloc_free --features internals      # emits dhat-heap.json on drop
DHAT_DUMP=1 cargo bench --bench alloc_free_gpu                       # same, for the GPU path
DHAT_DUMP=1 cargo bench --bench alloc_resize --features internals    # same, for the resize path
```

If either fails, load `dhat-heap.json` at
<https://nnethercote.github.io/dh_view/> for per-call-site bytes and
blocks. Don't use these benches for timing — dhat adds 10-30×
allocator overhead.

When the GPU baseline legitimately moves (wgpu/cosmic-text upgrade,
intentional aperture change), bump `RENDER_BLOCKS_PER_FRAME_MAX` in
`benches/alloc_free_gpu.rs` and note the new floor in the PR.

All three benches and the `frame` bench pull `build_ui` + `FormState`
from `benches/support/frame_fixture.rs` via a `#[path]` include — one
synthetic UI tree (~800 nodes, ~500 text shapes at `NODE_SCALE = 32`)
exercising every layout driver, widget, `Shape`, and `Brush` variant
plus the popup/tooltip layers. The `frame_visual` example includes the
same fixture at a smaller scale so a human can eyeball the workload the
benches measure. Grow the fixture and every allocation bench tracks the
new surface area automatically — there is no longer a per-bench mirror
to keep in sync.

## Profiling on macOS

`scripts/profile-bench.sh` records a samply CPU profile and emits a
text report — works headless, no Firefox needed.

### Quick start

```sh
scripts/profile-bench.sh                                    # default: frame bench, 5s
BENCH=frame FILTER='post_record$' scripts/profile-bench.sh    # one case
scripts/profile-bench.sh --profile-time 10                  # longer sample
BENCH=damage FEATURES=internals scripts/profile-bench.sh    # gated bench
TOPN=15 MIN_PCT=1.0 scripts/profile-bench.sh                # tighter cutoffs
CONTEXT=0 scripts/profile-bench.sh                          # skip callers/callees
```

Outputs:
- `tmp/profile-<bench>.json` — raw samply profile. Open interactively
  with `samply load <file>` (serves the Firefox Profiler at
  `127.0.0.1:3000`); the report below is sufficient for most analysis.
- `tmp/profile-<bench>.txt` — flat report:
  - **Self-time top N** with sample count, wall-time (ms), and
    function name (deepest inlined function — atos `-i` expansion).
  - **Inclusive top N**, criterion/std harness filtered.
  - **Callers / callees** for the top 5 self-time entries (immediate
    edges only — full call graph available via `samply load`).

Env:
- `BENCH` — bench target name (default `frame`)
- `FILTER` — criterion regex (default empty = all cases)
- `FEATURES` — cargo features, comma-separated (default empty)
- `TOPN` — rows per section (default 20)
- `MIN_PCT` — drop entries under this % (default 0.5)
- `CONTEXT` — show callers/callees (default 1, set 0 to skip)

Optional dep: `cargo install rustfilt` gives clean Rust v0 demangling.
Without it the script falls back to manual `$LT$`→`<` etc.; legacy
mangling is fine, v0 symbols may show as raw `_RNvCs…`.

### Reading the report

**Self-time** = where the CPU was at sample time (deepest inlined
function at the leaf address). Iterator adapters like
`Map::fold` appearing in the top-10 are real — the body of the
closure inlined into them is the hot code; the report attributes it
to the iterator because that's the named function in the binary.
Walk callees in `samply load` to find the closure source.

**Inclusive** = function appeared anywhere in the stack (per-sample
dedup, so recursion doesn't double-count). Use this to total subsystem
cost: e.g. `Damage::compute` inclusive ≈ everything below it in the
damage call tree.

**Callers / callees** show the immediate parent and child edges by
sample share. A function with one dominant caller and several thin
callees is leaf-ish work concentrated on one path; a function with
many callers indicates a shared utility (often a candidate for
inlining or specialization).

**Red flags to look for:**

- `format_inner` / `String::write_str` / `Vec::reserve` in steady-state
  → per-frame allocation, violates the project's alloc-free posture
  (per `AGENTS.md`). Inspect callers to find the source.
- `HashMap::insert` / `HashMap::rehash` high self-time → a per-frame
  map rebuild that should reuse a retained scratch.
- `core::mem::drop` / `__rust_dealloc` high self-time → drop cost on a
  per-frame data structure; consider clearing-without-dropping or
  `MaybeUninit` reuse.

### Hand-rolling the pipeline

If you need something the script doesn't support (attach to a running
process, sample over a custom window, custom aggregation, diff
against a baseline), the moving parts:

```sh
cargo bench --bench <name> --no-run
BIN=$(ls -t target/release/deps/<name>-* | grep -vE '\.(d|dSYM)$' | head -1)
samply record --save-only -o tmp/raw.json "$BIN" --bench --profile-time 5 '<regex>'
```

Then to symbolicate offline (the `--save-only` JSON contains raw
RVAs; symbolication normally happens at `samply load` time):

1. From `tmp/raw.json` read `frameTable.address` (RVA) and
   `funcTable.resource` (lib index). Keep only frames whose resource
   indexes the bench binary's `libs[]` entry.
2. Get the `__TEXT` vmaddr: `otool -l "$BIN"` — typically
   `0x100000000` on arm64 macOS.
3. Compute absolute addrs (`vmaddr + rva`) — **use python, not BSD
   awk**; awk silently drops the upper 32 bits.
4. Feed absolute addrs to `atos -i -o "$BIN" -l 0x<vmaddr>` **via
   stdin** (`-i` expands inline frames; inline-arg-list silently fails
   past ~30 addrs).
5. Pipe demangled names through `rustfilt` (handles legacy + v0). Don't
   trust `atos` demangling — it doesn't know v0.
6. Walk `stackTable.prefix` from each sample's stack index to
   enumerate the full call stack (leaf first, root last). Per-sample
   `samples.weight` may be null (treat as 1) or a per-sample integer
   when `samples.weightType !== 'samples'` — honor it.
7. Wall-time = `samples × meta.interval` (ms).

`scripts/profile-bench.sh` is ~275 lines doing exactly this; copy it.

### Profiler config notes

- `[profile.bench]` in `Cargo.toml` already builds with `optimized +
  debuginfo`, so symbolication Just Works — no extra flags needed.
- Use `--profile-time N` (criterion arg) for profile runs, not the
  default adaptive loop. It runs each matched case for N seconds of
  uninterrupted measurement.
- Apple Silicon has heterogeneous P/E cores; for stable numbers close
  other apps, run on AC power, and don't move the process between
  P-clusters mid-run. macOS has no `taskset` equivalent — the OS owns
  scheduling.
- `meta.interval = 1.0` ms = samply's default 1 kHz sampling. Bump it
  with `samply record --rate 4000` if you need finer resolution on
  short hot loops.

## GPU profiling on macOS (Metal)

`scripts/profile-metal.sh` captures a **Metal System Trace** of an
example via `xctrace`. Shows the encode→submit→GPU-execute timeline,
named per-pass (`aperture.renderer.main.pass`, `…overlay.damage_rect`)
and per-batch debug groups (`preclear` / `mask` / `quads` / `text` /
`meshes`).

```sh
scripts/profile-metal.sh                                # showcase, 10s
scripts/profile-metal.sh helloworld                     # different example
DURATION=5 scripts/profile-metal.sh                     # shorter capture
HUD=0 scripts/profile-metal.sh                          # skip live HUD overlay
```

Outputs `tmp/metal-<example>.trace` — open with `open
tmp/metal-<example>.trace` (Instruments.app). The script also sets
`MTL_HUD_ENABLED=1` so the running example shows a live frame-time /
GPU-time overlay during capture.

Refuses to run if `MTL_DEBUG_LAYER` or `MTL_SHADER_VALIDATION` are
non-zero in the environment — those add 2-5× draw cost and silently
distort timings.

**What to look for in the trace:**

- GPU-timeline gaps with full CPU encode → frame is GPU-bound.
- CPU encode time eating into the frame budget → CPU-bound; profile
  with `samply` instead.
- Per-pass duration: should be dominated by
  `aperture.renderer.main.pass`. If `overlay.damage_rect` is heavy,
  the debug overlay is on — disable it for production timing.
- Sub-pass debug groups (`quads` / `text` / `meshes`) let you see
  which workload dominates each pass.

**One-shot GPU frame capture** via Xcode's Metal debugger: insert
`device.start_capture(&desc)` / `device.stop_capture()` around one
frame in an example, run it, and Xcode opens the `.gputrace` for
per-draw shader profiling. Not scripted here — usually a manual
investigation tool.

## Profiling on Linux

`scripts/bench-perf.sh` is the Linux companion. It is **vendor-aware**:
it reads `/proc/cpuinfo` `vendor_id` and picks the right PMU layout,
microarch metrics, and precise-sampling mechanism. It pins to one core
(`PIN_CPU`, default 2) and runs up to five passes:

1. **`perf stat`** — hardware counters → IPC, branches, cache, TLB.
   Intel: explicit `cpu_core/.../` events (generic `-e cycles`
   auto-expands across cpu_core + cpu_atom on a hybrid and half-counts).
   AMD: plain `cpu` PMU via `perf stat -d` (homogeneous cores; LLC shows
   `<not supported>` — it's an uncore PMU).
   → `tmp/aperture-perf-stat.txt`
2. **microarch metrics** — Intel: `perf stat -M TopdownL1` (TMA L1
   buckets: retiring / frontend / backend / bad-spec). AMD: `perf stat
   -M branch_prediction,tlb` (Zen<4 has **no** slot-based topdown; Zen4+
   adds `Pipeline_Util_*`, auto-detected). Other AMD groups —
   `l2_cache`, `decoder`, and the uncore `l3_cache`/`data_fabric` (need
   `-a`) — run one at a time for clean (un-multiplexed) counts.
   → `tmp/aperture-perf-micro.txt`
3. **`perf record`** (cycles + callgraph) — the flat/inclusive
   workhorse. `dwarf,16384` default (full depth, ~5-10×). `CALLGRAPH=lbr`
   is Intel-only (AMD Zen3 BRS isn't wired for cycles → falls back to
   dwarf). → `tmp/aperture-perf.data` + `tmp/aperture-perf-report.txt`
4. **precise-IP** (no skid) — Intel: `cpu_core/cycles/ppp` (PEBS). AMD:
   `ibs_op//` (IBS). Tags the exact retiring op, unlike skid-prone cycles
   sampling — pair with `perf annotate` to land on the instruction.
   → `tmp/aperture-perf-ibs.txt`
5. **`perf mem record`** — load/store data-source (cache-level). Intel:
   PEBS `-t load --ldlat=50`. AMD: IBS (no `--ldlat` on Zen<5 — the
   ibs_op `ldlat` cap is absent, and Intel's `50` is outside AMD's valid
   128–2048 range anyway). Report sorted by `mem,sym,dso`.
   → `tmp/aperture-perf-mem.txt`

IBS / raw events / kernel symbols need `kernel.perf_event_paranoid <= -1`
(`sudo sysctl kernel.perf_event_paranoid=-1`); the script warns if it's
higher. For 100% counter coverage also `sudo sysctl kernel.nmi_watchdog=0`
(the watchdog reserves one PMC).

### Usage

```sh
scripts/bench-perf.sh                                # frame bench, 5s
BENCH=damage FILTER='damage/workload' scripts/bench-perf.sh
CALLGRAPH=lbr scripts/bench-perf.sh                  # Intel only; AMD → dwarf
IBS_PERIOD=500000 scripts/bench-perf.sh              # sparser AMD IBS sampling
SKIP_MEM=1 SKIP_MICRO=1 SKIP_IBS=1 scripts/bench-perf.sh   # skip optional passes
FEATURES=internals BENCH=caches scripts/bench-perf.sh
```

Env: `BENCH` (default `frame`), `FILTER` (criterion regex),
`FEATURES` (default `internals`), `CALLGRAPH` (`dwarf`|`lbr`),
`PIN_CPU` (default 2), `FREQ` (cycles Hz, default 4000),
`IBS_PERIOD` (AMD, default 250000), `LDLAT` (Intel PEBS cycles, default
50), `SKIP_MEM`, `SKIP_MICRO`, `SKIP_IBS`.

> The **top-down drill recipe and Raptor-Lake pitfalls below are
> Intel-specific** (cpu_core / TopdownL1 / PEBS). On AMD there's no
> slot-based TMA on Zen<4 — read `perf-micro.txt`'s cache/TLB/branch
> counters and the precise `perf-ibs.txt` directly, then
> `perf annotate -i tmp/aperture-perf-ibs.data <symbol>`.

### Workflow (top-down)

Read in this order — drives sampling effort to where it pays off:

1. **`perf-topdown.txt`** — which TMA bucket dominates?
   - **Retiring >50%**: healthy. Further wins require algorithmic
     changes (fewer instructions retired), not microarch tuning.
   - **Backend_bound >40%** with `memory_bound` dominant → step to
     `perf-mem.txt` for cache-level attribution. With `core_bound`
     dominant → execution-port pressure / dependency chains; drill
     with `perf annotate` on hot symbols.
   - **Frontend_bound >20%** → icache / uop-cache pressure (large
     code, cold paths); look for excessive monomorphization or hot
     loops spanning a 32 KiB icache line.
   - **Bad_speculation >10%** → branch mispredicts; the `branch-misses`
     counter and `perf annotate` jumps confirm.
   - Each TMA leaf prints a `Sampling events:` hint — feed it into
     `perf record -e <event>` to land on the responsible instruction.
2. **`perf-stat.txt`** — IPC = instructions/cycles. Raptor Cove P-core
   peaks ~4-5 IPC, healthy >2.0, stalled <1.0. Compute MPKI
   (misses-per-kilo-instructions) for dTLB and L1-dcache:
   `misses * 1000 / instructions`. dTLB-MPKI >1 → consider huge pages.
3. **`perf-mem.txt`** — when memory_bound: columns bucket loads by
   level (L1/L2/L3/LFB/Local_RAM). High `Local_RAM` = working set
   spills LLC; high `L3` = spills L2; high `LFB` = prefetcher is
   covering you (cheap miss).
4. **`perf annotate -M intel <hot_sym>`** (interactive, on
   `aperture-perf.data`) — pinpoint the exact instruction. Use Intel
   syntax for readability over AT&T.

### Interpretation reference

**IPC isn't a tuning target on its own** — it's a sanity check. Low IPC
in retiring-bound code means the compiler emitted too many
instructions; low IPC in memory-bound code means cache stalls. TMA
tells you which; IPC alone can't.

**Cache miss counts without context are noise.** A 10% L1 miss rate is
fine if those misses hit L2; catastrophic if they hit DRAM. `perf mem`
is the only way to tell.

**Page-faults during steady-state** are the cheap "did we allocate?"
proxy without `dhat` — non-zero after warmup means new pages got
mapped, typically a `Vec::reserve` crossing a page boundary. For exact
allocation attribution use the `alloc_free*` benches with `DHAT_DUMP=1`.

### Hybrid-CPU pitfalls (Raptor Lake)

- Two PMUs: `cpu_core/event/` (P-cores 0-15) and `cpu_atom/event/`
  (E-cores 16-31). The script prefixes every hardware event with
  `cpu_core/.../` and pins with `taskset -c 0`. Don't strip the
  prefix — `-e cycles` reports per-PMU and looks halved.
- TMA metric groups only resolve on `cpu_core`; the cpu_atom event
  variants come back as `<not counted>` on a P-core run, which is
  fine. **Don't pass `--cpu` to the topdown `perf stat`** — it makes
  perf try to attach the cpu_atom event variants to the named CPU,
  and on a P-core target that fails the whole group with "no
  supported events found." `taskset -c 0` alone is sufficient.
- **Multiplexing**: 8 general counters + fixed counters per P-core.
  The HW event group above stays under that limit so measurement
  coverage reads `[100.00%]`. If you add events, split into multiple
  `perf stat` invocations rather than one fat `-e` list — multiplex
  scaling distorts short (<100 ms) runs.
- Thread Director can migrate threads mid-run despite a single-core
  pin if other cores are idle and the migration is "free." Pinning
  via `taskset -c 0` is sufficient for single-threaded benches; for
  multithreaded use `--cpu-list 0-7` with all 8 P-cores' SMT siblings
  ignored (`/sys/devices/cpu_core/cpus`).

### Hand-rolling

```sh
cargo bench --bench frame --features internals --no-run
BIN=$(ls -t target/release/deps/frame-* | grep -v '\.d$' | head -1)
# frame bench requires APERTURE_BENCH_MODE + APERTURE_BENCH_NOTE in env.
export APERTURE_BENCH_MODE=cpu APERTURE_BENCH_NOTE='drill note'

# TMA L1
taskset -c 0 perf stat -M TopdownL1 -- "$BIN" --bench --profile-time 5

# Drill: e.g. backend_bound -> memory_bound -> l3_bound
taskset -c 0 perf stat -M tma_memory_bound_group -- "$BIN" --bench --profile-time 5

# Sample with the event TMA suggested, then annotate
taskset -c 0 perf record -e cpu_core/mem_load_retired.l3_miss/ppp \
    --call-graph lbr -- "$BIN" --bench --profile-time 5
perf annotate -M intel <hot_sym>
```

### Top-down drill recipe (worked example)

The TMA hierarchy drills four levels: L1 bucket → memory sub-bucket →
cache-level sub-bucket → specific event with source-line attribution.
Each step narrows the search before the next:

```sh
# L1 — which of the 4 buckets dominates?
taskset -c 0 perf stat -M TopdownL1 -- "$BIN" --bench cached_cpu --profile-time 4

# If backend_bound dominates: split it into memory vs core.
# memory_bound itself splits into L1/L2/L3/DRAM/Store sub-levels.
taskset -c 0 perf stat -M tma_memory_bound_group -- "$BIN" --bench cached_cpu --profile-time 4

# If e.g. tma_l1_bound dominates (i.e. loads stalling but hitting L1):
# split into store-forwarding / split-loads / fb-full / dtlb.
taskset -c 0 perf stat -M tma_l1_bound_group -- "$BIN" --bench cached_cpu --profile-time 4

# Symmetric for stores:
taskset -c 0 perf stat -M tma_store_bound_group -- "$BIN" --bench cached_cpu --profile-time 4
```

Each `tma_*_group` lists the specific events its metrics derive from
(e.g. `LD_BLOCKS.STORE_FORWARD` for `tma_store_fwd_blk`,
`MEM_LOAD_RETIRED.L3_MISS` for `tma_dram_bound`). Once one leaf
metric is clearly the cost driver, sample that exact event with PEBS
to attribute it to source lines:

```sh
# :ppp suffix = max-precision PEBS — IP attribution lands on the
# offending instruction, not skid-shifted past it. LBR callgraph is
# essentially free; dwarf here would distort the very stalls being
# measured.
taskset -c 0 perf record -e cpu_core/LD_BLOCKS.STORE_FORWARD/ppp \
    --call-graph lbr -o tmp/perf-stfwd.data -- \
    "$BIN" --bench cached_cpu --profile-time 4

perf report -i tmp/perf-stfwd.data --stdio --no-children -g none \
    --percent-limit 1.0 | head -40

# Drill to the exact instruction in the worst symbol:
perf annotate -i tmp/perf-stfwd.data -M intel aperture::forest::Forest::open_node
```

**Reading the L1-bound sub-leaves** (Raptor Cove):

- **`tma_store_fwd_blk`** (`LD_BLOCKS.STORE_FORWARD`) — a load can't
  fast-path from an in-flight store. Causes: narrower load from a
  wider store, wider load from multiple narrow stores, partial
  overlap. ~10-20 cycles per block. Common in `Vec::push` /
  arena-bump append patterns (cursor stored then immediately re-read)
  and SoA pushes where each column is written separately.
- **`tma_split_loads`** (`MEM_INST_RETIRED.SPLIT_LOADS`) — load spans
  two cache lines. Misaligned `#[repr(packed)]` reads, `bytemuck`
  from a non-aligned buffer. Fix: align the source.
- **`tma_fb_full`** (`L1D_PEND_MISS.FB_FULL`) — fill buffers full,
  L1 can't dispatch more misses. Indicates a burst of L1 misses
  exceeding the ~12 fill buffers — bandwidth-bound, not latency-bound.
- **`tma_dtlb_load`** (`DTLB_LOAD_MISSES.WALK_ACTIVE`) — TLB walks.
  Anything >1% MPKI is worth investigating huge pages.

**Reading the store-bound sub-leaves:**

- **`tma_split_stores`** — store spans two cache lines. Same fix as
  split_loads: align the destination.
- **`tma_streaming_stores`** — non-temporal stores active.
  Informational only; usually 0% unless code uses `_mm_stream_*`.
- The unbroken remainder is "store-buffer-full" — too many stores
  in flight. Combine adjacent stores into wider writes
  (`copy_nonoverlapping` of a whole row vs field-by-field).

**Reading the memory-level sub-leaves:**

- **`tma_l1_bound`** — loads stall but eventually hit L1. Not a
  capacity miss; usually store-fwd or split.
- **`tma_l2_bound`** — L1 missed, L2 served. Working set spills L1
  (~48 KiB per core on Raptor Cove). Acceptable for short hot loops.
- **`tma_l3_bound`** — L2 missed, L3 served. Working set spills L2
  (1.25 MiB). Tighter packing or blocking helps.
- **`tma_dram_bound`** — L3 missed. The real "memory locality"
  problem. >5% is worth a `perf mem`-driven layout investigation.

### AMD (Zen) drill recipe

The Intel top-down above doesn't apply on AMD — there's **no slot-based
TMA before Zen4**. `bench-perf.sh` auto-selects the AMD path (IBS +
metric groups); read it like this.

**Finding — the frame bench is retiring-bound.** Every CPU arm runs at
**IPC ≈ 3.3** (Zen3+ peaks ~6) with branch-mispredict < 0.2%, ~3% L1-d
miss, < 4% frontend-idle. The pipeline is busy *retiring instructions*,
not stalling — so wins come from **executing fewer instructions**
(algorithmic / less per-frame recompute), not cache or branch tuning.
The metric groups are confirmation; let the IBS flat report + callgraph
drive. (This is why the O1 intrinsic-cache win came from *deleting* a
sibling re-walk, not microarch tuning — see `docs/cpu-arm-profiling.md`.)

**Drill order:**

1. `tmp/aperture-perf-ibs.txt` — precise (no-skid) self-time
   leaderboard. Trust it over the cycles flat report, whose IP skids
   past the costly instruction.
2. `tmp/aperture-perf-stat.txt` — IPC = insn/cycles. >2.5 with low
   miss rates ⇒ retiring-bound (do less work); <1.0 ⇒ stalled, go to (4).
3. `perf annotate -i tmp/aperture-perf-ibs.data <symbol>` — IBS lands on
   the exact retiring op, so the hot source line is real (no skid).
4. *Only if stalled:* `tmp/aperture-perf-mem.txt` (IBS data-source)
   buckets loads by level — the label column reads `L2 hit` / `L3 hit` /
   `core, same node Any cache hit` / `Local RAM`. Lots of `RAM` = the
   locality problem; mostly `L1`/`L2` is fine.
5. Per-dimension rates — **one metric group per run** (combining them
   oversubscribes the 6 general PMCs → multiplexing, coverage ~14%):

   ```sh
   taskset -c 2 perf stat -M branch_prediction -- "$BIN" --bench cached_cpu --profile-time 4
   taskset -c 2 perf stat -M tlb       -- "$BIN" ...      # i/d-TLB miss rates
   taskset -c 2 perf stat -M l2_cache  -- "$BIN" ...      # l2 hit/miss, ic/dc fill
   taskset -c 2 perf stat -a -M l3_cache -- "$BIN" ...    # uncore (amd_l3) — needs -a
   ```

**IBS knobs** (hand-rolled `perf record -e ibs_op/.../`):

- `-c <period>` = sample period in cycles (`IBS_PERIOD`, default 250000
  ≈ 35k samples / 2 s). Lower = denser + heavier.
- `cnt_ctl=1` = µop-count periods instead of cycles — uniform over ops,
  good for finding high-CPI ops rather than where cycles pool.
- `l3missonly=1` / `ldlat=128..2048` cut overhead (L3-miss-only /
  high-latency loads) — **Zen4+/Zen5+ only**, a no-op on this Zen3+ box.

**Pitfalls (AMD, Family 19h — verified on a Ryzen 7 6800U):**

- `cpu_core/event/` syntax is Intel-hybrid-only — use the bare event
  name (`-e cycles`, not `-e cpu_core/cycles/`).
- L3 / data-fabric counters read `<not counted>` per-process — they're
  uncore (`amd_l3` / `amd_df`), add `-a` (system-wide).
- IBS / raw events / kernel symbols need
  `kernel.perf_event_paranoid <= -1`.
- The NMI watchdog reserves one of the 6 PMCs (`sudo sysctl
  kernel.nmi_watchdog=0` for 100% counter coverage).
- Zen3 has no usable LBR/BRS for cycles, so callgraphs are dwarf-only
  (`CALLGRAPH=lbr` falls back).

## When to use what

- **CPU hotspots**: samply (macOS) / perf (Linux). Always first pass.
  → `scripts/profile-bench.sh` (macOS) or `scripts/bench-perf.sh` (Linux).
- **Microarchitectural attribution** ("where is time really going" when
  the flat profile is flat): Intel TMA via `perf stat -M TopdownL1`,
  then drill into the dominant leaf's metric group. AMD has no
  slot-based topdown before Zen4 — use the metric groups
  (`perf stat -M branch_prediction,tlb,l2_cache`) and the precise IBS
  report instead. Both wired into `bench-perf.sh` (vendor-detected). On
  Apple Silicon use Instruments' "CPU Counters" template with `xcrun
  xctrace record --template 'CPU Counters'`. macOS has no TMA equivalent.
- **Cache-miss attribution** (which loads stall, at which level): Intel
  `perf mem record -t load --ldlat=50` (PEBS); AMD `perf mem record`
  (IBS Op — no `ldlat` before Zen5) + `perf mem report`. Wired into
  `bench-perf.sh`, source-line resolved. macOS has no direct equivalent.
- **Precise instruction attribution** (no sampling skid): Intel PEBS
  (`cycles/ppp`); AMD IBS (`ibs_op//`). The `bench-perf.sh` precise pass
  emits `tmp/aperture-perf-ibs.txt`; `perf annotate -i
  tmp/aperture-perf-ibs.data <symbol>` lands on the exact instruction.
- **False sharing** in multithreaded code: `perf c2c record/report`.
  Not wired in — single-threaded benches don't need it.
- **HW counters** (IPC, L1/L2/TLB miss rates, branch mispredicts) on
  Apple Silicon: Instruments "CPU Counters" template. Limited to 10
  events per run, no multiplexing. Useful for validating SoA/cache
  hypotheses; not scripted.
- **Allocations** (catch steady-state allocs that violate
  alloc-free-per-frame): `alloc_free.rs` bench (assertion mode) or
  `DHAT_DUMP=1` for per-call-site attribution. Samply/perf only show
  CPU time inside the allocator, not allocation counts.
- **GPU work** (wgpu encoder/queue timings): `scripts/profile-metal.sh`
  (macOS Metal System Trace). On Linux, RenderDoc or Tracy.
- **Instruction counts** (stable micro-bench deltas when wall-clock
  variance hides small wins): `iai-callgrind` on Linux. No native
  arm64-darwin port — run in a Linux arm64 CI runner.

**Bench hygiene on Apple Silicon:** P/E core scheduling + thermal
throttling are real sources of variance. For long runs:

```sh
sudo powermetrics --samplers thermal -i 100 -n 200 > tmp/thermal.log &
```

If `thermal_pressure` shifts off `Nominal` mid-run, your variance is
thermal — re-run on power, lid open, with other apps closed.

## Adding a new bench

1. Drop a file under `benches/`, register it in `Cargo.toml`'s
   `[[bench]]` table.
2. If it needs `test_support` reach-in helpers from a production
   module (e.g. `aperture::ui::damage::region::test_support::…`), add
   `required-features = ["internals"]` to the `[[bench]]` entry and
   profile with `FEATURES=internals scripts/profile-bench.sh`. Reach
   in via the canonical per-module `test_support` path — there is no
   `support::internals` aggregator.
3. Name cases `<group>/<case>` so criterion filters work consistently
   with the profile-bench script.
4. After landing, profile once and paste the text report into the PR
   description as the steady-state baseline.
