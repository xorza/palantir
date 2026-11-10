# Open issues

- `renderer::Quad`'s doc says 64 B per instance; `hot_struct_sizes` pins it at 100 B.
- `cargo bench-once` stops at `atomic/record_pass/gpu/text/per_group` (`src/gpu/bench.rs`): the
  workload expects 256 text batches and the frame records 1, as an unturned run's glyphs now clip
  to its bounds on the CPU and its batch no longer closes at each group. The cases after it in the
  run do not start.
