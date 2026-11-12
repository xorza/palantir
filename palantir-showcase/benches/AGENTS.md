# Benches

The target `palantir` runs every case through `bench::run`; `damage_sweep` and `atlas_memory` are
studies of their own, which `cargo bench-all` leaves out. Each driver sits in a `bench.rs` beside the code it measures,
and `src/bench/mod.rs` states the rules: the kinds, the ids, one number a case, and the sampling
policy.

## Which cases

The showcase's first page is the representative screen: `integration/frame` runs it whole, and
one atomic group per stage runs that stage alone over it, so a moved frame points at its stage. A
case on made inputs covers what the page does not hold. Every case of a group has one id depth, so
a filter on a level selects alike.

## Running

```sh
cargo bench-all -- --list
cargo bench-all -- '^atomic/hit_test/'
cargo bench-all -- '/cpu/'                 # every case that opens no device
cargo bench-all -- --exact integration/frame/gpu/cached
cargo bench-all -- '^integration/frame/' --quick
cargo bench-once                           # each case once, under the test profile
```

- An id is `<kind>/<group>/<backend>/<rest…>`, and criterion's filter is a regex over it. A case
  builds its inputs in its own closure, so a filtered run pays only for what it times.
  `PALANTIR_BENCH_NO_GPU=1` skips the adapter probe and every GPU case; a host with no adapter
  runs no GPU case.
- `--quick` while you iterate; report only a full run's numbers.
- **The frame cases measure what the desktop pays**: they present as the winit host does, not
  through the screenshot path, which copies on every frame.
- **No timestamp inside a pass**: on a tiler such as VideoCore each one splits the pass and slows
  the frame several times over.

## Measuring

- **Pin the run, never the build**: `taskset` in front of `cargo` pins the compiler too. One core,
  its SMT sibling idle; `setarch -R` fixes the address layout; governor and EPP at `performance`.
- **No `RUSTFLAGS`**: it replaces `.cargo/config.toml`'s rustflags. Add a flag with `--config`,
  which merges:
  `--config "target.'cfg(target_arch = \"x86_64\")'.rustflags=['-C','force-frame-pointers=yes']"`.
- **A/B in ABBA order**: before, after, after, before, and trust a change only when both
  comparisons report it. Run from the repository root, as criterion keeps baselines in
  `./target/criterion`:

```sh
bench_bin() {
    cargo bench -p palantir-showcase --bench palantir --no-run 2>&1 |
        sed -n 's/.*Executable .*(\(.*\))$/\1/p'
}
cp "$(bench_bin)" tmp/criterion-a
# ... make the change ...
cp "$(bench_bin)" tmp/criterion-b
RUN=(taskset -c 2 setarch -R)
"${RUN[@]}" tmp/criterion-a --bench cascade/ --save-baseline a
"${RUN[@]}" tmp/criterion-b --bench cascade/ --baseline a   # B against A
"${RUN[@]}" tmp/criterion-b --bench cascade/ --save-baseline b
"${RUN[@]}" tmp/criterion-a --bench cascade/ --baseline b   # A against B
```

- **Bisecting through `git archive`**: an extracted file's mtime is its commit's, so Cargo reads
  commits built into one target directory as unchanged. Touch the files, or give each commit its
  own target directory.

## The damage sweep

`cargo damage-sweep` times each way of repainting a frame's damage on this GPU and scores every
setting of the merge budget, the rect cap and the promote rule against it; its reasons and its
output are in `src/damage/sweep/mod.rs`.

```sh
cargo damage-sweep                            # every stage
cargo damage-sweep -- scene/pair/             # one `<stage>/<scenario>`
cargo damage-sweep -- --list                  # each case and its arms
cargo damage-sweep -- --rounds 24 --csv tmp/damage-sweep
PALANTIR_BENCH_NO_GPU=1 cargo damage-sweep    # each decision, no timing
```

- **A cost holds for one GPU**: run it on each GPU class, and a knob the classes disagree on wants
  a device rule.
- **A frame's time is its two halves added**, the host until its submit returns and the wait for
  the device; a policy that wins only one way favors what its host is short of.
- **Compare scenarios that differ in one thing**: the draws, the pixels, the materials.

## The atlas memory

`cargo atlas-memory` runs each arm of `integration/frame` and every backdrop look and scene, a
host of its own each, and prints per run what each GPU atlas holds: its peak bytes, its bytes at
the last frame, the bytes its live items fill and their share. It reads memory, not time, which no build profile moves, so
`cargo test -p palantir-showcase --bench atlas_memory -- --bench` runs it without a fat link; its
reasons are in `src/gpu/atlas_memory/bench.rs`.

## Profiling

`palantir-showcase/benches/bench-perf.sh` (Linux) profiles the bench under the rules above, with
`perf` on the other cores; its header lists the passes and options, and
`palantir-showcase/benches/profiling.md` says how to read them. A CPU-only filter runs with
`PALANTIR_BENCH_NO_GPU=1`, so the driver stays out of the profile. It needs
`sudo sysctl kernel.perf_event_paranoid=-1 kernel.nmi_watchdog=0`.
