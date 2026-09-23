# Benches

Criterion benches over the frame pipeline, and the profiling script that
drives them. How to invoke the script is in `benches/bench-perf.sh`'s
header; how to read what it writes is in `benches/profiling.md`.

## Running them

One target, `criterion`, `harness = false` with its own `main` and every
timing driver in it: `[profile.bench]` is fat-LTO with one codegen unit,
and twenty parallel links of the whole dependency graph was an OOM
risk.

`criterion` takes `--driver` (exact, repeatable) and `--list-drivers`;
`--arms` picks a half of the pipeline, and a bare positional is
criterion's own regex over benchmark ids.

`--features bench` is not optional: the target declares
`required-features = ["bench"]`, and cargo refuses to build it without
one.

```sh
cargo bench -p palantir --features bench --bench criterion -- --list-drivers
cargo bench -p palantir --features bench --bench criterion -- -d damage
cargo bench -p palantir --features bench --bench criterion -- --arms cpu
cargo bench -p palantir --features bench --bench criterion -- 'cascade/hit_test$'
cargo bench -p palantir --features bench --bench criterion -- -d frame --arms cpu --note 'after belt rework'
```

`--help` lists the rest — `--profile-time`, `--sample-size`,
`--measurement-time`, `--warm-up-time`, `--noplot`, and the frame
bench's `--size`, `--scale`, `--machine`, `--note`.

**A/B a change** with the baseline pair, which is the only way to
compare two builds without the machine's own drift confounding it —
back-to-back runs on a busy machine move several percent on their own:

```sh
cargo bench -p palantir --features bench --bench criterion -- -d cascade --save-baseline before
# ... make the change ...
cargo bench -p palantir --features bench --bench criterion -- -d cascade --baseline before
```

`--baseline` fails if a selected benchmark has no sample under that
name; `--baseline-lenient` leaves it uncompared instead.

**Every input is a flag.** No bench reads an environment variable of its
own: one parser per target, and what it resolves is handed down in a
`Run`. A knob that has to reach a driver goes on the CLI — never into
the environment, where nothing declares it and `--help` can't list it.

**`frame` is opt-in** — `-d frame`. It is the one driver kept out of a
bare run: the full matrix is ~90 s and appends a results row to
`benches/results/<machine>.txt` that demands a `--note`.

`--driver` is exact: an unselected driver never runs, so it costs
nothing and stays out of the profile. That is why the runner lives in
`src/bench/` and owns `main` rather than using `criterion_main!` — see
its module doc for the mechanism.

## Profiling

`benches/bench-perf.sh` — Linux only, needs `perf` + `taskset`. Reads
`vendor_id` and picks the PMU layout, metrics, and precise-sampling
mechanism to match, pins to one core, and runs five passes, each written
to `tmp/palantir-perf-*`:

| pass | Intel | AMD | output |
|---|---|---|---|
| counters | `cpu_core/…/` events | `perf stat -d`¹ | `-stat.txt` |
| microarch | `-M TopdownL1` (TMA) | `-M branch_prediction,tlb`² | `-micro.txt` |
| callgraph | `perf record` cycles, `dwarf,16384`³ | same | `.data`, `-report.txt` |
| precise-IP | `cycles/ppp` (PEBS) | `ibs_op//` (IBS) | `-ibs.data`, `-ibs.txt` |
| data-source | `perf mem -t load --ldlat=50` | `perf mem` (no `ldlat` pre-Zen5) | `-mem.data`, `-mem.txt` |

¹ LLC reads `<not supported>`; it's an uncore PMU. ² **Zen<4 has no
slot-based topdown**, which is why the TMA drill in `profiling.md` is
Intel-only; Zen4+ adds `Pipeline_Util_*`, auto-detected. ³ `CALLGRAPH=lbr`
is Intel-only — Zen3's BRS isn't wired for cycles and silently falls back.

Needs `sudo sysctl kernel.perf_event_paranoid=-1` (IBS, raw events,
kernel symbols) and `kernel.nmi_watchdog=0` (the watchdog reserves a
PMC, so coverage never reads 100% with it on). The script warns on both.

Two config facts it leans on: `[profile.bench]` already builds optimized
with line-table debuginfo, so symbolication needs no extra flags; and
`--profile-time N` beats criterion's adaptive loop, because a fixed
window makes sample counts comparable across runs.

Read `benches/profiling.md` before interpreting a capture or hand-rolling
a `perf` command: it holds the reading order, the Intel drill, and the
hybrid-CPU and AMD pitfalls.

### Budget

Run the script before hand-rolling `perf record` — it already knows the
vendor, PMU prefix, call-graph mechanism, and pinning. Skipping it turned
a ten-minute pass into an hour:

- **`perf report -g graph,…` over a whole capture never finishes** — 10
  min, no output, 16 MB file. For "who calls X", filter `perf script`
  stacks to those containing X and tally the frame above it. `perf
  report` is for flat self-time (`-g none`), where it is instant.
- **dwarf profiles perf itself.** `--call-graph dwarf,16384` at 3 kHz
  wrote 577 MB in 12 s, and its own writeback read as ~25% kernel
  page-fault and FS time — indistinguishable from a real finding. The
  same run flat is 7 MB.
- **dwarf does not unwind this binary** — 146 of 76 000 stack lines
  resolved against the 137 MB fat-LTO build. Call graphs need
  `CALLGRAPH=lbr`, or `-C force-frame-pointers=yes` and a 2-min relink.
- **Startup is ~1 s** (font DB), so 8% of a 12 s window and
  `fontdb::parse_face_info` near the top. `-D <ms>` skips it;
  `--profile-time 4` is enough for flat self-time.

Budget rather than discover: each `RUSTFLAGS` variant is a fresh fat-LTO
relink (~2 min), and a four-arm `--arms cpu` run is ~40 s at stock
statistics. Cut `--sample-size` / `--measurement-time` while iterating;
spend the full run on the number you report.
