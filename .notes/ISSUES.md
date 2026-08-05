# Open issues

- `scripts/bench-perf.sh` defaults `FEATURES=internals`, but every bench
  target carries `required-features = ["bench"]`. Cargo silently skips a
  target whose required features are unmet, so the documented default
  invocation builds nothing and the script then fails at the
  binary-lookup step. `benches/AGENTS.md` documents the same default and
  shows `FEATURES=internals BENCH=caches` as a worked example.
