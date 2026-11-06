# Benches

The target `palantir` in this crate runs every case through `bench::run`, and the
target `damage_sweep` is a study of its own (**The damage sweep**), which `cargo bench-all`
leaves out; each
driver sits in a `bench.rs` beside the code it measures in palantir's `src/`, and
`src/bench/mod.rs` states the rules: the kinds, the ids, one number for each case, and the
`Scale` sampling policy (`clippy.toml` keeps a group from setting its own).

## Which cases

The showcase's first page (`Showcase`, a `bench::scene::Scene`) is the
representative screen: `integration/frame` runs it whole, and one atomic group
per stage runs that stage alone over it, so a moved frame points at its stage.

| stage | over the scene | on made inputs |
|---|---|---|
| input | | `atomic/input`, `atomic/hit_test` |
| record | `atomic/record` | `atomic/text_edit`, `atomic/animation` |
| layout | `atomic/layout` | `atomic/text_shape`, `integration/measure_cache` |
| cascade | `atomic/cascade` | |
| damage | | `atomic/damage`, `integration/damage`, `integration/damage_policy` |
| encode + compose | `atomic/encode` | `atomic/composer`, `atomic/rect_grid` |
| paint, CPU | | `atomic/record_pass`, `atomic/schedule` |
| paint, GPU | `atomic/main_pass`, `atomic/main_pass_batch` | `atomic/*_pipeline`, `atomic/*_atlas`, `atomic/backdrop_*` |
| the frame | `integration/frame`, `integration/backdrop_frame` | |

`integration/frame` arms: `cached` (a still frame), `partial` and
`alternating` (state changes), `scrolling` and `resizing` (whole repaints),
`hovering` and `typing` (input driving the frame), `theming` (a theme
transition in progress). Every case of a group has
one id depth (`CaseGroup` holds it), so a filter on a level selects alike.

## The damage sweep

`cargo damage-sweep` times what each way of repainting a frame's damage costs on this GPU, and
scores every setting of the merge budget, the rect cap and the promote rule against it. Its
reasons sit in `src/damage/sweep/mod.rs`.

```sh
cargo damage-sweep                            # every stage
cargo damage-sweep -- scene/pair/             # what a `<stage>/<scenario>` holds
cargo damage-sweep -- --list                  # each case and its arms
cargo damage-sweep -- --rounds 24 --csv tmp/damage-sweep
PALANTIR_BENCH_NO_GPU=1 cargo damage-sweep    # each decision, with the pair grid, and no timing
```

Each stage prints a table and a score:

- **The table** has a row per scenario: the arm production takes, the cheapest arm, each with its
  time and CPU half, the whole repaint, and the regret between them, read round by round, `tie`
  when the gap lies inside the rounds' spread. An arm's name says which raw
  rects each repainted rect holds: `AB|C` is two scissors, `A*15/full` a cycle of 15 frames of `A`
  and a whole one, and past eight raw rects a count, `4 rects #2` the second arm of that count.
  A session's scenario is named by what it covers, as its counts move a few frames from run to run.
- **The score** gives the five best policies, how many score better than production, and each knob
  swept with the others at production's values. A policy reads by its worst regret, then by its mean
  regret per frame, where a scenario of a scripted session counts its frames and a made one
  counts one. `mean %` is that regret as a share of each scenario's best frame, which compares
  across hosts, and `longer %` the same with a frame read as its longer half.

How to read it:

- **A cost holds for this GPU.** A number never moves to another host; the stages only compare
  against each other on one. Run it on each GPU class, and a knob the classes disagree on is a
  knob that wants a device rule. A Linux board builds nothing: its binary is cross-built, as
  `.notes/PERF.md` says.
- **A frame's time is its two halves added**: the host until its submit returns, then the wait for
  the device. In a window the two overlap, which `longer %` reads, and a policy that wins only one
  way favors what its host is short of.
- **Time alone.** A smaller repaint draws less power even at a tie.
- **`dense` against `plain` asks whether a cost follows the draws**, as a replay per scissor makes
  it; **`dense_1x` and `dense_4k` against `dense`** whether it follows the pixels, while a budget
  counts logical px² whatever the scale.
- **`scene` alone holds the session**, whose rects describe that screen.
- **`scene_glass` against `scene` asks what panes add**: few rows, and much of a whole frame's cost
  in its captures, which a partial frame leaves held.
- **Rects scale with the surface**, so a share stays a share: `dense_4k`'s icons are 1.5× larger,
  which keeps the coverage and not the logical sizes.

A session runs over the showcase only; a darkroom session's rects would need a recording of their
own. The regression cases `integration/damage_policy/gpu/<scenario>` run production's decision on
eight of these scenarios in a plain `cargo bench-all`.

## Running

```sh
cargo bench-all -- --list
cargo bench-all -- '^atomic/hit_test/'
cargo bench-all -- '/cpu/'                 # every case that opens no device
cargo bench-all -- --exact integration/frame/gpu/cached
cargo bench-all -- '^integration/frame/' --quick
cargo bench-once                           # each case once, built under the test profile
```

- An id is `<kind>/<group>/<backend>/<rest…>`, and criterion's filter is a
  regex over it. A case builds its inputs in its own closure, so a filtered
  run pays for nothing it does not time, and `'/cpu/'` opens no device;
  `BenchGpu::shared` panics if a group's body opens one. The adapter probe
  at startup still loads the GPU driver; `PALANTIR_BENCH_NO_GPU=1` skips it
  and every GPU case.
- A host without a GPU adapter runs no `gpu` case and says so once.
- `--quick` while you iterate; report only a full run's numbers.

**The frame cases measure what the desktop pays.**

- **The GPU frames present as the desktop does.** `bench_host` builds its
  `OffscreenHost` with `swapchain_present`, the winit host's
  `DirectAdaptive`: a skip frame presents nothing, a full frame renders
  straight into the target, and a partial frame paints into palantir's
  backbuffer and copies all of it out. The default `BackbufferCopy` is the
  screenshot path, which also copies on skip and full frames: 1.3 ms on a
  1440p `cached` frame.
- **No timestamp inside a pass.** The frame device is `Timing::PassOnly`, and
  the timed frames collect no GPU stats. On a tiler such as a VideoCore GPU
  each in-pass timestamp splits the pass: a 1440p `scrolling` frame took
  149 ms with them against 45 ms without. The `image_pipeline` and
  `curve_pipeline` cases time their batch with them, at the same cost on a
  tiler.
- **`cpu/cached` paints nothing, as `gpu/cached` does**: a still frame plans
  no paint, and the CPU harness encodes and composes only what a frame
  planned.

## Measuring

- **Pin the run, never the build**: `taskset` in front of `cargo` pins the
  compiler too.
- **One core, its SMT sibling idle.** `lscpu -e` lists each CPU's core.
- **`setarch -R`** fixes the address layout, so alignment does not change
  between processes.
- **Governor and EPP at `performance`.** The SMU ignores boost caps, so a
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

**Bisecting through `git archive`.** An extracted file's mtime is its
commit's time, so commits extracted into one directory and built into one
target directory read to Cargo as unchanged: it hands back the binary of the
commit before. Touch every extracted file before the build, or give each
commit its own target directory.

## Profiling

`palantir-showcase/benches/bench-perf.sh` (Linux) profiles the bench under the
rules above, with `perf` on the other cores; its header lists the options. A filter that
selects CPU cases alone runs with `PALANTIR_BENCH_NO_GPU=1`, so the driver
stays out of the profile. It needs
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

`palantir-showcase/benches/profiling.md` says how to read the output and how to
go past it by hand.
