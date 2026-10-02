# Open issues

- `src/common/expiry_wheel/mod.rs:113` says the shaped-buffer cache ring has 128 buckets. `with_keep(120 + 15)` gives 256.
- `src/layout/cache/bench.rs` `build_broad_variant`: changing one leaf's height from 1 px to 2 px leaves zero measure-cache hits in the whole tree on the next frame, where its 21 unchanged sibling subtrees each have an unchanged subtree hash.
- Glyph ink outside a text run's advance box — italic overhang, a negative left side bearing — lies outside both the run's damage rect (`src/scene/cascade/paint_rect.rs` `inflate_text_damage`) and its text batch scissor (`src/renderer/frontend/composer/session.rs` `text`); neither reads glyph ink bounds. A run alone is cut at its box plus the scale-step pad, and the same run batched with a wider neighbour shows its overhang.
- `RUSTDOCFLAGS=-D warnings cargo doc --no-deps --all-features` fails with `rustdoc::redundant-explicit-links` ("redundant explicit link target"); rustdoc prints no location. The CI doc command, without `--all-features`, passes.
- `src/icons/icon_registry/mod.rs` `resident`: a released-but-undrained set still answers `Some`, and the epoch bump on release starts an icon prewarm walk that rasterizes the set about to be freed.
