# Glyphon vendor fork: rebuild plan

**Status: Phase 1 landed. Phase 2 attempted and reverted (regressed
+17–23%, see §6).** Phase 3 (steps 7–9) remains as open decisions.

Audit of `vendor/glyphon/` (~2800 LOC across `cache.rs`, `text_atlas.rs`,
`text_render.rs`, `viewport.rs`, `custom_glyph.rs`, `lib.rs`) against
Palantir's actual usage. Findings ordered roughly by
`(LOC saved × certainty) / risk`.

Wrapper today: `src/renderer/backend/text.rs` + `src/text/cosmic.rs` +
`src/text/mod.rs`. Two `GlyphonRenderer` instances (plain + stencil)
sharing one `TextAtlas`, driven via the fork's `prepare_append` /
`render_range` / `clear_frame` extension so text interleaves with
quads in paint order.

## Big wins — safe, mechanical

### 1. Delete `custom_glyph` entirely (~−200 LOC)

Palantir always passes `custom_glyphs: &[]` and `|_| None` for the
rasterizer closure. The fork carries:

- `custom_glyph.rs` (183 LOC: `CustomGlyph`, `CustomGlyphId`,
  `RasterizeCustomGlyphRequest`, `RasterizedCustomGlyph`,
  `ContentType` discriminant pieces)
- `enum GlyphonCacheKey { Text, Custom }` — collapse to bare
  `cosmic_text::CacheKey`
- The `for glyph in text_area.custom_glyphs.iter()` block in
  `prepare_append` (`text_render.rs:192-272`)
- A second arm in `prepare_glyph`'s `get_glyph_image` closure
- A custom-glyph replay branch in `replay_cache_into_pending`
  (`text_atlas.rs:319-336`)

Removes one closure parameter from `prepare_append` and the
corresponding plumbing through `prepare_glyph` + `allocate_or_grow` +
`grow`. Eliminates the `R: FnMut(...)` generic threading.

### 2. Drop the `metadata` / depth pipeline (~−30 LOC, −4 bytes/vertex)

We always pass `|_| 0.0` for `metadata_to_depth`. The fork stores
`metadata: usize` on every glyph through prepare and packs
`depth: f32` into the vertex (`GlyphToRender.depth` at offset 24).
The shader emits it but nothing meaningful consumes it in our setup
(no depth test on text). Dropping shrinks `GlyphToRender` 28 → 24
bytes and removes another closure parameter from `prepare_append`.

### 3. Drop `ColorMode::Web` + the per-vertex sRGB flag (~−15 LOC, −2 bytes/vertex)

Palantir is always `Accurate`. The fork packs `srgb_flag: u16` into
`content_type_with_srgb[1]` on every single vertex even though it's
constant for the whole prepare. Either hardcode in the shader (the
`srgb_to_linear` step becomes unconditional) or push-constant it.
Combined with #2, vertex goes 28 → 22 bytes (~21% smaller — real
text-bandwidth win at scale).

### 4. Collapse `Cache` into `TextRenderer::new` (~−50 LOC)

`Cache` (`cache.rs`, 258 LOC) exists to share sampler / shader /
layouts / pipelines across many `TextRenderer` instances. We have
two. The pipeline cache is a `Mutex<Vec<PipelineCacheEntry>>` with
linear search by `(format, multisample, depth_stencil)` — for two
pipeline variants this is overkill. Build both pipelines eagerly in
our wrapper, share the rest via plain fields. Drop the `Arc<Inner>`
and `Mutex`.

### 5. One `TextRenderer`, two pipelines, one vertex buffer (~−30 LOC in wrapper, halves text vertex memory)

Currently `ModeState { renderer: GlyphonRenderer, ranges: Vec<...> }`
× 2 keeps two completely separate accumulators. The vertex data is
identical between plain and stencil — only the pipeline differs.
Rebuild: one vertex buffer, one `Vec<GlyphToRender>`, two
`RenderPipeline`s; `ranges: Vec<Option<(StencilMode, Range<u32>)>>`
so `render_batch` picks the right pipeline before draw. This is the
biggest structural win — saves the lazy-build-stencil dance and the
`if mode == ... { &mut self.plain } else { ... unwrap() }`
borrow-juggling at `text.rs:215`.

## Measurement-driven

### 6. ~~Replace `flush_pending_uploads` with `queue.write_texture`~~ — TRIED, REVERTED

**Status:** prototyped in commit `b83e6bb` on the vendor; reverted
because the bench regressed +17–23%.

Bench: `cargo bench --package glyphon --bench prepare_zoom` against
saved baseline `pre-phase2`.

| Scenario                            | Before  | After (write_texture) | Δ      |
|-------------------------------------|---------|-----------------------|--------|
| `zoom_burst_20_sizes_one_prepare`   | 45.5 ms | 53.4 ms               | **+17%** |
| `zoom_sustained_20_frames`          | 31.5 ms | 38.8 ms               | **+23%** |

The hypothesis ("modern wgpu's `queue.write_texture` internally
batches small writes") didn't hold. The staging-buffer +
`copy_buffer_to_texture` path packs N glyphs into one contiguous
buffer write + one command encoder + one `queue.submit`. Direct
`queue.write_texture` per glyph pays per-call overhead (internal
staging-slot allocation + per-call validation) that adds up over
dozens of misses per frame.

**Conclusion: keep the deferred staging-buffer flush.** Don't retry
unless wgpu publishes documented improvements to small-write
batching, or unless a profile shows the staging path's row-pitch
packing is the new bottleneck.

### 6b. Future Phase 2 candidates (not tried)

Biggest LOC win and the riskiest. The current path
(`text_atlas.rs:226-267` + `plan_upload_regions` +
`pack_uploads_into_scratch` + `ensure_staging_buffer` +
`submit_copies_to_texture`, ~250 LOC) does:

1. Accumulate `Vec<PendingUpload>` (each owns `Vec<u8>` from swash)
2. Plan row-aligned `Region`s with `COPY_BYTES_PER_ROW_ALIGNMENT`
   padding
3. Pack into one big `packed_scratch: Vec<u8>`
4. Create a *separate* `CommandEncoder` and submit it independently
   (extra `queue.submit`)
5. N `copy_buffer_to_texture` ops

The comment justifies it as "instead of N `queue.write_texture`
calls", but modern wgpu's staged write path already batches these
and avoids the extra submit. For UI text, cache misses per frame are
typically 0 (steady state) or a few dozen (first frame). Direct
`queue.write_texture` per pending glyph is likely faster *and*
simpler. Benchmark before/after with the existing
`docs/cache-history/` rigor before committing.

### 7. Drop `clip_to_bounds` per-glyph CPU clipping (~−50 LOC)

Palantir's composer already emits scissor rects. We're double-
clipping. Passing `i32::MIN..MAX` bounds and letting the scissor
handle it removes the per-glyph atlas-uv shift math and
`ClippedGlyph` shuffling. The only loss is rejecting fully-off-
screen glyphs before they hit the vertex buffer — for typical UI
almost everything is on-screen, so the saving is small. Likely a
wash on perf, big simplification of `text_render.rs`.

## Semantic shifts

### 8. Drop the color atlas if we don't ship emoji

Each glyph lives in exactly one of two atlases (mask R8 vs color
Rgba8Srgb). Bundled fonts (Inter + JBMono) emit zero color glyphs.
The split costs: two textures, two `HashMap`s, two packers, two
upload pipelines, two staging buffers, two `inner_for_content_mut`
branches everywhere. Decide: do we want emoji ever? If "not for the
foreseeable future", delete the color path entirely and the codebase
shrinks substantially.

### 9. Move the `cosmic_text` re-exports to a separate seam

`vendor/glyphon/src/lib.rs:27-33` blanket-re-exports
`cosmic_text::*`. Palantir then imports
`glyphon::cosmic_text::{Attrs, Buffer, ...}`. This makes the
dependency stack ambiguous — code reads as if glyphon owns these
types. If we're going to deeply integrate, depend on `cosmic-text`
directly in `palantir` and stop pretending glyphon is the gateway.

## Not worth touching

- TriangleStrip + instance-id quad expansion (`text_render.rs:373`) —
  standard, optimal.
- Frame-counter eviction (`text_atlas.rs:272`) — already clean
  post-refactor.
- Pow2 vertex buffer growth — fine.
- `BucketedAtlasAllocator` — etagere is the right call.

## Suggested order

**Phase 1 (no perf risk, ~300 LOC out, smaller vertex, simpler wrapper):**
5 → 1 → 2 → 3 → 4.

**Phase 2 (benchmark first):** 6.

**Phase 3 (decisions, not refactors):** 7, 8, 9 — flag and decide
separately.

## Numbers

| Change | LOC | Vertex bytes | Notes |
|--------|-----|--------------|-------|
| Today | 2806 | 28 | baseline |
| After Phase 1 | ~2100 (−25%) | 22 (−21%) | safe |
| After Phase 2 | ~1850 | 22 | + one fewer submit/frame |
| After Phase 3 (max) | ~1400 | 22 | drops color atlas + double-clip |
