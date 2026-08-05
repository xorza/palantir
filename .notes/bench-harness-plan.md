# Bench harness: own `main`, clap CLI, driver registry

Replace `criterion_main!` with a hand-rolled main so driver selection is
**exact and happens before a driver runs**, and collapse the bench
targets from five to two. The runner is in place and the targets are
down to four; what remains is the dhat consolidation below.

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

**Delegate-or-own.** When argv is criterion's, hand it to
`configure_from_args()` and run every driver — cargo and tooling
untouched. Otherwise parse with our own clap and drive the public
setters. Driver selection doesn't apply in test mode, which is right:
that mode wants everything exercised once.

The line is drawn by a lenient three-flag `Gate` (`ignore_errors`,
because on the delegating path argv carries criterion flags the strict
`Cli` would hard-exit on), and **the rule is an absence**: criterion
treats anything without `--bench` as test mode, and cargo sends a
`harness = false` bench target *no arguments at all* under `cargo test`.
The first cut keyed on a `--test` that cargo never sends, which silently
turned every `cargo test --all-targets` into a full measurement run.

**A driver registry** — in place, see `src/bench/`. Gating is an exact
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

**What the runner decides, a driver is handed** — in a `Run { arms,
recording, note }`, never re-derived from argv. `recording` is false in
test mode and under `--profile-time`, both of which disable criterion's
analysis, so the frame bench skips the results row it would otherwise
fill with "estimates not found". `note` rides the same channel: no
env var, no `set_var` into our own process.

Two readings of one argv were two things that could disagree, and did.

## Steps

- [ ] 4. `benches/alloc.rs` consolidating the three dhat targets, and
      `Cargo.toml` 4 `[[bench]]` → 2.

      **Open decision, not just open work.** The code now argues the
      other way — `src/bench/mod.rs` and `benches/AGENTS.md` both say
      each dhat driver keeps its own target because `#[global_allocator]`
      is per-binary. That was this plan's premise too ("one binary can
      choose which workload runs"), so one of the two is stale. Settle
      which before touching it.

## Risks

- `cargo-criterion` integration rides `Criterion::default()`'s
  `connection`, so the own-branch keeps it — but baselines through
  `cargo criterion` are untested here.
- The registry is hand-maintained: a driver function added without a
  table row is invisible. The tests pin the table's shape but cannot see
  a function that was never added.
