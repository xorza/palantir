# Open issues
- `cargo drive bench --test` stops at `atomic/curve_pipeline/gpu` on an adapter without in-pass timestamps (Apple M5, Metal): `curve_pipeline/bench.rs:148` asserts a curve batch timestamp, so every case after it never runs.
