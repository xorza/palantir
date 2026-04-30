# Vello — reference notes for Palantir

Vello is a compute-shader 2D path renderer (the production heir to piet-gpu). It treats the GPU as a parallel computer rather than a triangle pipe: the user records a `Scene`, the encoder packs it into flat parallel streams, and ~14 compute dispatches consume them through prefix-scan / monoid passes to produce a per-tile command list, which a final compute "fine" pass shades to the output texture. No CPU tessellation. This is the most ambitious end of the design space Palantir has to choose against.

All paths are under `tmp/vello/`.

## 1. Scene encoding — flat parallel streams, not a display list

`Scene` (`vello/src/scene.rs:43`) is a thin façade over `Encoding` (`vello_encoding/src/encoding.rs:26`). The encoding is **six parallel SoA streams**:

```text
path_tags:  Vec<PathTag>     // 1 byte per element: segment kind + transform/style/path markers
path_data:  Vec<u32>         // raw f32 or i16 coords, indexed by tag stream
draw_tags:  Vec<DrawTag>     // 1 word per draw object (COLOR=0x44, LINEAR_GRADIENT=0x114, …)
draw_data:  Vec<u32>         // payload for each draw object
transforms: Vec<Transform>   // 6-float affines
styles:     Vec<Style>       // packed flags+miter / line_width
```

Each `PathTag` (`vello_encoding/src/path.rs:246`) packs into 8 bits: 3 bits segment type (`LINE_TO`/`QUAD_TO`/`CUBIC_TO`), 1 bit `F32_BIT` (else `i16` coords), 1 bit `SUBPATH_END_BIT`, plus `TRANSFORM=0x20`, `PATH=0x10`, `STYLE=0x40` markers that ride in the same stream. State changes (transform, style) are *interleaved* with geometry rather than living in a separate command list — this is what makes the prefix-scan model work.

`DrawTag` (`vello_encoding/src/draw.rs:13`) encodes both kind and stream sizes: top nibble of the constant is the info-buffer width, bottom nibble the draw-data size. `BLUR_RECT = 0x2d4` therefore declares 11 info words + 5 draw words. The shader reads the layout straight off the tag — no dispatch table, no virtual call.

`Resources` (referenced from `encoding.rs:42`) holds *late-bound* data: `glyph_runs: Vec<GlyphRun>`, gradient ramps, image blobs, and `patches: Vec<Patch>` markers. At resolve time, `Resolver` (`vello_encoding/src/resolve.rs`) walks patches and splices glyph subpaths and ramp/atlas indices back into the streams, producing a single packed `Vec<u8>` (`Layout` at `resolve.rs:18` records the byte offset of each substream).

The whole thing is `Pod`/`Zeroable` — `bytemuck::cast_slice` straight to a wgpu storage buffer with no marshalling.

## 2. The pipeline — prefix scans all the way down

`Render::render_encoding_coarse` (`vello/src/render.rs:135`) records ~14 dispatches in fixed order. The pattern is "reduce → scan → leaf" repeated for each derived quantity:

1. `pathtag_reduce` / `pathtag_scan` (`render.rs:250-294`) — prefix-sum `PathMonoid` over the tag stream so every tag knows its global path index, transform index, style index, and segment offset. `pathtag_reduce2` + `pathtag_scan1` is the two-level decoupled-lookback variant for very large scenes (`use_large_path_scan` switch at `render.rs:257`).
2. `bbox_clear` then `flatten` (`render.rs:304-328`) — one wg per path, flattens cubics/quads into line segments and writes per-path bboxes. `lines_buf` is allocated from a GPU bump allocator (`bump_buf`), which is the recurring memory pattern.
3. `draw_reduce` / `draw_leaf` (`render.rs:333-358`) — prefix scan the draw-tag stream into `DrawMonoid`s (per-object info offset and scene offset).
4. `clip_reduce` / `clip_leaf` (`render.rs:368-393`) — pair `BEGIN_CLIP`/`END_CLIP` tags into a balanced bracket structure and propagate clip bboxes via a binary-comonoid scan (`Bic`).
5. `binning` (`render.rs:405`) — bucket each draw into 256×256 screen bins.
6. `tile_alloc` + `path_count_setup` + `path_count` (`render.rs:425-464`) — count segment-tile intersections, allocate per-tile segment ranges out of `seg_counts_buf` via atomic bumps.
7. `backdrop` (`render.rs:465`) — propagate winding-number backdrops along tile rows.
8. `coarse` (`render.rs:470`, shader at `vello_shaders/shader/coarse.wgsl`, 469 lines) — emit per-tile **PTCL** (Per-Tile Command List): a packed bytecode like `CMD_FILL` / `CMD_SOLID` / `CMD_COLOR` / `CMD_LIN_GRAD` / `CMD_BLUR_RECT`. This is the moment the work pivots from "list of paths" to "list of tiles, each carrying the paths that touch it".
9. `path_tiling` (`render.rs:490`) — write actual segment data into per-tile slots.
10. `fine` (`render.rs::record_fine`, shader at `vello_shaders/shader/fine.wgsl`, **1402 lines**) — one workgroup per 16×16 tile. Reads the PTCL, walks commands, and for `CMD_FILL` accumulates analytic-area-coverage from segment lists into a per-pixel area buffer; mixes with brushes (solid, gradient, image, blurred-rounded-rect closed form) and writes RGBA to the output texture. Blend stack (`blend_spill_buf` at `render.rs:53`) is a register-spill region for nested layers exceeding shared-memory budget.

Everything between record and fine is **memory layout transformation by parallel scan**. There is no rasterizer in the GPU sense; coverage is integrated analytically per-pixel from segment endpoints in the fine pass.

## 3. Path encoding, gradients, blurs

Paths: `PathEncoder` (referenced from `vello_encoding/src/path.rs`, see `Style::from_fill` at `path.rs:71` and `from_stroke` at `path.rs:85`) appends one tag byte + N coord words per segment. Closed-form curves are expressed as quad/cubic Béziers; `flatten.wgsl` (923 lines) does Wang-style adaptive subdivision on GPU using Raph Levien's Euler-spiral framing (`vello_shaders/src/cpu/euler.rs`).

Gradients: `Ramps` (`vello_encoding/src/ramp_cache.rs`) bake stop arrays into a 512-px-wide texture, one row per gradient, indexed from `DrawLinearGradient`/`DrawRadialGradient`/`DrawSweepGradient`. The fine shader samples the ramp texture by the parametric coordinate computed per-pixel (`fine.wgsl:26`).

**Blurred rounded rect**: this is the load-bearing UI primitive Palantir cares about. `Scene::draw_blurred_rounded_rect` (`scene.rs:254`) encodes the bounding shape as a normal path, then attaches a `DrawBlurRoundedRect` (tag `BLUR_RECT = 0x2d4`) carrying `(rgba, width, height, radius, std_dev)`. The fine shader handles `CMD_BLUR_RECT` with a closed-form analytic Gaussian-blurred-rounded-rect approximation per-pixel — no FFT, no separable blur, no offscreen render target. **This is the only "primitive" in Vello that isn't a path**, and it exists precisely because shadows are too expensive to do as paths-with-blur. Worth noting for Palantir's drop-shadow story.

## 4. Glyphs — outlines, not an atlas

`Scene::draw_glyphs(font)` (`scene.rs:453`) returns `DrawGlyphs<'_>`. User passes an iterator of `Glyph { id, x, y }`; `DrawGlyphs::draw_outline_glyphs` (`scene.rs:619`) appends a `GlyphRun` to `resources.glyph_runs` and a `Patch::GlyphRun` marker. Skrifa (`use skrifa::...` at `scene.rs:14-22`) extracts each glyph's outline at resolve time; `GlyphCache` (`vello_encoding/src/glyph_cache.rs:15`) keys on `(font_id, font_index, size_bits, style_bits, coords)` and caches the encoded `Encoding` per glyph as `Arc<Encoding>`. At resolve, the cached glyph encoding is appended into the scene streams.

Color glyphs (COLR, bitmap) are detected via `font.colr().is_ok() && font.cpal().is_ok()` (`scene.rs:604`) and routed through `try_draw_colr` which expands each color glyph into multiple drawing commands — Vello renders colored emoji as compositions of paths, not as bitmap blits.

**There is no glyph atlas.** Each glyph is rendered as paths every frame it appears. The cache is on the *encoding* level, not pixels. This is opposite to egui/skia and is only viable because Vello's path pipeline is so cheap.

## 5. Why CPU tessellation lost (per Vello's design rationale)

Documented in `tmp/vello/doc/ARCHITECTURE.md:9-13`, `doc/blogs.md`, `roadmap_2023.md`, and Raph Levien's piet-gpu posts (`raphlinus.github.io/rust/graphics/gpu/2020/06/13/fast-2d-rendering.html`, `…/2020/06/01/anti-aliasing.html`, "Potato" doc linked from `sparse_strips/README.md`). Summary of what kills CPU tessellation at scale:

- **Bandwidth.** A complex SVG can produce millions of triangles. Uploading them every frame swamps PCIe even before rasterization.
- **Allocation.** Tessellator output size is data-dependent and unpredictable; either you over-allocate (waste) or grow buffers (reupload). Vello's GPU bump allocators (`BumpAllocators`, `vello_encoding/src/config.rs`) bound this with feedback-driven retry.
- **Path joins/caps under transform.** Stroke expansion is non-trivial and changes with zoom. Tessellate-once-per-frame becomes tessellate-per-paint-per-zoom.
- **Sorting / clip stacks.** Order-dependent compositing forces serialization at exactly the layer triangulation wants to parallelize.
- **Anti-aliasing quality.** MSAA on triangulated paths is fixed-rate; Vello's analytic-area-coverage in `fine.wgsl` is exact and free in the same dispatch.

The Vello bet: spend GPU compute (cheap, parallel, no PCIe trip) instead of CPU tessellation (serial, allocates, then transferred).

## 6. Hybrid CPU/GPU — the retreat

`tmp/vello/sparse_strips/README.md` and the "Potato" doc (Raph Levien, linked there) document a strategic retrofit. The pure-compute pipeline has real costs:

- WebGL / older devices / iOS-pre-compute have no compute shaders.
- Many small scenes pay a fixed pipeline cost (~14 dispatches) regardless of complexity.
- Robust dynamic memory (the `robust` flag at `render.rs:142`) requires bump-allocation readback round-trips when first-pass overflow is detected.
- `vello_shaders/src/cpu/*.rs` mirrors every GPU shader as a CPU function (`coarse.rs`, `fine.rs`, `flatten.rs`, …) — an entire second implementation, kept in lockstep, used as fallback.

`vello_hybrid` (sparse-strip approach) moves path processing (flatten, tile, sparse-strip allocation) to CPU and uses the GPU only for compositing the strips through fragment+vertex shaders. This trades compute-shader dependence for a slightly different scaling curve and ships on WebGL2. The fact that this exists at all is a tell that the pure-GPU bet was too aggressive for the breadth of deployment targets.

## 7. Lessons for Palantir

**Don't use Vello directly (yet) — wrong scale.** Palantir's per-frame scene is a few dozen rounded rects and some text. Vello pays a fixed pipeline cost (~14 dispatches, scratch buffer allocations, gradient texture, image atlas, glyph encoding cache) that's calibrated for tens-of-thousands of paths. For a button toolbar, instanced SDF quads + glyph atlas will be one or two dispatches and noticeably less GPU memory. Reconsider when Palantir gains real vector content (icon paths beyond what an atlas can carry, plotted curves, complex stroke decorations).

**Steal the encoding model.** This is the highest-leverage idea regardless of renderer choice:
- Append-only **flat parallel streams** instead of a `Vec<enum Shape>`. Palantir already has `Tree.shapes` flat — extend the discipline: separate `tags`, `data`, and `style` streams, all `Pod`. One `bytemuck::cast_slice` to a storage buffer; zero per-shape boxing.
- **Tag-as-layout-descriptor** (Vello's trick where `DrawTag = 0x2d4` literally encodes payload size). Lets a GPU shader iterate without a dispatch table; lets CPU code skip the unknown-size enum problem. If Palantir keeps a CPU paint path, this still helps cache density.
- **Per-instance state via interleaved markers** (`PathTag::TRANSFORM`, `STYLE`) rather than per-shape struct fields. Most siblings share transform/style — interleave the change. Cuts encoding size dramatically for typical UI.

**Steal the blurred-rounded-rect primitive verbatim.** `vello/src/scene.rs:254` + the `CMD_BLUR_RECT` path in `fine.wgsl` is a closed-form analytic Gaussian-blurred-rounded-rect shader. Drop shadows in UI are exactly this primitive. Copy the shader math (it's ~30 WGSL lines), encode as one more case in Palantir's SDF-quad pipeline. Avoids both an offscreen blur RT and a separable-blur compute pass.

**Don't steal: glyph-as-paths.** Vello renders every glyph as outlines every frame because its path pipeline is free. Palantir's renderer is not free. Use a glyph atlas (cosmic-text/glyphon style — see `references/` adjacent docs once written). Vello's glyph *cache by encoding* is the wrong abstraction at our volume.

**Don't steal: prefix-scan rasterization.** A four-stage `reduce → scan → leaf → fine` pipeline buys parallelism across millions of paths. Palantir has dozens. Each scan dispatch has a launch-overhead floor that dwarfs the work for our scenes.

**Watch the hybrid retreat (`sparse_strips/`).** If Palantir ever wants WebGL deployment or to support older Android/iOS GPUs, the sparse-strips approach is the prior art: do path processing CPU-side, ship strip data to GPU, composite with vertex/fragment only. That maps cleanly onto Palantir's existing CPU-side measure/arrange/paint walk.

**Concrete short-term action.** Adopt the encoding model now (cheap, helps any renderer); plan for SDF-instanced rounded-rects + glyph atlas + Vello's blur-rect formula as the v1 GPU path. Treat Vello itself as the "if Palantir grows into a graphics editor" upgrade path, not the v1 renderer.
