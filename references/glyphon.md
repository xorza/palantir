# glyphon — reference notes for Palantir

glyphon is the de-facto wgpu glyph renderer in the Rust ecosystem: it bridges `cosmic-text` (shaping + layout) to a `wgpu` middleware that rasterizes glyphs on demand into a growable atlas and emits one instanced quad per glyph. The whole crate is ~1.6k LOC across seven files (`tmp/glyphon/src/{lib,cache,viewport,text_atlas,text_render,custom_glyph,error}.rs`). It's exactly the candidate `DESIGN.md` lists for Palantir's text path; this file pins down what it does, what it doesn't, and where it would or wouldn't slot into our paint pass.

## 1. Atlas: two textures, etagere bin-packing, LRU eviction, doubling growth

`InnerAtlas` (`text_atlas.rs:19`) wraps one wgpu `Texture` plus a `BucketedAtlasAllocator` from etagere (the wgpu-team's shelf-packer crate, line 5). Two of them live inside `TextAtlas`: a **mask** atlas in `R8Unorm` for SDF/coverage glyphs and a **color** atlas in `Rgba8Unorm[Srgb]` for emoji and color-fonts (`text_atlas.rs:225-249`, `Kind::texture_format`). `ColorMode::{Accurate, Web}` (`text_atlas.rs:260-280`) toggles `Rgba8UnormSrgb` vs `Rgba8Unorm` on the color atlas — Accurate gives physically correct blending, Web matches browser behavior.

Initial size is 256² clamped to `device.limits().max_texture_dimension_2d` (`text_atlas.rs:31-35`). On a failed pack, `try_allocate` (`text_atlas.rs:72-105`) walks the LRU and evicts least-recently-used glyphs *that aren't in use this frame*; only when every cached glyph is in `glyphs_in_use` does it bail out and trigger `grow`. `glyphs_in_use` is a `HashSet` populated as `prepare_glyph` touches each cache entry (`text_render.rs:455, 467`). It's cleared at end of frame via `TextAtlas::trim` (called explicitly by the user, see `examples/hello-world.rs:255`).

`grow` (`text_atlas.rs:111-217`) doubles each axis (capped at `max_texture_dimension_2d`), creates a new texture, and **re-rasterizes every cached glyph** by calling `cache.get_image_uncached(font_system, cache_key)` (line 157). This is heavy — full reshape of the atlas — but it happens at most `log2(max_dim/256)` times per process. There is no defrag pass; eviction handles fragmentation by eventually freeing whole shelves.

Per-glyph `GlyphDetails` (`lib.rs:45`) stores `{ width, height, gpu_cache: GpuCacheStatus, atlas_id, top, left }` — atlas position plus the original swash placement offsets. `GpuCacheStatus::SkipRasterization` (`lib.rs:42`) is the sentinel for zero-sized glyphs (whitespace) — kept in cache so we don't re-ask cosmic-text every frame.

## 2. cosmic-text bridge: caller owns Buffer + FontSystem + SwashCache

glyphon takes `&Buffer`, `&mut FontSystem`, `&mut SwashCache` separately on every `prepare` call (`text_render.rs:50-71`). It re-exports them all from `lib.rs:25-31` so users don't add a direct cosmic-text dep. Ownership split:

- `FontSystem` — fontdb + shape cache, owned by the app, mutable on every shape.
- `Buffer` — paragraph: text + `Metrics(font_size, line_height)` + width/height constraints + already-shaped lines. App-owned, app re-shapes on text/size change via `set_text` / `set_size` / `shape_until_scroll` (see example `hello-world.rs:93-100`).
- `SwashCache` — rasterized glyph image cache (CPU-side, separate from the GPU atlas). `cache.get_image_uncached(font_system, cache_key)` (`text_render.rs:276`) does the actual rasterization on miss.

glyphon never calls into shaping itself — it only iterates `buffer.layout_runs()` (`text_render.rs:244`) and per-run `run.glyphs` (`text_render.rs:251`), pulling each `LayoutGlyph::physical((left, top), scale)` to get a `PhysicalGlyph { x, y, cache_key }` whose `cache_key: cosmic_text::CacheKey` is `(font_id, glyph_id, font_size, x_bin, y_bin)`. This is the cache key for both `SwashCache` and the GPU atlas.

The implication: **shaping is the user's responsibility** and is not reactive. If your text changes, you call `buffer.set_text(...); buffer.shape_until_scroll(...)`. If the layout width changes, `set_size`. There's no diffing — cosmic-text's own `Buffer` does line-level dirty tracking, but Palantir's per-frame tree rebuild loses that.

## 3. wgpu pipeline: instanced quads, per-glyph 28-byte vertex, two atlas bindings

Vertex format `GlyphToRender` (`lib.rs:54-63`): `pos: [i32;2]`, `dim: [u16;2]`, `uv: [u16;2]`, `color: u32`, `content_type_with_srgb: [u16;2]`, `depth: f32`. 28 bytes packed, attributes declared as a tight 6-attribute layout (`cache.rs:63-98`). `step_mode: VertexStepMode::Instance`, draw is `pass.draw(0..4, 0..N)` (`text_render.rs:352`) — one instance per glyph, four `vertex_idx`'s reconstruct corners in the shader (`shader.wgsl:52-57`). No index buffer.

Bind groups (two, layout in `cache.rs:100-144`):
- group 0: color atlas texture, mask atlas texture, sampler (Nearest/Nearest, mip 0 only — `cache.rs:48-56`).
- group 1: `Params { screen_resolution: vec2<u32>, _pad: vec2<u32> }` uniform from `Viewport`.

Pipeline: `TriangleStrip`, `BlendState::ALPHA_BLENDING`, no culling, depth-stencil optional (`cache.rs:240-247`). Format/multisample/depth combos are cached in `Cache::get_or_create_pipeline` (`cache.rs:199-255`) so multiple `TextRenderer`s sharing one `Cache` reuse pipelines.

Shader (`shader.wgsl`): vertex stage rebuilds the quad from `vertex_idx` bits 0/1, scales `pos` to clip space against `params.screen_resolution`, flips Y (line 70), branches on `content_type` (0=color, 1=mask) and `srgb` (0=passthrough, 1=`srgb_to_linear` per channel). Fragment samples the matching atlas; mask path is `vec4(color.rgb, color.a * mask.r)`; color path is straight texture sample.

Worth noting: **no SDF, no signed-distance shader, no MSAA on glyphs**. Coverage is whatever swash rendered into the bitmap. Subpixel positioning is via `cosmic_text::SubpixelBin` — `CacheKey` includes `x_bin`/`y_bin` (4 bins per axis), so the atlas can hold up to 16 distinct rasterizations of the same glyph at sub-pixel offsets. This buys positional precision without true subpixel-AA color fringing.

## 4. Frame lifecycle: `prepare` builds vertex buffer, `render` records draws

The middleware pattern mirrors `egui-wgpu`'s prepare/render split.

`TextRenderer::prepare` (`text_render.rs:50-335`):
1. Clear `glyph_vertices`.
2. For each `TextArea { buffer, left, top, scale, bounds, default_color, custom_glyphs }` and each visible `LayoutRun`/`LayoutGlyph`, call `prepare_glyph` (`text_render.rs:438-622`).
3. `prepare_glyph` looks up `cache_key` in the mask atlas, then color atlas; on miss, rasterizes via swash, calls `inner.try_allocate`, on failure loops `atlas.grow` until success or `Err(PrepareError::AtlasFull)`.
4. Once the glyph is in the atlas, clip against `text_area.bounds` (lines 567-604 — manual rect clip, atlas-uv shift on left/top clip), then push a `GlyphToRender`.
5. End of loop: `queue.write_buffer` into the existing `vertex_buffer` if it fits, otherwise `destroy()` and re-create with `next_power_of_two` size (`text_render.rs:318-332`, `next_copy_buffer_size` at 371).

Run-level visibility is a quick `is_run_visible` check (`text_render.rs:236-242`) wrapping `skip_while + take_while` over `layout_runs()` — runs are y-sorted so this short-circuits both ends. No spatial index; large multi-area scenes loop linearly.

`TextRenderer::render` (`text_render.rs:338-355`) is six lines: set pipeline, bind atlas, bind viewport uniform, set vertex buffer, draw `0..4 × 0..N`. It does *not* begin a render pass — caller passes `&mut RenderPass`. This is the key middleware property: glyphon emits draws into someone else's pass, alongside whatever else they're rendering.

`Viewport::update(queue, Resolution)` (`viewport.rs:46-57`) writes the params uniform; only writes when changed. `RenderError::ScreenResolutionChanged` (`error.rs:25`) is *declared* as a possible error but the current code path does not check resolution between prepare and render — the variant is reserved.

## 5. Limitations

- **No real subpixel AA** (no LCD striping). Mask atlas is 8-bit single-channel coverage; subpixel positioning via 4×4 binned cache keys, but final filtering is per-pixel alpha. Issues #44, #76, #82 in the upstream tracker have lived for years; the consensus is "out of scope, swash doesn't expose it cleanly". For dark-on-light body text on a low-DPI display this is visibly fuzzier than CoreText/DirectWrite.
- **Color glyphs (emoji, COLR)** use the color atlas as plain RGBA bitmaps. Vector-color formats (COLRv1) are flattened by swash to whatever resolution `font_size` requested — no resolution-independent color glyphs.
- **Atlas full → hard error.** `PrepareError::AtlasFull` (`error.rs:8`) returns once growth hits `max_texture_dimension_2d` (commonly 8192 or 16384) and every glyph is in use this frame. There's no spilling to a second atlas, no shelf re-pack. In practice this requires *thousands* of distinct glyphs visible in one frame, but CJK + many sizes can hit it.
- **Atlas grow re-rasterizes everything.** `text_atlas.rs:148-211` re-runs swash for every cached glyph. First grow is cheap, later grows aren't.
- **Pipeline cache leaks at most a handful of variants.** `Cache::get_or_create_pipeline` (`cache.rs:199`) is a `Vec<(format, multisample, depth_stencil, RenderPipeline)>` linearly scanned. Fine because variants are usually 1-2.
- **No batching across `TextArea`s when they share state.** Each call to `prepare` rebuilds the entire vertex buffer from scratch — no incremental update.
- **Pixel snapping is opt-in per glyph.** `CustomGlyph::snap_to_physical_pixel` (`custom_glyph.rs:26`) snaps non-text glyphs; for shaped text, snapping happens via cosmic-text's own physical glyph projection. Mixing scaled and unscaled text in the same atlas at typical body sizes shows minor weight drift.
- **Shaping is fully synchronous, on the prepare thread.** Big text changes block the frame.
- **`shaper-applied` features are limited to what cosmic-text exposes** — variable-axis support, ligature toggles, OpenType feature lists are recent and partial. For a CAD-style UI with a single fixed font this is a non-issue; for rich-text editors it's a real ceiling.

## 6. Lessons for Palantir

**Use it directly, behind a thin adapter — don't fork.** The crate is small, scoped, and exactly the layer between cosmic-text and wgpu we'd otherwise write. Targeting it gets us shaped text, Unicode, BiDi, fallback fonts, color emoji, and glyph caching for ~1k LOC of dependency. The middleware pattern lets us call `text_renderer.render(&mut pass)` inside our own paint pass alongside the future rounded-rect SDF pipeline — no extra render passes, no surface ownership conflict.

**Map Palantir's `Shape::Text` → cosmic-text `Buffer` per measure.** `Buffer` is the right level: it's the unit cosmic-text shapes once and re-uses across `prepare` calls. Cache `Buffer`s in the persistent `Id → Any` state map (the same map that will hold scroll/focus state) keyed by `WidgetId`, so re-shaping only happens on text/font/width change. Re-shaping every frame because the tree is rebuilt would defeat cosmic-text's caching.

**Do measurement against `Buffer::layout_runs()`.** `Hug` on a text node = `set_size(None, None); shape_until_scroll(); take max width and total height of layout runs`. `Fill`/`Fixed` width = `set_size(Some(w), None); shape_until_scroll(); height = sum(line_height × runs)`. This is exactly what egui's `WidgetText::into_galley` does internally.

**Lifecycle alignment with our four passes:**
- *Record:* push `Shape::Text { buffer_id, color, bounds }` referencing a cached `Buffer`. No shaping yet.
- *Measure:* if the buffer's constraint changed since last frame, `buffer.set_size(...) + shape_until_scroll(...)`; read sized layout runs out.
- *Arrange:* assigns owner rect; nothing text-specific.
- *Paint:* during the wgpu pass, build a `Vec<TextArea>` from the shape list and call `text_renderer.prepare` once per frame, then `render` inside the pass. Single prepare-per-frame matches glyphon's design.

**What we need on top:**
- A wrapper that owns `FontSystem`, `SwashCache`, `Cache`, `TextAtlas`, `TextRenderer`, `Viewport`. One of each per renderer is fine.
- A `Buffer` cache keyed on `(WidgetId, font, size, max_width)` — invalidated when the `Shape::Text` content hash changes. Cosmic-text doesn't dedupe `Buffer`s for us.
- Don't forget `atlas.trim()` at end-of-frame (example/hello-world.rs:255). Without it `glyphs_in_use` grows monotonically and eviction stops working. Wire into the paint-pass tail.
- Bounds clipping is per-`TextArea`, axis-aligned only. For our future rotated/clipped widgets we'd need scissor in the render pass — glyphon's `TextBounds` is a CPU-side rect clip on glyph quads.

**Where it falls short for Palantir specifically:**
- *Sub-pixel AA.* If the design target is "sharp text on 1× monitors" we'll outgrow glyphon. Vello/parley is the realistic upgrade path; deferring is fine for a wgpu-first prototype where we assume hi-DPI.
- *Rich text styling per run.* `TextArea` has one `default_color`; per-glyph color comes from `LayoutGlyph::color_opt` which is set via `Attrs` on the `Buffer` at shape time. Workable but means rich text = re-shape, not re-style.
- *Damage tracking.* glyphon rebuilds its vertex buffer every `prepare`. Once we add dirty-rect rendering, glyphon will still re-prepare the world — acceptable up to maybe 10k glyphs/frame, then a problem. Not the prototype's problem.

**Don't copy:**
- The `Cache::Cache(Arc<Inner>)` clone-everywhere pattern. We have one renderer; `Cache` can be a plain owned struct embedded in our renderer state.
- The two-atlas split *inside one renderer* if we end up wanting more than two content types (e.g. distance-field icons). Better to keep glyphon's atlas as-is for text only, and have separate atlases for our own SDF rounded-rects/icons.
- `RasterizeCustomGlyph` for our own shapes. It's a hook for non-text glyph atlases (SVG icons inline with text). Our shapes have their own pipeline; routing them through glyphon's vertex format would constrain us pointlessly.

**Single biggest takeaway:** glyphon is a clean middleware in the same shape as our planned paint pass — caller-owned wgpu, prepare/render split, instanced quads, growable atlas with LRU eviction. Wrap it, cache `Buffer`s in our state map, call `trim()` after present, and text becomes a solved problem until we hit the subpixel-AA ceiling.
