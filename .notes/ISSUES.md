# Open issues

- `src/primitives/corners/mod.rs:16-17` and `src/primitives/spacing/mod.rs:11-12` state "~0.25 px error at 4096" for f16 storage. 4096 is exact in f16; the step is 2 px in [2048, 4096) and 4 px in [4096, 8192), so the error is up to ±1 px below 4096 and up to ±2 px above it.
- `src/common/expiry_wheel/mod.rs:113` says the shaped-buffer cache ring has 128 buckets. `with_keep(120 + 15)` gives 256.
