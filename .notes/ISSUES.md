# Open issues

- `Cargo.toml` `[lints.clippy]` allows `assert_is_empty`, which clippy 0.1.98 (stable on the Pi) does not know: every clippy run prints `unknown lint: clippy::assert_is_empty`, and `unknown_lints` ignores `-D warnings`.
