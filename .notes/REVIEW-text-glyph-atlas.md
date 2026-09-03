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

## 2. Two raster atlases, and what sharing them would really cost

Both tenants own a `RasterPass`: a shader module (the same WGSL, specialized
identically, compiled twice), a pipeline pair, a bind group, a vertex buffer,
a `Starvation` tracker and an atlas.

**Investigated, and the verdict is narrower than "merge them".** Sharing a
texture means sharing the space it holds, so an atlas cannot offer both one
texture and per-tenant budgets — the current split chose isolation, and that
is defensible. Eviction runs per **side** today
(`RasterAtlas::evict_one(target: ContentType)` takes only slots with
`content == target`), so a colour icon already cannot evict a mask glyph and
merging would not change that. What merging *would* newly allow is a glyph
miss evicting a tintable icon, and the two are not interchangeable: an SVG
re-raster is 13-72 µs against roughly a microsecond for a glyph, so a
text-heavy zoom would thrash icon rasters at an order of magnitude more cost
than it thrashes glyphs. The headline win — one draw call where a group mixes
icons and text — does **not** follow from the merge either, and needs the
composer change the third item below describes.

So the items worth doing are the ones that share what is genuinely identical,
without sharing the space.

- [ ] `RasterQuad::shader_module` compiles one WGSL twice, once per pass, from
  the same three substituted constants. Build it once and hand both passes a
  reference. The bind group *layouts* are structurally identical too, so one
  layout would let one pipeline pair serve both — check that wgpu treats them
  as compatible before relying on it.
- [ ] `IconBackend::prepare_batch` and `TextEncoder::encode_run` both assemble a
  `RasterQuad` by hand from a `SlotPlacement` (`RasterQuad::dim`, `pack_uv`,
  `pos ± bearing`). One `SlotPlacement::quad(pos, color) -> RasterQuad` (icon
  bearing is zero) removes the duplicate.
- [ ] Icons could join the text batch path: an icon has the same "no
  per-instance clip" property a glyph has, so the composer's strict-bounds
  scissor rule applies to it unchanged. That is what actually collapses the
  draw calls, and it needs no shared atlas — only a shared batch table.
- [ ] Two comments claim the icon atlas keeps its own clock:
  `UNALLOCATED_KEEP_FRAMES` ("the icon atlas counts its own submits") and
  `RasterAtlas::current_frame` ("the icon backend's submit count"). Both are
  wrong — `IconBackend::end_frame(frame)` takes the value
  `TextBackend::end_frame` returns, which is the shaper's clock.

## 3. Two rasterizers with two output shapes, one of them allocating per glyph

- [ ] `CosmicMeasure::rasterize_glyph` goes through `SwashCache::get_image_uncached`,
  which calls swash `Render::render` and allocates a fresh `Image { data: Vec<u8> }`
  per glyph. A zoom gesture re-rasterizes every visible glyph per scale rung, so
  this is per-glyph allocation on a frequent path. swash offers
  `Render::render_into(&mut Image)`, which resizes the image's buffer in place
  and so reuses its capacity. Cosmic's `swash_image` is ~40 lines; replicate it
  over a retained `swash::scale::ScaleContext` plus one retained `Image`,
  clearing that image per glyph. That also drops `SwashCache` (an unused map,
  and the reason `CosmicMeasure` has a manual `Debug`).

  **Blocked on a dependency decision.** cosmic-text re-exports only
  `swash::scale::image::{Content, Image}` and `swash::zeno::{...}`, not
  `Render` / `Source` / `StrikeWith` / `ScaleContext`, so this needs `swash`
  as a direct entry in `Cargo.toml`. It compiles nothing new — swash is already
  in the graph under cosmic-text, at the version cosmic-text pins — but it is a
  manifest entry all the same. The second item below only pays off together
  with this one: filling an out-param while still calling
  `get_image_uncached` adds a copy and keeps the allocation.
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

## 4. The frame clock advances once per window, not once per frame

- [ ] The clock is app-global but the tick is per window: two windows advance it
  twice per host frame, halving every retention window (shaped buffers, encoded
  rows, atlas slots) for both. `CosmicMeasure::frame`'s doc calls the jump "fine
  for an age comparison". Either tick once per host frame or state the halving
  as a known limit beside the constants it affects.

## 5. `TextSystem::measure` demotes on one arm only

- [ ] The `WrapCommit::Unbounded` arm returns without touching `entry.wrap`. A
  truncating run whose width grows until the text fits keeps its last bounded
  buffer referenced by the row and never superseded, so that buffer ages on the
  120-frame protected window instead of the 4-frame probation. `take()` the slot
  and supersede it on that arm as the `Bound` arm does.
- [ ] Rows for a live widget never shrink: a widget whose text-ordinal count drops
  from 100 to 3 keeps 97 stale rows until the widget leaves the tree. Bounded, so
  low priority; note it or sweep rows whose ordinal is past the widget's count
  this frame.

## 6. Truncation reads the probe through a closure and re-probes per retry

- [ ] `fitting_prefix` takes `glyph: impl Fn(usize) -> ClusterGlyph`, and
  `shape_truncated` calls `CacheEntry::probe(&self.cache, probe_key)` once per
  back-off round, both to keep the cache borrow disjoint from `logical_order`.
  Snapshot the probe's first layout run into a retained `Vec<ClusterGlyph>` once
  per miss; `fitting_prefix` then takes `&[ClusterGlyph]`, `CacheEntry::probe`
  goes, and a retry costs no hash. The copy is 24 bytes per glyph of one line.
  Measure on `text_shape/resize_drag_frame`.

## 7. Staging pads every glyph row to 256 bytes

- [ ] `RasterAtlas::enqueue_upload` pads each row to `COPY_BYTES_PER_ROW_ALIGNMENT`.
  A 12 px mask glyph stages 256 bytes per 12-byte row, about 20× its pixels, and
  every byte is memcpy'd on the CPU and written through the belt. A frame that
  rasterizes a few hundred glyphs (one zoom rung) stages 1–2 MB for ~100 KB of
  pixels. `queue.write_texture` has no row-alignment requirement and is safe on
  every frame with no pending grow, which is nearly all of them; keep the encoder
  path for grow frames where ordering against the blit matters. Measure on
  `text_atlas/cache_churn`.

## 8. Visibility and placement

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

## 9. Comments restate other comments, history, and the code

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
