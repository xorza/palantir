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

Tried twice and reverted both times, on a measurement I do not trust —
re-run it on a quiet machine before deciding.

- [ ] `fitting_prefix` takes `glyph: impl Fn(usize) -> ClusterGlyph`, and
  `shape_truncated` calls `CacheEntry::probe(&self.cache, probe_key)` once per
  back-off round, both to keep the cache borrow disjoint from `logical_order`.
  Snapshot the probe's first layout run into a retained `Vec<ClusterGlyph>` once
  per miss; `fitting_prefix` then takes `&[ClusterGlyph]`, `CacheEntry::probe`
  goes, and a retry costs no hash. The copy is 24 bytes per glyph of one line.

  Two shapes were written: the snapshot plus the existing `order` index
  vector, and the snapshot sorted in place so `order` goes too. Against a
  `--save-baseline` pair they measured +1.0% and +2.2% on
  `text_shape/resize_drag_frame` — but a third run of the *reverted* code
  against the same baseline read −4.9%, so the machine had drifted about
  5% over the session and none of the three is a measurement of the
  change. What the attempt did establish is that the predicted win is not
  where the item says: the back-off runs a second round only when a
  reshaped prefix overruns, so the per-retry hash is almost never paid,
  and the copy is spent to save it. Measure the pair back to back, on a
  machine doing nothing else.
