# Benches

Criterion benches for the layout/measure/frame/cascade/damage pipeline.

Each `*.rs` file is a criterion target; cases inside are named like
`<group>/<case>` (e.g. `frame/end_frame`, `frame/end_frame_resizing`).
Filter at run-time with a criterion regex.

## Running

```sh
cargo bench --bench frame                              # all cases in frame.rs
cargo bench --bench frame -- 'end_frame$'              # one case (regex, anchored)
cargo bench --bench caches --features internals        # gated benches
```

`caches`, `cascade`, `damage`, `damage_merge_gpu` are gated behind
`internals` / `bench-deep`. `cargo bench --no-run` without features only
builds `frame`.

## Profiling on macOS

`scripts/profile-bench.sh` records a samply CPU profile and emits a
text report — works headless, no Firefox needed.

### Quick start

```sh
scripts/profile-bench.sh                                    # default: frame bench, 5s
BENCH=frame FILTER='end_frame$' scripts/profile-bench.sh    # one case
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
  (per `CLAUDE.md`). Inspect callers to find the source.
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

`scripts/profile-bench.sh` is ~150 lines doing exactly this; copy it.

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

## Profiling on Linux

`scripts/bench-perf.sh` is the Linux companion: `perf record` +
`perf stat`, pinned to a P-core. It also captures hardware counters
(IPC, cache, branches, page faults) that samply doesn't.

## When to use what

- **CPU hotspots**: samply (macOS) / perf (Linux). Always first pass.
- **Allocations** (steady-state allocs that violate alloc-free-per-frame):
  `dhat-heap` integrated into the bench, or `heaptrack` on Linux.
  Samply/perf only show CPU time inside the allocator, not allocation
  counts. `perf stat`'s `page-faults` is a cheap proxy.
- **GPU work** (wgpu encoder/queue timings): Instruments' Metal System
  Trace on macOS, RenderDoc or Tracy on Linux. Out of scope for the
  criterion benches in this folder.
- **Instruction counts** (stable micro-bench deltas when wall-clock
  variance hides small wins): `iai-callgrind` on Linux. Not wired up.

## Adding a new bench

1. Drop a file under `benches/`, register it in `Cargo.toml`'s
   `[[bench]]` table.
2. If it needs `support::internals` reach-in, add
   `required-features = ["internals"]` to the `[[bench]]` entry and
   profile with `FEATURES=internals scripts/profile-bench.sh`.
3. Name cases `<group>/<case>` so criterion filters work consistently
   with the profile-bench script.
4. After landing, profile once and paste the text report into the PR
   description as the steady-state baseline.
