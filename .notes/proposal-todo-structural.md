# Proposal: the three structural TODO items (2026-10-28)

Covers the last three items of `.notes/TODO.md`. Facts and numbers are in
`.notes/research/2026-10-28-atlas-growth-and-reexports.md` and below.

# Part 1: Decisions

## Layout state in one copy

- **Close the item; layout keeps its scratch columns and the capture copy.**
  On the frame fixture's `integration/frame/cpu/resizing` (Apple M5, macOS
  `sample`, 6 s), `memmove` and `memset` under `FrameCycle::post_record`, where
  `LayoutEngine::run` and `MeasureCache::capture_tree` inline, are 70 of 3763
  frame samples (1.9 %), and the capture copy is at most 44 of them (1.2 %).
  `resize_for`'s fills and the hit restore (29 samples under
  `LayoutPass::measure`) stay in either design. Refused: forest-wide columns
  written in place, which change every reader of `Layout` (cascade, encoder,
  hit test, scroll) and the `prev`/`cur` swap for about 1 % of a resizing
  frame.

## Atlas growth by pages

- **Pages only past the trigger, and the trigger is measured where it can
  fire.** The M5 grow frame is 0.9 ms at most. lavapipe runs the copy and the
  clear of the new texture on the CPU and is the adapter that can pass 2 ms.
  Refused: building pages on the M5 numbers alone.
- **If built: Skia's design.** Fixed-size pages per side (mask 2048² R8, color
  1024² RGBA8), up to four, so the 16 MiB budget holds; all four bound in the
  one bind group, with a shared 1×1 stand-in for a page not yet allocated; the
  page in two bits of the instance; `textureLoad` by page through a `switch`,
  which has no uniformity rule. Eviction begins only once every page exists.
  Refused: array textures (wgpu cannot add a layer, so a grow is a new texture
  and a copy again); binding arrays (`TEXTURE_BINDING_ARRAY` is native only);
  re-rasterizing on grow (glyphon). Source: Skia `GrDrawOpAtlas`, WebRender
  `SharedTextures`.

## Suites below `ui` that drive `Ui`

- **`text/probe/tests.rs` drives `TextShaper` directly.** Its three tests read
  `Ui::probe_text`, which is `TextShaper::layout` and nothing more, through a
  harness used only as an interner. `TextShaper::test_mono` and `layout` give
  the same probe with no `Ui`, so the suite stays in `text`.
- **`theme/tests/file_values.rs` moves to `widgets/tests/`.** What it asserts
  is that the bundled widgets render every number a theme file can carry and
  still load; the theme layer only parses. Its one-line `pretty` helper is
  inlined there.
- **`layout/cache/tests/` stays.** The cache's subject is reuse across frames
  of recorded trees, and `Ui` is the only recorder; every suite under
  `layout/tests/` drives it the same way.

# Part 2: Plan

Each batch leaves `cargo drive gate` green.

## Batch 1: Suites below `ui`

- [ ] `text/probe/tests.rs`: `TextShaper::test_mono()` and `layout` in place
  of `UiHarness::arena()` and `Ui::probe_text`; the first test's doc names
  `TextShaper::layout`.
- [ ] Move `theme/tests/file_values.rs` to `widgets/tests/theme_file_values.rs`
  with its module line; inline `pretty`; `theme/tests/mod.rs` drops the module
  and `pretty` if nothing else uses it.
- [ ] Delete the third TODO item.

## Batch 2: Close the layout item

- [ ] Delete the first TODO item; this proposal's decision folds into the
  `layout/cache` module doc as one `Refused:` line.

## Batch 3: Measure the atlas grow on lavapipe

- [ ] On the Linux rig with Mesa lavapipe, `cargo drive bench raster_atlas`;
  add the table to the research note.
- [ ] Under 2 ms: the TODO item stays with its trigger, and Batch 4 is
  dropped.

## Batch 4: Atlas pages (only if Batch 3 passes 2 ms)

- [ ] `Side` holds up to four page textures and packers of one fixed edge;
  `grow` adds a page; `PendingGrow` and the grow blit go.
- [ ] `AtlasSlot` and the instance carry the page; `uv_and_kind` gives the
  page two bits from `u` and `v`, which a 2048 edge frees.
- [ ] The sides' bind group holds four mask and four color views, stand-ins
  for absent pages, rebuilt when a page is added.
- [ ] `shader.wgsl` loads from the instance's page.
- [ ] `allocate` evicts only once every page of the side exists;
  `eager_growth_bytes` becomes a page count.
- [ ] Tests: placement across pages, no eviction before the last page, uvs
  stable across a page add. The bench's `grow` cases become `new_page`.
