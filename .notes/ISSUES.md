# Open issues
- `src/layout/engine.rs` container-text pass and `src/layout/pass.rs` measure both read a node's own `Visibility`, not the cascaded one, so every container under a `Hidden` ancestor still shapes its paint-only run.
- `src/hot_struct_sizes.rs` pins `FRAME_ENGINES_SIZE` at 1456 without the `bench` feature, but `size_of::<FrameEngines>()` is 1480 there, so `hot_struct_sizes_are_pinned` fails on a default-feature `cargo test -p palantir`.
- `cargo doc -p palantir --no-default-features` fails with 11 unresolved intra-doc links, all to `WinitHost`, `HostHandle` and their members, from `src/lib.rs`, `src/host/offscreen.rs` and `src/ui/mod.rs`.
