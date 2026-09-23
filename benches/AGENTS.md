# Benches

Criterion benches over the frame pipeline, and `benches/bench-perf.sh`, which
profiles them. The script's header says how to invoke it;
`benches/profiling.md` says how to read what it writes.

## Running them

One target, `criterion` (`harness = false`, its own `main`), holds every
driver: under the fat-LTO `[profile.bench]`, separate targets meant parallel
whole-graph links that risked OOM. It requires `--features bench`.

```sh
cargo bench -p palantir --features bench --bench criterion -- --list-drivers
cargo bench -p palantir --features bench --bench criterion -- -d damage
cargo bench -p palantir --features bench --bench criterion -- --arms cpu
cargo bench -p palantir --features bench --bench criterion -- 'cascade/hit_test$'
cargo bench -p palantir --features bench --bench criterion -- -d frame --arms cpu --note 'after belt rework'
```

`-d`/`--driver` is exact and repeatable — an unselected driver never runs, so
it costs nothing and stays out of a profile. `--arms` picks a half of the
pipeline; a bare positional is criterion's regex over benchmark ids. `--help`
lists the rest.

**`frame` is opt-in.** A bare run skips it: the full matrix is ~90 s and
appends a row to `benches/results/<machine>.txt`, which demands a `--note`.

**A/B a change with a baseline pair** — back-to-back runs on a busy machine
drift several percent on their own:

```sh
cargo bench -p palantir --features bench --bench criterion -- -d cascade --save-baseline before
# ... make the change ...
cargo bench -p palantir --features bench --bench criterion -- -d cascade --baseline before
```

`--baseline` fails on a benchmark with no sample under that name;
`--baseline-lenient` leaves it uncompared.

**Every input is a flag.** No bench reads an environment variable: a knob a
driver needs goes on the CLI, where `--help` lists it, and reaches the driver
in the resolved `Run`.

## Profiling

`benches/bench-perf.sh` (Linux; needs `perf` and `taskset`) detects the CPU
vendor, pins one core, and runs five passes into `tmp/palantir-perf-*`:

| pass | Intel | AMD | output |
|---|---|---|---|
| counters | `cpu_core/…/` events | `perf stat -d`¹ | `-stat.txt` |
| microarch | `-M TopdownL1` (TMA) | `-M branch_prediction,tlb`² | `-micro.txt` |
| callgraph | `perf record` cycles, `dwarf,16384`³ | same | `.data`, `-report.txt` |
| precise-IP | `cycles/ppp` (PEBS) | `ibs_op//` (IBS) | `-ibs.data`, `-ibs.txt` |
| data-source | `perf mem -t load --ldlat=50` | `perf mem` (no `ldlat` pre-Zen5) | `-mem.data`, `-mem.txt` |

¹ LLC reads `<not supported>` — it is an uncore PMU. ² Zen<4 has no
slot-based topdown; Zen4+ adds `Pipeline_Util_*`, auto-detected.
³ `CALLGRAPH=lbr` is Intel-only — Zen3's BRS silently falls back.

It needs `sudo sysctl kernel.perf_event_paranoid=-1` (IBS, raw events, kernel
symbols) and `kernel.nmi_watchdog=0` (the watchdog holds a PMC, so coverage
never reaches 100%); it warns on both. `[profile.bench]` already carries
line-table debuginfo, so symbolication needs no extra flags. `--profile-time N`
beats criterion's adaptive loop: a fixed window keeps sample counts comparable
across runs.

Read `benches/profiling.md` before interpreting a capture or hand-rolling a
`perf` command.

### Budget

Run the script before hand-rolling `perf record` — it already knows the
vendor, PMU prefix, call-graph mechanism, and pinning. Skipping it turned a
ten-minute pass into an hour:

- **`perf report -g graph,…` over a whole capture never finishes** (10 min, no
  output, 16 MB file). For "who calls X", filter `perf script` stacks to those
  containing X and tally the frame above it; keep `perf report` for flat
  self-time (`-g none`), where it is instant.
- **dwarf profiles perf itself.** `--call-graph dwarf,16384` at 3 kHz wrote
  577 MB in 12 s, and its own writeback read as ~25% kernel page-fault and FS
  time — indistinguishable from a real finding. Flat, the same run is 7 MB.
- **dwarf does not unwind this binary** — 146 of 76 000 stack lines resolved
  against the 137 MB fat-LTO build. Call graphs need `CALLGRAPH=lbr`, or
  `-C force-frame-pointers=yes` and a 2-min relink.
- **Startup is ~1 s** (font DB) — 8% of a 12 s window, with
  `fontdb::parse_face_info` near the top. `-D <ms>` skips it;
  `--profile-time 4` is enough for flat self-time.

Each `RUSTFLAGS` variant is a fresh fat-LTO relink (~2 min), and a four-arm
`--arms cpu` run is ~40 s at stock statistics. Cut `--sample-size` /
`--measurement-time` while iterating; spend the full run on the number you
report.
