# Bench harness: own `main`, clap CLI, driver registry

Replace `criterion_main!` with a hand-rolled main so driver selection is
**exact and happens before a driver runs**, and collapse the bench
targets from five to two.

**Delete an item when it's done.** This file lists open work only.

## Why

Criterion's filter gates the `bench_function` call, not the setup a
driver does before it — and that setup cannot move inside the closure,
which criterion invokes once per sample (`criterion-0.8.2`,
`src/routine.rs`: a doubling `loop` in `warm_up`, then once per sample in
`bench`). So every driver sharing a binary pays every other driver's
setup, and a `perf` profile of one contains all of it.

Splitting the four GPU drivers into their own target took that from 11 s
to 410 ms and removed two adapter requests from every CPU profile. The
remaining 410 ms, and the coarseness of "split by target", is what this
plan addresses: with a registry we gate by name, exactly, and the split
stops being needed at all.

## What the investigation settled

**The macros are trivial and public.** `criterion_main!` expands to
"call each group fn, then `Criterion::default().configure_from_args()
.final_summary()`"; `criterion_group!` to "build a `Criterion` from the
config, call each target with `&mut` it". Nothing private is involved.

**Every knob has a public setter** — `sample_size`, `measurement_time`,
`warm_up_time`, `nresamples`, `noise_threshold`, `confidence_level`,
`significance_level`, `save_baseline`, `retain_baseline`,
`with_benchmark_filter`, `with_output_color`, `output_directory`,
`plotting_backend`, and `profile_time(Some(d))`, which is what selects
profile mode for the perf scripts.

**Two hard constraints:**

1. `configure_from_args()` builds its own clap `Command` and calls
   `.get_matches()`, which **hard-exits on unknown args**. Our flags
   cannot coexist with a call to it.
2. **`Mode` is `pub(crate)`.** Only `Mode::Profile` is reachable.
   `Mode::Test` and `Mode::List` are not — and `Mode::Test` is what makes
   `cargo test --benches` / `--all-targets` run each benchmark once
   instead of measuring for real. Losing it turns a routine
   `cargo test --all-targets` into an hours-long bench run.

   **Verified, not hypothetical:** `cargo test -p palantir --benches
   --features bench,showcase` runs every bench binary and completes in
   ~26 s. Test mode is load-bearing here.

   (Use that feature set, not `--all-features` — the latter enables
   `profile-with-tracy`, whose sampling threads turned the same 26 s of
   work into 36 min of CPU across 4.8 cores. See the crate's
   `AGENTS.md`.)

## Design

**Delegate-or-own.** If argv contains `--test` or `--list`, hand
everything to `configure_from_args()` and run every driver — today's
behaviour exactly, cargo and tooling untouched. Otherwise parse with our
own clap and drive the public setters. Driver selection doesn't apply in
test mode, which is right: that mode wants everything exercised once.

**A driver registry** — in place, see `src/bench.rs`. Gating is an exact
match against the table, before the driver is called: no regex guessing,
no fail-open heuristic.

`needs_gpu: bool` and the frame bench's `PALANTIR_BENCH_MODE` turned out
to be the same axis at two levels — driver capability vs run request —
so both are now `Arms { Cpu, Gpu, Both }`, matched by `overlap`. That
deleted `BenchMode`, `bench_mode()`, the skip notice, the planned
`--skip-gpu`, and the whole `gpu` bench target (5 targets → 4): asking
for CPU now runs no GPU driver at all, verified.

`opt_in` stays a separate field on purpose — it says whether a driver
belongs in the default set (cost, side effects), not what hardware it
uses. Only `frame` sets it.

**CLI:**

```
[FILTER]                    criterion regex, applied within selected drivers
-d, --driver <NAME>...      exact driver names, repeatable
    --list-drivers          print names and exit
    --skip-gpu              drop drivers needing a device
    --mode <cpu|gpu|both>   enables the frame bench (was PALANTIR_BENCH_MODE)
    --note <TEXT>           results-row context (was PALANTIR_BENCH_NOTE)
    --profile-time/--sample-size/--measurement-time/--warm-up-time
    --save-baseline/--baseline/--noplot
```

**End state — 5 targets → 2:**

| target | holds |
|---|---|
| `criterion` | all 18 criterion drivers, exact `--driver` gating (folds `gpu` back in) |
| `alloc` | the 3 dhat drivers behind a selector — `#[global_allocator]` is per-binary, but one binary can choose which workload runs |

Also deletes 114 lines of wrapper files and three `[[bench]]` blocks.

## Steps

- [ ] 2. `benches/criterion.rs` → clap CLI + delegate-or-own (~120
      lines). The registry loop is already in place; this adds the arg
      parsing and the `--driver` / `--skip-gpu` / `--mode` gating on top,
      keeping the `configure_from_args()` path for `--test` / `--list`.
- [ ] 3. Fold `gpu` in; delete `benches/gpu.rs`.
- [ ] 4. `benches/alloc.rs` consolidating the three dhat targets.
- [ ] 5. `frame` mode/note → flags, env kept as fallback so
      `scripts/bench-perf.sh` keeps working.
- [ ] 6. `Cargo.toml`: 5 `[[bench]]` → 2.
- [ ] 7. Update `scripts/bench-perf.sh` (`--driver` for selection rather
      than `FILTER`), `benches/AGENTS.md`, workspace `AGENTS.md`.
- [ ] 8. Verify: fmt/clippy/test, **`cargo test --all-targets`** for the
      test-mode delegation, a real filtered run, a perf run.

## Risks

- **`clap` becomes a direct manifest entry.** Already in the graph at
  4.6.5 via criterion, so `clap = { version = "4", optional = true }`
  under `bench` unifies with no extra build cost. Named because it is a
  new dependency line.
- **Test-mode delegation is the one thing that can silently break
  `cargo test`.** Establish whether `cargo test --all-targets` passes
  *before* the change, so we know if we're preserving something that
  works or something already broken.
- `cargo-criterion` integration rides `Criterion::default()`'s
  `connection`, so the own-branch keeps it — but baselines through
  `cargo criterion` are untested here.
- The registry is hand-maintained: a driver function added without a
  table row is invisible. Needs a test pinning the row count, or at
  minimum both edits in one place.
