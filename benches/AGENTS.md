# Benches

One target, `criterion`, holds every driver: separate targets under the
fat-LTO `[profile.bench]` linked in parallel and risked OOM. The drivers sit
in `bench.rs` files beside the code they measure; `src/bench/mod.rs` says why.

## Running

```sh
cargo bench -p palantir --bench criterion -- --list-drivers
cargo bench -p palantir --bench criterion -- -d cascade 'hit_test$'
cargo bench -p palantir --bench criterion -- -d frame --arms cpu --note 'after belt rework'
```

- `-d` selects drivers by exact name; an unselected driver never runs its
  setup. A positional is criterion's regex over benchmark ids, which filters
  only after the setup. `--arms cpu|gpu|both` picks a half of the pipeline.
  `--help` lists the rest. The binary reads no environment variable.
- `frame` is opt-in: its ten arms take over 15 s each (3 s warm-up, 12 s
  window), and it appends a row to `benches/results/<machine>.txt`, so it
  needs `--note`.
- A driver's window is its `config` row in `src/bench/driver.rs`: 50 samples
  over 5 s after a 1 s warm-up, unless the row says otherwise. A group never
  sets one, as a group's setting outranks the flags; clippy enforces it. Cut
  `--sample-size` and `--measurement-time` while you iterate, and report only
  a full run's numbers.

**The frame bench measures what the desktop pays.**

- **The GPU arms present as the desktop does.** `bench_host` builds its
  `OffscreenHost` with `retained_target(true)`, the winit host's
  `DirectAdaptive`: a skip frame presents nothing, a full frame renders
  straight into the target, and a partial frame paints into palantir's
  backbuffer and copies all of it out. The default `BackbufferCopy` is the
  screenshot path, which also copies on skip and full frames: 1.3 ms on a
  1440p `cached_gpu` frame.
- **No timestamp inside a pass.** The device is `Timing::PassOnly`, so the
  `write_stats` dump times whole passes, and the timed arms collect no GPU
  stats. On a tiler such as the Pi 5's V3D each in-pass timestamp splits the
  pass: a 1440p `scrolling_gpu` frame took 149 ms with them against 45 ms
  without. The `image_pipeline` and `curve_pipeline` per-kind times still
  write them, at the same cost on a tiler.
- **`cached_cpu` paints nothing, as `cached_gpu` does**: a still frame plans
  no paint, and the CPU harness encodes and composes only what a frame
  planned.

Rows in `benches/results` from before 2026-10-06 do not compare with later
ones: the GPU arms then took the copy path, the timed arms wrote in-pass
timestamps, and `cached_cpu` repainted the whole scene (about 13 µs on the
6800U).

## Measuring

- **Pin the run, never the build**: `taskset` in front of `cargo` pins the
  compiler too.
- **One core, its SMT sibling idle.** `lscpu -e` lists each CPU's core; on
  the rig these notes come from, CPU 2 shares a core with CPU 3.
- **`setarch -R`** fixes the address layout, so alignment does not change
  between processes.
- **Governor and EPP at `performance`** (`sudo
  /usr/local/sbin/cpu-bench-pin.sh` here). The SMU ignores boost caps, so a
  drift of several percent between runs stays.
- **No `RUSTFLAGS`**: it replaces `.cargo/config.toml`'s rustflags and drops
  `target-cpu=x86-64-v3`. Add a flag with `--config`, which merges:
  `--config "target.'cfg(target_arch = \"x86_64\")'.rustflags=['-C','force-frame-pointers=yes']"`.
  Each flag set is a fresh fat-LTO link, ~2 min.

**A/B in ABBA order.** Drift reads as a change in one direction only: run
before, after, after, before, and trust a change only when both comparisons
report it, mirrored. Run from the repository root: without cargo, criterion
keeps baselines in `./target/criterion`.

```sh
bench_bin() {
    cargo bench -p palantir --bench criterion --no-run 2>&1 |
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
commit's time, so commits extracted into one directory and built into one
target directory read to Cargo as unchanged: it hands back the binary of the
commit before. Touch every extracted file before the build, or give each
commit its own target directory.

## Profiling

`benches/bench-perf.sh` (Linux) profiles the bench under the rules above,
with `perf` on the other cores; its header lists the options. It needs
`sudo sysctl kernel.perf_event_paranoid=-1 kernel.nmi_watchdog=0`, and warns
when either is missing. It builds the binary `cargo bench` builds, so neither
rebuilds after the other.

| pass        | Intel                         | AMD                         | output in `tmp/`                     |
| ----------- | ----------------------------- | --------------------------- | ------------------------------------ |
| counters    | `cpu_core/…/` events          | `perf stat -d`              | `palantir-perf-stat.txt`             |
| microarch   | `-M TopdownL1`                | `-M branch_prediction,tlb`¹ | `palantir-perf-micro.txt`            |
| callgraph   | cycles, `dwarf,65528` or LBR  | cycles, `dwarf,65528`       | `palantir-perf.data`, `-report.txt`  |
| precise IP  | `cycles/ppp` (PEBS)           | `ibs_op//` (IBS)            | `palantir-perf-ibs.data`, `-ibs.txt` |
| data source | `perf mem -t load --ldlat=50` | `perf mem`                  | `palantir-perf-mem.data`, `-mem.txt` |

¹ `Pipeline_Util_Level1` where perf offers it (Zen4+).

`benches/profiling.md` says how to read the output and how to go past it by
hand.
