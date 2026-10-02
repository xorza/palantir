# Open issues

- `src/layout/cache/bench.rs` `build_broad_variant`: changing one leaf's height from 1 px to 2 px leaves zero measure-cache hits in the whole tree on the next frame, where its 21 unchanged sibling subtrees each have an unchanged subtree hash.
- Glyph ink outside a text run's advance box — italic overhang, a negative left side bearing — lies outside both the run's damage rect (`src/scene/cascade/paint_rect.rs` `inflate_text_damage`) and its text batch scissor (`src/renderer/frontend/composer/session.rs` `text`); neither reads glyph ink bounds. A run alone is cut at its box plus the scale-step pad, and the same run batched with a wider neighbour shows its overhang.
- `RUSTDOCFLAGS=-D warnings cargo doc --no-deps --features internals` fails with `rustdoc::redundant-explicit-links` ("redundant explicit link target"); rustdoc prints no location. The CI doc command, without the feature, passes.
- `cargo doc --document-private-items` reports unresolved intra-doc links in `src/gpu/mod.rs`'s module doc (`WgpuBackend::submit`, `RasterPass`, `RasterProgram`); rustdoc prints no location for them.
