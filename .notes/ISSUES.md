# Open issues

- `atomic/text_atlas/gpu/cache_churn/*` and `mixed_stable_churn/frame` run 16 to 85 % slower on the
  atlas pool than on its base (`1851f60e2`), on every bench host: the pool evicts 15.6 glyphs a
  churn frame where the base evicted 9.8, at the same 4 MiB mask budget. `stable_keys/prepare` and
  `steady_warm/prepare`, which rasterize nothing, run 8 to 15 % slower.
- `MERGE_GAP_ROWS` (48) folds clean rows a flush now bakes again from their ramps: a folded row
  costs 0.7 to 12 µs of bake against a copy's 4.6 to 45 µs on the bench hosts, and its doc comment
  still prices a gap row as a staged copy alone.
