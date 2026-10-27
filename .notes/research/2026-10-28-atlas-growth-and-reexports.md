# Research for the shelved atlas growth and the re-export rule (2026-10-28)

## How glyph and image atlases grow

- Skia (Ganesh `GrDrawOpAtlas`): when a page is full the atlas "will attempt
  to allocate a new atlas texture (or "page") of the same size, up to a
  maximum number of textures" (`kMaxMultitexturePages` = 4); new uploads go
  to the lowest page; each page is a grid of plots on an LRU list; eviction of
  flushed plots waits until every page exists; `compact()` moves data down and
  frees idle pages. Pages are bound together (multitexturing), so a draw does
  not split by page. Graphite caps a page at 2048² (half-float texture
  coordinates). <https://skia.googlesource.com/skia/+/refs/heads/chrome/m84/src/gpu/GrDrawOpAtlas.h>,
  <https://skia.googlesource.com/skia/+/HEAD/src/gpu/graphite/text/TextAtlasManager.h>
- WebRender: `SharedTextures` is "a set of lazily allocated, fixed size"
  textures per format, each an `AllocatorList<ShelfAllocator, …>`; the cache
  adds a texture, not a bigger one. It left texture arrays (bug 1673387).
  <https://doc.servo.org/webrender/texture_cache/struct.SharedTextures.html>,
  <https://bugzilla.mozilla.org/show_bug.cgi?id=1544050>
- Impeller (flutter/engine#52849): when the packer is full, doubles the
  height, allocates a new texture, blits the old one into its top-left, and
  packs the new rows with a packer of the added area only; past the maximum
  it rebuilds. Palantir's `Side::grow` is this design.
  <https://www.github.com/flutter/engine/pull/52849>
- glyphon 0.11: doubles to the device maximum and re-rasterizes every cached
  glyph into the new texture at its old position.
  <https://docs.rs/crate/glyphon/latest/source/src/text_atlas.rs>
- epaint (egui): a grown atlas is uploaded whole (`ImageDelta::full`); a
  partial delta must fit the existing texture.
  <https://docs.rs/crate/epaint/0.33.3/source/src/texture_atlas.rs>
- wgpu: a texture cannot be resized, and an array texture cannot gain layers,
  so growing either is a new texture and a copy. Binding arrays are a native
  feature (`TEXTURE_BINDING_ARRAY`), not WebGPU.
- In the code: `raster_atlas/side.rs` doubles a square side up to
  `max_bytes` (16 MiB a side); text starts at 1024² mask and 256² color, icons
  at 256² mask and 512² color, so a side grows at most four times in a
  process. The last mask grow allocates 16 MiB and copies 4 MiB; the atlas
  shader reads by `textureLoad`, so no sampler or derivative ties a page to
  uniform control flow.

## Measured grow frame (Apple M5, Metal, `cargo drive bench raster_atlas`)

Wall time from `Side::grow` to the drained submit of its blit, the old side
fully written first; `submit` is the same submit with nothing recorded, and
`text_atlas/gpu/steady_warm/frame` a whole warm text frame for scale.

| case | time |
|---|---|
| `submit` | 17.7 µs |
| `grow/mask/1024` (4 MiB new, 1 MiB copied) | 296 µs |
| `grow/color/1024` (16 MiB new, 4 MiB copied) | 625 µs |
| `grow/mask/2048` (16 MiB new, 4 MiB copied) | 914 µs |
| `text_atlas/gpu/steady_warm/frame` | 248 µs |

The M5 is an integrated GPU on unified memory. Not measured: lavapipe, where
the copy and the clear of the new texture run on the CPU.

## Re-exports

- See `2026-10-26-review-decisions.md`, "Foreign re-exports" (Effective Rust
  Item 24; API guidelines discussion #176; egui-wgpu and eframe practice).
- RFC 3516 (`public = true` in Cargo, `exported_private_dependencies`) is
  still unstable: the 2026 project goal is to stabilize a minimal subset of
  the lint. <https://goals.rust-lang.org/2026/pub-priv.html>
