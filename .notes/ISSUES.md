# Open issues

- `Cargo.toml` and `palantir-anim-derive/Cargo.toml` set `package.homepage` to the same URL as `package.repository`, and every cargo run warns `cargo::redundant_homepage`.
- `primitives::math::num::unit_to_u8` has no production caller; only its own tests and a test comment in `srgb_transfer/tests.rs` reach it.
