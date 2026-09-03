# Review: text system, shaped-buffer cache, glyph atlas

Scope: `src/text/`, `src/renderer/backend/text/`, `src/renderer/backend/raster_atlas/`,
`src/renderer/backend/raster_pass.rs`, `src/renderer/backend/icon/mod.rs`,
`src/icons/icon_rasterizer/`, and the callers in `layout`, `ui/frame_cycle.rs` and the composer.

When you address an item, delete it from this file. Delete a heading when its
items are gone. Test structure was ignored. Rewrite tests to fit the better
production shape.

## 1. Mono leaves a sentinel behind

The mono metric stays: it backs `UiHarness::new` and so the whole in-crate UI
suite, where a hand-computable width is worth more than a measured one. It is
now a flag on `ShaperInner` rather than a variant every production method
matches, so the production stack no longer carries it. One consequence remains.

- [ ] `TextShapeKey::INVALID` means three things: empty text, an unusable face,
  and "a mono run". Only the third still needs the sentinel — the first two are
  screened before a key is minted (`TextShape::is_noop`,
  `TextShapeRequest::unbounded`), so `TextSystem::shapes_buffers`, the free fn
  `shaped(shapes_buffers, key, size)`, and the sentinel asserts in
  `shaped_run` / `supersede` / the encoder are all production weight carried for
  a test metric. Worth revisiting if a cheaper way to make the renderer drop a
  mono run appears.

## 2. Icons and text still draw as two batches

The two raster tenants now share a [`RasterProgram`] — one shader, one group-0
layout, one sampler, and so one pipeline pair per format. What they still do
not share is the **space**: separate textures, separate bind groups, separate
eviction budgets, deliberately. Merging the atlases was investigated and
rejected: sharing a texture means sharing the space it holds, and a glyph miss
evicting a tintable icon is not a fair trade, since an SVG re-raster costs
13-72 µs against roughly a microsecond for a glyph.

- [ ] Icons could join the text batch path: an icon has the same "no
  per-instance clip" property a glyph has, so the composer's strict-bounds
  scissor rule applies to it unchanged. That is what actually collapses the
  draw calls where a group mixes icons and text, and now that the pipeline is
  shared it needs no shared atlas — only a shared batch table. The win is
  larger than the draw call: `admit_higher_kind` closes the open text batch
  for every icon, so a toolbar of labelled buttons splits its text into one
  batch per icon today. What the merge has to carry is the order the split
  currently buys for free — one batch draws text then icons, so a run
  recorded *after* an icon it overlaps would paint under it. That needs an
  intra-batch overlap test, which is `HigherKindRects` scoped to the batch
  rather than the group.
- [ ] Until then, consecutive text and icon steps rebind the same pipeline.
  Both arms of the render loop reset `bound = Bound::None` because
  `RasterAtlas::draw_span` sets its own state, so a text step followed by an
  icon step issues a redundant `set_pipeline`. The bind group genuinely
  differs (different atlas); only the pipeline is shared. Worth a
  `Bound::Raster` variant if batch counts ever climb — dozens a frame today.

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
