# Review: text system, shaped-buffer cache, glyph atlas

Scope: `src/text/`, `src/renderer/backend/text/`, `src/renderer/backend/raster_atlas/`,
`src/renderer/backend/raster_pass.rs`, `src/renderer/backend/icon/mod.rs`,
`src/icons/icon_rasterizer/`, and the callers in `layout`, `ui/frame_cycle.rs` and the composer.

When you address an item, delete it from this file. Delete a heading when its
items are gone. Test structure was ignored. Rewrite tests to fit the better
production shape.

## 1. Mono leaves a sentinel and a duplicate metric behind

The mono metric stays: it backs `UiHarness::new` and so the whole in-crate UI
suite, where a hand-computable width is worth more than a measured one. It is
now a flag on `ShaperInner` rather than a variant every production method
matches, so the production stack no longer carries it. Two consequences remain.

- [ ] `TextShapeKey::INVALID` means three things: empty text, an unusable face,
  and "a mono run". Only the third still needs the sentinel — the first two are
  screened before a key is minted (`TextShape::is_noop`,
  `TextShapeRequest::unbounded`), so `TextSystem::shapes_buffers`, the free fn
  `shaped(shapes_buffers, key, size)`, and the sentinel asserts in
  `shaped_run` / `supersede` / the encoder are all production weight carried for
  a test metric. Worth revisiting if a cheaper way to make the renderer drop a
  mono run appears.
- [ ] `text/mono.rs` re-implements the `LineFit` arithmetic and a wrap-floor
  scan. That is a second spelling of policy the cosmic path owns.

## 2. Two raster atlases where one would do

Both tenants own a `RasterPass`: a shader module (the same WGSL, specialized
identically, compiled twice), a pipeline pair, a bind group, a vertex buffer,
a `Starvation` tracker and an atlas. `raster_pass.rs` gives three reasons for
the split. Two no longer hold and one is weaker than stated.

- [ ] Eviction already runs per **side** (`RasterAtlas::evict_one(target:
  ContentType)` only takes slots with `content == target`), so a colour icon can
  never evict a mask glyph. The one real contention is tintable (mask) icons
  against glyphs, which the 4 MiB eager-growth budget and 16 MiB ceiling make
  rare. Merge the two into one `RasterPass<RasterKey>` with
  `enum RasterKey { Glyph(GlyphRasterKey), Icon(IconRasterKey) }` (32 bytes).
  Initial sides: mask 1024², colour 512². `forget` becomes
  `!matches!(key, RasterKey::Icon(k) if sets.contains(&k.set))`. Delete
  `TextBackend.pass` / `IconBackend.pass`, one `flush`, one `end_frame`, and the
  `frame` hand-off from text to icon in `WgpuBackend::submit`.
- [ ] Both atlases already age on the shaper clock (`IconBackend::end_frame(frame)`
  takes the value `TextBackend::end_frame` returns). Two comments say otherwise:
  `UNALLOCATED_KEEP_FRAMES` ("the icon atlas counts its own submits",
  `raster_atlas/mod.rs:117-121`) and `RasterAtlas::current_frame` ("the icon
  backend's submit count", `:169-171`). Stale either way; gone after the merge.
- [ ] The merge does not by itself merge the **draws**: `text_batches` (strict-
  bounds scissor coalescing in the composer) and `PaintTier::Icon` group batches
  stay separate `RenderStep`s. An icon has the same "no per-instance clip"
  property a glyph has, so the strict-bounds rule applies unchanged and icons
  could join the text batch path in a second step. Until then the shared pass
  at least lets consecutive text/icon steps keep one `Bound` state instead of
  resetting to `Bound::None` after each.
- [ ] `IconBackend::prepare_batch` and `TextEncoder::encode_run` both assemble a
  `RasterQuad` by hand from a `SlotPlacement` (`RasterQuad::dim`, `pack_uv`,
  `pos ± bearing`). One `SlotPlacement::quad(pos, color) -> RasterQuad` (icon
  bearing is zero) removes the duplicate and is where a merged pass would build
  every quad.

## 3. Two rasterizers with two output shapes, one of them allocating per glyph

- [ ] `CosmicMeasure::rasterize_glyph` goes through `SwashCache::get_image_uncached`,
  which calls swash `Render::render` and allocates a fresh `Image { data: Vec<u8> }`
  per glyph. A zoom gesture re-rasterizes every visible glyph per scale rung, so
  this is per-glyph allocation on a frequent path. swash offers
  `Render::render_into(&mut Image)`, which reuses the image's buffer. Cosmic's
  `swash_image` is ~40 lines; replicate it over a retained
  `swash::scale::ScaleContext` plus one retained `Image`. That also drops
  `SwashCache` (an unused map, and the reason `CosmicMeasure` has a manual
  `Debug`).
- [ ] Give both rasterizers one shape. `IconRasterizer::rasterize(table, key,
  out: &mut Vec<u8>) -> Option<ContentType>` fills a retained buffer;
  `rasterize_glyph(key) -> Option<GlyphImage>` returns an owned one. Make the
  glyph side `rasterize(key, out: &mut Vec<u8>) -> Option<RasterFacts>` where
  `RasterFacts { content, size: UVec2, bearing: IVec2 }` is exactly what
  `RasterImage` wants, and both encoders build `RasterImage` the same way. The
  public `TextGlyphs::rasterize` takes the out-param too, or returns a borrowed
  `GlyphImage<'_>` over the lease's scratch.
- [ ] `GlyphPlacement { left, top, width, height }` is rebuilt by the encoder into
  `UVec2`/`IVec2` and then narrowed by `PackedMetadata`. Spell the public type as
  `size: UVec2, bearing: IVec2` and one conversion goes.

## 4. Retention state is scattered over `CosmicMeasure`

`cache`, `expiry`, `recycle_pool`, `frame`, `counters`, and the four protocol
operations (`insert`, `hit_entry`, `supersede`, `tick_frame`) plus `shaped_run`,
`cache_hit`, `drop_all_buffers` are one thing spread across a twelve-field
struct. That is why `hit_entry` and `CacheEntry::probe` are associated fns that
take fields, and why `tick_frame` and `drop_all_buffers` destructure by hand.

- [ ] Extract a `ShapedBufferCache` (or make `cache_entry.rs` the owner file of a
  `ShapedCache`) holding those five fields with those methods. `CosmicMeasure`
  keeps `font_system`, `resolved`, the swash context, the ellipsis memo and the
  three scratch buffers, and calls `self.cache.hit(key)` while it holds
  `self.break_scratch`. `cache_entry.rs`'s module doc argues the four ops belong
  together "because they are one protocol"; put them on one type.
- [ ] `PROTECTED_KEEP_FRAMES` and `PROTECTED_SPREAD_MASK` (`cosmic/mod.rs:128,138`)
  are pure aliases of `text::RENDERED_RUN_KEEP_FRAMES` /
  `RENDERED_RUN_KEEP_SPREAD_MASK`, each with a paragraph explaining the alias.
  Delete them and use the `text::` names.
- [ ] `CosmicMeasure::insert` recycles a displaced buffer for a case its own
  comment calls unreachable. A logic error should crash:
  `debug_assert!(displaced.is_none())`.
- [ ] `CosmicMeasure: Default` has no production caller (every use is a test).
  Move it under `test_support` or delete it in favour of `new(FontScope::Bundled)`.

## 5. The frame clock advances once per window, not once per frame

- [ ] The clock is app-global but the tick is per window: two windows advance it
  twice per host frame, halving every retention window (shaped buffers, encoded
  rows, atlas slots) for both. `CosmicMeasure::frame`'s doc calls the jump "fine
  for an age comparison". Either tick once per host frame or state the halving
  as a known limit beside the constants it affects.

## 6. `TextSystem::measure` demotes on one arm only

- [ ] The `WrapCommit::Unbounded` arm returns without touching `entry.wrap`. A
  truncating run whose width grows until the text fits keeps its last bounded
  buffer referenced by the row and never superseded, so that buffer ages on the
  120-frame protected window instead of the 4-frame probation. `take()` the slot
  and supersede it on that arm as the `Bound` arm does.
- [ ] Rows for a live widget never shrink: a widget whose text-ordinal count drops
  from 100 to 3 keeps 97 stale rows until the widget leaves the tree. Bounded, so
  low priority; note it or sweep rows whose ordinal is past the widget's count
  this frame.

## 7. Truncation reads the probe through a closure and re-probes per retry

- [ ] `fitting_prefix` takes `glyph: impl Fn(usize) -> ClusterGlyph`, and
  `shape_truncated` calls `CacheEntry::probe(&self.cache, probe_key)` once per
  back-off round, both to keep the cache borrow disjoint from `logical_order`.
  Snapshot the probe's first layout run into a retained `Vec<ClusterGlyph>` once
  per miss; `fitting_prefix` then takes `&[ClusterGlyph]`, `CacheEntry::probe`
  goes, and a retry costs no hash. The copy is 24 bytes per glyph of one line.
  Measure on `text_shape/resize_drag_frame`.

## 8. Staging pads every glyph row to 256 bytes

- [ ] `RasterAtlas::enqueue_upload` pads each row to `COPY_BYTES_PER_ROW_ALIGNMENT`.
  A 12 px mask glyph stages 256 bytes per 12-byte row, about 20× its pixels, and
  every byte is memcpy'd on the CPU and written through the belt. A frame that
  rasterizes a few hundred glyphs (one zoom rung) stages 1–2 MB for ~100 KB of
  pixels. `queue.write_texture` has no row-alignment requirement and is safe on
  every frame with no pending grow, which is nearly all of them; keep the encoder
  path for grow frames where ordering against the blit matters. Measure on
  `text_atlas/cache_churn`.

## 9. Visibility and placement

- [ ] `TextShapeKey`'s five quantized fields (`size_q`, `max_w_q`, `lh_q`,
  `family_q`, `face_q`) are `pub(crate)` but nothing outside `key.rs` reads them;
  every reader goes through the accessors. Only `text_hash` is read elsewhere.
  Make the five private.
- [ ] `ContentType` lives under `renderer::backend::raster_atlas` and is imported
  by `text::render`, `text::cosmic` and `icons::icon_rasterizer`, and re-exported
  from `lib.rs`. `text/mod.rs` states that `renderer` depends on `text` and not
  the reverse; this import inverts it. It is rasterizer-output vocabulary. Move
  it to `primitives` (or a small `raster` vocabulary module) that text, icons and
  the atlas all sit above.
- [ ] `font_scope.rs` calls `crate::text::cosmic::warm_matches(...)` by full
  inline path. `use crate::text::cosmic;` then `cosmic::warm_matches(...)`.
- [ ] `glyphs/mod.rs`: the free fns `request` and `placement` are one-liners whose
  doc comments are longer than their bodies. Inline at the two call sites.
- [ ] `EllipsisMemo::wanted(face).measured(advance)` is a two-step builder for a
  two-field struct. Write `EllipsisMemo { face, advance }`.
- [ ] `HAlign::Stretch` never reaches a key: `WrapBound::new` projects it to `Auto`
  and unbounded keys store `Auto`. `FaceBits::halign`'s `Stretch` decode arm,
  the `Stretch` half of the `const _` assertion, and `cosmic_align`'s `Stretch`
  arm are unreachable. Encode the four reachable values in two bits.
- [ ] `TextEncoder::try_emit_cached` forwards to `EncodedCache::emit_cached` in
  one line. Call the cache directly or fold the two.
- [ ] `RasterQuad::shader_module` compiles one WGSL twice, once per pass. Share
  the module (moot after item 3).
- [ ] `cosmic/mod.rs` has two `use` groups separated by the `mod` declarations.
  One group.

## 10. Comments restate other comments, history, and the code

- [ ] The retention ordering is told in full five times: `RENDERED_RUN_KEEP_FRAMES`
  (`text/mod.rs`, 40 lines), `PROTECTED_KEEP_FRAMES`, `ENCODED_CACHE_KEEP_FRAMES`,
  `UNALLOCATED_KEEP_FRAMES`, and the module docs of `cache_entry.rs` and
  `expiry_wheel`. State it once on `RENDERED_RUN_KEEP_FRAMES`; the others link.
- [ ] `text/mod.rs`'s module doc explains the crate's module-naming philosophy
  ("Owner modules" / "Vocabulary modules"). That is a crate convention and
  belongs in `CLAUDE.md` or `docs/`, not in one module's doc.
- [ ] Docs that narrate the history of a change rather than the reason for the
  code: `FaceBits` ("Four separate bytes is what the key used to hold"), `Starvation`
  ("Two bools carried these three plus a fourth combination"),
  `TextSystem::end_frame` ("Under the `hot` sweep a run therefore lost…"),
  `CosmicMeasure::ellipsis` ("One slot was not enough"), `EncodedCache::expiry`
  ("the previous `map.retain` walked…"). Keep the measurement and the rule; drop
  the narrative.
- [ ] `TextShapeKey` doc: "Three quantized fields rather than one collapsed `u64`
  so the renderer can also reuse the size/width components if it wants to (e.g.
  group runs by size for atlas bin reuse)". Nothing does. The actual reason is
  that the restore path (`ensure_buffer`) rebuilds `Metrics` and `Attrs` from
  the key alone, so the key must be lossless. Say that.
- [ ] `encoded_counters.rs` module doc: "These were added to size a probation tier
  … which the measurement then argued against building for now". Either the
  counters answer a live question or they go; a doc that says they exist for an
  abandoned experiment is a delete note.
