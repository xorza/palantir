# Benches

One target, `criterion`, holds every driver and needs `--features bench`.
Separate targets under the fat-LTO `[profile.bench]` linked in parallel and
risked OOM. The drivers sit in `bench.rs` files beside the code they measure;
`src/bench/mod.rs` says why.

## Running

```sh
cargo bench -p palantir --features bench --bench criterion -- --list-drivers
cargo bench -p palantir --features bench --bench criterion -- -d cascade 'hit_test$'
cargo bench -p palantir --features bench --bench criterion -- -d frame --arms cpu --note 'after belt rework'
```

- `-d` selects drivers by exact name, and an unselected driver never runs
  its setup. A positional is criterion's regex over benchmark ids, which
  filters only after the setup. `--arms cpu|gpu|both` picks a half of the
  pipeline. `--help` lists the rest.
- `frame` is opt-in. Its full matrix takes ~110 s (`--arms cpu`, ~50 s) and
  appends a row to `benches/results/<machine>.txt`, so it needs `--note`.
- The binary reads no environment variable. Every knob is a flag.
- Cut `--sample-size` and `--measurement-time` while you iterate. Report
  only numbers from a full run.

**The GPU arms present the way the desktop does.** `bench_host` builds its
`OffscreenHost` with `retained_target(true)`, which is the winit host's
`DirectAdaptive`. A desktop can never rely on what a swapchain image held,
and `DirectAdaptive` never reads it: a skip frame presents nothing, a full
frame renders straight into the target, and a partial frame paints into
palantir's own backbuffer and copies all of it out. The default
`BackbufferCopy` is the screenshot path. It also copies out on skip and
full frames, which costs 1.3 ms on a 1440p `cached_gpu` frame, so rows in
`benches/results` from before 2026-10-06 do not compare with later ones.

**The frame bench writes no timestamp inside a pass.** Its device is
`Timing::PassOnly`, so the `write_stats` dump times the whole pass and
gives no per-kind split, and the timed arms collect no GPU stats. Before
2026-10-06 the timed arms wrote a timestamp at each batch-kind change
inside the main pass. On a tiler such as the Pi 5's V3D, each one splits
the pass, and a 1440p `scrolling_gpu` frame took 149 ms against 45 ms
without them. Earlier `*_gpu` rows do not compare with later ones. The
`image_pipeline` and `curve_pipeline` per-kind times have the same
problem on a tiler.

**`cached_cpu` paints nothing, as `cached_gpu` does.** A still frame plans
no paint, and the CPU harness encodes and composes only what a frame
planned. Before 2026-10-06 it repainted the whole scene on such a frame,
about 13 µs on the 6800U, so earlier `cached_cpu` rows do not compare with
later ones.

## Measuring

- **Pin the run, never the build.** `taskset` in front of `cargo` pins the
  compiler too.
- **One core, sibling idle.** Pin to one core and keep its SMT sibling idle.
  On this machine CPU 2 shares a core with CPU 3.
- **`setarch -R`.** It fixes the address layout, so stack and heap alignment
  do not change between processes.
- **Governor and EPP at `performance`** (`sudo /usr/local/sbin/cpu-bench-pin.sh`
  here). The SMU ignores boost caps, so a drift of several percent between
  runs stays.
- **No `RUSTFLAGS`.** It replaces the rustflags of `.cargo/config.toml` and
  drops `target-cpu=x86-64-v3`. Add a flag with `--config`, which merges:
  `--config "target.'cfg(target_arch = \"x86_64\")'.rustflags=['-C','force-frame-pointers=yes']"`.
  Each flag set is a fresh fat-LTO link, ~2 min.

**A/B in ABBA order.** A drift reads as a change in one direction only. Run
before, after, after, before, and trust a change only when both comparisons
report it, mirrored. Run from the repository root: without cargo, criterion
keeps baselines in `./target/criterion`.

```sh
bench_bin() {
    cargo bench -p palantir --features bench --bench criterion --no-run 2>&1 |
        sed -n 's/.*Executable .*(\(.*\))$/\1/p'
}
cp "$(bench_bin)" tmp/criterion-a
# ... make the change ...
cp "$(bench_bin)" tmp/criterion-b
RUN=(taskset -c 2 setarch -R)
"${RUN[@]}" tmp/criterion-a --bench -d cascade --save-baseline a
"${RUN[@]}" tmp/criterion-b --bench -d cascade --baseline a   # B against A
"${RUN[@]}" tmp/criterion-b --bench -d cascade --save-baseline b
"${RUN[@]}" tmp/criterion-a --bench -d cascade --baseline b   # A against B
```

**Bisecting through `git archive`.** An extracted file's mtime is its
commit's time. Extract several commits into one directory and build them
into one target directory, and an older commit reads to Cargo as "not
changed": it rebuilds the crate only when the version, the lock file or
the profile changes, and otherwise hands back the binary it built for the
commit before.
Touch every extracted file before the build, or give each commit its own
target directory.

## Profiling

`benches/bench-perf.sh` (Linux) profiles the bench under the rules above,
with `perf` on the other cores. Its header lists the options. It needs
`sudo sysctl kernel.perf_event_paranoid=-1 kernel.nmi_watchdog=0`, and
warns when either is missing. It builds the binary `cargo bench` builds,
so a profile after a bench run, or a bench run after a profile, rebuilds
nothing.

| pass        | Intel                         | AMD                         | output in `tmp/`                     |
| ----------- | ----------------------------- | --------------------------- | ------------------------------------ |
| counters    | `cpu_core/…/` events          | `perf stat -d`              | `palantir-perf-stat.txt`             |
| microarch   | `-M TopdownL1`                | `-M branch_prediction,tlb`¹ | `palantir-perf-micro.txt`            |
| callgraph   | cycles, `dwarf,65528` or LBR  | cycles, `dwarf,65528`       | `palantir-perf.data`, `-report.txt`  |
| precise IP  | `cycles/ppp` (PEBS)           | `ibs_op//` (IBS)            | `palantir-perf-ibs.data`, `-ibs.txt` |
| data source | `perf mem -t load --ldlat=50` | `perf mem`                  | `palantir-perf-mem.data`, `-mem.txt` |

¹ `Pipeline_Util_Level1` where perf offers it (Zen4+).

`benches/profiling.md` says how to read the output and how to drill past it
by hand.
