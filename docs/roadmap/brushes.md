# Brushes: gradients & image fills

This doc proposes a `Brush` surface that adds linear / radial / conic
gradients and image fills, the GPU encoding to support them, and an
incremental rollout that does not regress the steady-state alloc-free
contract.

## Status (2026-05)

- **Slice 1 (Brush enum + Solid migration):** shipped.
- **Slice 2 (Linear gradient):** shipped.
- **Slice 3 (Radial + Conic):** shipped.
- **Slice 4 (Image fills):** **not started.**
- **Slice 5 (OKLab interp default, polish):** **not started** — current
  per-variant defaults are `Linear` for Conic and as-supplied for
  Linear / Radial; CSS Color 4 alignment (OKLab default) is open.

Divergence from the plan as designed below — current code matches the
intent but not the literal types:

- **No `SolidQuad` / `Quad` split.** Single 92 B `Quad` (`src/renderer/quad.rs`),
  hot-path solid quads fill in zeroed brush fields. Throughput delta vs the
  planned 68 B `SolidQuad` was deemed not worth the second pipeline.
- **Gradient structs are inline, not behind `GradientId(u64)` + arena.**
  `Brush::Linear(LinearGradient)` etc. carry `ArrayVec<Stop, MAX_STOPS>`
  inline (~80 B per variant), `Brush` is `Copy`. The "gradient morph
  animation" Future-work section's **path 3 (inline data) is therefore
  already structurally available** — only the `Animatable for Brush`
  impl still snaps gradient-↔-gradient lerps (`src/primitives/brush.rs:474`).
  Lifting that to a stop-wise lerp on matching variant + matching stop
  count is now a localised change.
- **LUT atlas uses content-hash + linear probe + LRU eviction** (256 rows,
  `src/renderer/gradient_atlas.rs`). Functionally equivalent to the planned
  "content-addressed rows"; LRU triggers only when the table is full and
  the new content is absent.
- **Cmd buffer is not split into `DrawRect` / `DrawRectBrush`.** Brush
  metadata rides inline on the existing draw-rect path.
- **`docs/roadmap/brushes-slice-2-plan.md`** is referenced from
  `src/renderer/quad.rs:92` but does not exist — either restore as a
  historical artifact or drop the reference.

## Today

- `Background { fill: Color, stroke: Stroke, radius: Corners }`
  in `src/primitives/background.rs:25`. Stored as `Tree.chrome:
  SparseColumn<Background>` (`src/forest/tree/mod.rs:131`), filtered out when
  `is_noop()` so transparent panels stay zero-cost.
- `Stroke { color: Color, width: f32 }` (`src/primitives/stroke.rs:18`),
  `#[repr(C)] Pod` with hand-`Hash` over `bytes_of(self)`
  (`stroke.rs:45-50`). Widening to `Brush` makes `Stroke` non-`Pod`; the
  hand-`Hash` impl has to be rewritten in terms of the brush fields, not a
  byte-slice.
- Six shape variants carry colour today: `Shape::RoundedRect.fill`,
  `Shape::Line.color`, `Shape::CubicBezier.color`,
  `Shape::QuadraticBezier.color`, `Shape::Text.color`, `Shape::Mesh.tint`
  (`src/shape.rs:25,36,66,77,85,99`), plus `Shape::Polyline` with
  `PolylineColors::{Single,PerPoint,PerSegment}` (`src/shape.rs:107`).
  `Shape::Line` lowers to `ShapeRecord::Polyline` at authoring time
  (`src/forest/shapes.rs:280`); there is no `ShapeRecord::Line` and the
  encoder paints lines via the polyline path (`encoder/mod.rs:128`).
- Encoder emits `DrawRect` / `DrawRectStroked` / `DrawText` / `DrawPolyline`
  / `DrawMesh` (`src/renderer/frontend/cmd_buffer/mod.rs:47-60`); composer
  lowers to a 68 B `Quad { rect, fill, radius, stroke }`
  (`src/renderer/quad.rs:17`, pinned by `size_of::<Quad>() == 68` at
  `quad.rs:36`); shader `src/renderer/backend/quad.wgsl` does the SDF
  rounded-rect with one or two AA bands.
- The only texture in flight is glyphon's text atlas
  (`src/renderer/backend/text.rs`). No user-image path.
- `Background` participates in the per-node hash
  (`src/forest/tree/mod.rs:233`), so cache eviction + damage are already
  wired.
- **No encode cache, no compose cache.** Both were implemented and removed
  after profiling; see `src/renderer/frontend/encoder/encode-cache.md`. The
  encoder rebuilds the cmd buffer from scratch every frame. Anywhere this
  doc talks about "encode-cache coherence" below is conditional on the
  cache returning — keep the LUT row-addressing scheme either way because
  it costs nothing and makes a future cache trivial.

## Goals

- One `Brush` enum that fills and strokes both consume — no separate
  `FillBrush` / `StrokeBrush` types.
- `impl From<Color> for Brush` so widget/theme/showcase call sites
  (`fill: palette::ELEM`, `stroke: Stroke { color: ..., width }`) keep
  compiling unchanged after the rename. Without it slice 1 churns ~30
  files for no semantic gain.
- Linear, radial, conic gradients with arbitrary stop counts (cap at 16) and
  pad / repeat / reflect spread modes.
- Image fills referencing user-uploaded textures with object-space UV mapping
  and a tile / stretch mode.
- Steady-state heap-alloc-free: brush data lives in retained `CacheArena`s,
  not new `Vec`s per frame.
- No regression to the solid-color fast path: a `Brush::Solid(Color)` must
  encode to exactly the same `Quad` bytes that today's `Color` does.
- Cache invalidation falls out of the existing per-node hash; gradients and
  images participate by hashing their content key.

## Non-goals (for v1)

- Mesh / pattern / visual brushes (WPF `VisualBrush`).
- Along-stroke gradient parameterization — object-space only. Defer to a
  future `StrokeBrush::AlongPath` if a workload appears.
- SVG-style `gradientUnits="userSpaceOnUse"` — object-space (= node-local
  rect) is the only mode. Re-use across siblings is cheap because the LUT
  cache keys on stops, not geometry.
- Sub-rect masking, conic two-point gradients, gradient meshes.

## Brush surface

```rust
// src/primitives/brush.rs
pub enum Brush {
    Solid(Color),
    LinearGradient(GradientId),
    RadialGradient(GradientId),
    ConicGradient(GradientId),
    Image(ImageBrush),
}

/// Content hash of a registered gradient/image. Stable across frames:
/// the same gradient definition always produces the same id, so it
/// participates in `Brush`'s `Hash` impl as content (not as a slot
/// position). The arena is a `FxHashMap<GradientId, _>`, not a `Vec`.
pub struct GradientId(u64);
pub struct ImageId(u64);

pub struct LinearGradient {
    pub angle: Radians,             // 0 = →, π/2 = ↓
    pub stops: ArrayVec<Stop, 16>,  // hard-cap 16, no heap ever
    pub spread: Spread,
    pub interp: Interp,             // Srgb | Linear | Oklab | Oklch{hue}
}

pub struct RadialGradient {
    pub center: Vec2,               // object-space, 0..1
    pub radius: Vec2,               // object-space, 0..1
    pub stops: ArrayVec<Stop, 16>,
    pub spread: Spread,
    pub interp: Interp,
}

pub struct ConicGradient {
    pub center: Vec2,
    pub start_angle: Radians,
    pub stops: ArrayVec<Stop, 16>,
    pub spread: Spread,
    pub interp: Interp,
}

pub struct Stop { pub offset: f32, pub color: Color }

#[repr(u32)] pub enum Spread { Pad = 0, Repeat = 1, Reflect = 2 }
#[repr(u32)] pub enum Interp { Linear = 0, Oklab = 1, Srgb = 2 }
#[repr(u32)] pub enum ImageFit { Stretch = 0, Tile = 1, Fit = 2, Cover = 3 }

/// Image fill — registered separately so `sizeof(Brush)` stays small.
/// `uv` and `fit` live on the registered `ImageRegistration`, keyed by
/// `ImageId`. `Brush::Image(ImageId)` carries 8 B + tag.
pub struct ImageRegistration {
    pub image: GpuImageHandle,
    pub uv: Rect,
    pub fit: ImageFit,
}
```

`sizeof(Brush) ≈ 16 B` (tag + `Color` for the hot solid case, tag + `u64`
for everything else) — matches today's `Color`-typed `Background.fill`,
so widening to `Brush` doesn't bloat `Background` / `Stroke` / authoring
parameters. Image params (uv, fit) live on the registration, not in the
`Brush` value.

`Background` and `Shape::RoundedRect` carry `Brush` instead of `Color`.
`Stroke` becomes `Stroke { brush: Brush, width: f32 }` for symmetry — this
drops `Stroke`'s `bytemuck::Pod` derive (a `Brush` enum can't be `Pod`),
so the hand-`Hash` impl at `src/primitives/stroke.rs:45-50` swaps from
`state.write(bytes_of(self))` to hashing `brush` + `width` directly.

`Shape::Line`, `Shape::CubicBezier`, `Shape::QuadraticBezier`, `Shape::Text`,
and `Shape::Mesh` all swap their `color`/`tint` field to `Brush` in the
same slice — leaving stragglers is exactly the half-finished state the
codebase forbids. `Shape::Polyline`'s `PolylineColors` is the one
exception: `Single(Color)` becomes `Single(Brush)`, but
`PerPoint(&[Color])` / `PerSegment(&[Color])` stay `Color`-typed. Per-vertex
gradient/image fills aren't meaningful (each vertex would need its own
brush evaluation context) and v1 explicitly defers parametric-t along-path
brushing.

### `Animatable` for `Brush`

`Background` and `Stroke` currently derive `Animatable` and lerp colors
componentwise — Button hover/press depends on it. Generic `Brush` can't
lerp across variants (no meaning to "halfway between a solid red and a
radial gradient"), and `Arc<LinearGradient>` etc. can't lerp *within* a
variant without allocating a new `Arc` per frame (violates the
steady-state alloc-free contract). So we hand-write `Animatable for
Brush` with one rule:

- `(Brush::Solid(a), Brush::Solid(b))` → componentwise color lerp.
- Any other pair → **snap at `t = 1.0`** (the discrete-state convention
  already used for `Corners` via `#[animate(snap)]`).

That includes gradient ↔ gradient pairs **even when both sides are the
same variant with matching stop counts.** See "Future work: gradient
morph animation" below for the path to lifting this restriction.

A test (`button_hover_color_lerp_unchanged`) lands in slice 1 and pins
the solid-solid path; widget color animations are untouched by the
migration.

## User types vs GPU types

Two layers, deliberately separate:

- **User-facing types** (`primitives/`): `Brush`, `Background`, `Stroke`,
  `Shape`. Authoring vocabulary. `Stroke` carries a `Brush`, not a flat
  color. None of these are `Pod` — they don't go to the GPU as bytes.
- **GPU instance types** (`renderer/quad.rs`): `Quad` and `BrushSlot`.
  `Pod`, `repr(C)`, exact wire layout. The composer is the only translator:
  it pulls a `Brush` apart and writes flat bytes (or a `BrushSlot` with a
  kind tag + LUT row) into the instance buffer.

There is no `GpuStroke` / `StrokeWire` / similar named type. Stroke on the
GPU is two inline fields in `Quad` (`stroke_brush: BrushSlot`,
`stroke_width: f32`) — the WGSL `vertex_attr_array` already treats them as
independent locations, so a wrapper struct would be naming for naming's
sake. Same rationale for fill: `fill_brush: BrushSlot` lives inline in
`Quad`, not behind a sub-struct.

This split means the user-facing `Stroke` is free to grow brush enums,
animation hooks, etc. without touching the GPU layout, and the GPU layout
is free to repack for cache reasons without leaking through to widget code.

## GPU encoding

Pick **per-instance brush kind tag + LUT atlas for gradients + page atlas
for images**. Three rationales drive this:

1. WebGPU has no real bindless (`gpuweb#380`); per-image bind groups would
   gut throughput, so we batch through atlases.
2. Vello's 1D ramp LUT is the proven shape for "many gradients, few stops"
   workloads — it amortizes stop processing across all draws and keeps the
   shader to a single `textureSample`.
3. Iced's inline-stops-in-instance-data hits an 8-stop ceiling and bloats
   the quad. We want >8 occasionally (multi-stop UI bars) without paying
   per-instance bytes for the common 2-stop case.

### Quad layout

Extend the instance struct (`src/renderer/quad.rs`) with a small brush
header. Use `padding-struct` so future fields don't rot the layout:

```rust
// src/renderer/quad.rs — the only place these layouts live.
#[padding_struct::padding_struct]
#[repr(C)]
pub(crate) struct Quad {
    pub rect: Rect,
    pub radius: Corners,
    pub stroke_width: f32,
    pub fill_brush: BrushSlot,    // 24 B, inline
    pub stroke_brush: BrushSlot,  // 24 B, inline
}

#[padding_struct::padding_struct]
#[repr(C)]
pub(crate) struct BrushSlot {
    /// low 8: kind (0=solid, 1=linear, 2=radial, 3=conic, 4=image).
    /// high 24: aux index — `lut_row` for gradients, `atlas_page` for image.
    pub kind: u32,
    /// low 8: spread (gradients) or fit (image).
    /// rest: reserved.
    pub mode: u32,
    /// kind-dependent payload, *one* `[f32; 4]` (not two — there isn't room):
    ///   solid:  payload = rgba.
    ///   linear: payload = (axis_dx, axis_dy, t0, t1) in object-local 0..1.
    ///   radial: payload = (cx, cy, rx, ry).
    ///   conic:  payload = (cx, cy, start_angle, _).
    ///   image:  payload = (uv_min_x, uv_min_y, uv_max_x, uv_max_y).
    pub payload: [f32; 4],
}
```

`BrushSlot` is the GPU wire encoding of a user-facing `Brush`. The composer
builds it; nothing outside `renderer/` constructs one. It has no
`From<Brush>` impl — the conversion needs `&mut GradientLutAtlas` /
`&mut ImageAtlas` to resolve aux indices — and is `pub(crate)` only so the
backend can bind-group it.

Net: 16 + 16 + 4 + 24 + 24 = **84 B** per quad (was 68). Verified by hand:
no auxiliary `[f32; 4]` second slot; gradient row / atlas page / spread /
fit pack into the spare 24 bits of `kind` and 24 bits of `mode`. A
`size_of::<Quad>() == 84` test pins the layout. Throughput delta measured
in `benches/quad_throughput.rs` before merging slice 2.

### Gradient LUT atlas

`GradientLutAtlas` (`src/renderer/backend/gradient_lut.rs`):

- Single `Rgba8Unorm` texture, 256 px wide, **256 rows fixed** (no
  grow-on-demand path until profiling demands it — keep one shape, exercise
  it well). Each row is one baked gradient. Shader samples with `linear`
  filter; spread mode is folded into `t` in shader before the sample.
- **Content-addressed rows.** `lut_row = (stops_hash ^ interp_tag) % 256`.
  Bake is idempotent: same content → same row, never overwritten with
  different content. A row collision (two distinct gradients hashing to the
  same row) is resolved by linear probing into a small overflow set; if
  the table is genuinely full, oldest-frame entries get re-baked first.
  This makes encode-cache coherence trivial — a cached `BrushSlot` with
  `lut_row = 5` always paints the gradient it was baked from, because
  row 5 either still holds it or got re-baked into a different row.
- Bake on CPU: convert each stop to OKLab (or selected interp space),
  interpolate per-texel, premultiply alpha, write to staging row
  (`bake_scratch: [u8; 1024]` retained on the atlas), upload via
  `queue.write_texture` (single row, never re-create the texture).
- The atlas lives on the renderer side; the encoder resolves
  `Brush::LinearGradient(id)` → `BrushSlot { kind, lut_row, mode, payload }`
  while building the cmd buffer.

### Image atlas

`ImageAtlas` (`src/renderer/backend/image_atlas.rs`):

- `Vec<AtlasPage>` of `Rgba8UnormSrgb` 4096² pages. Shelf packer for
  uploads; sort draws by page so each page = one bind group.
- `ImageId(u64)` is content hash of the source bytes; `ImageRegistration`
  in `Ui::brushes` maps `ImageId → (page: u16, rect: URect, uv, fit)`.
- Sub-page LRU is out of scope for v1 — once uploaded, an image lives until
  the renderer's `release_image(id)` is called.

**Upload ownership.** `Ui` doesn't hold a `wgpu::Queue`; the renderer does.
The image API lives on the app-level renderer handle:

```rust
impl PalantirRenderer {
    pub fn upload_image(&mut self, rgba: &[u8], size: USize) -> ImageId;
    pub fn release_image(&mut self, id: ImageId);
}
```

`Ui` consumes ids only — `Brush::Image(id)`. The composer validates each
id against the live registry; an unknown id paints a magenta debug fill
(loud, never silent), so a "register before paint" ordering bug surfaces
immediately in the showcase rather than corrupting the atlas.

### Shader

Single über-shader (`src/renderer/backend/quad.wgsl`) gains a brush
evaluation function:

```wgsl
fn eval_brush(slot: BrushSlot, p_local: vec2<f32>) -> vec4<f32> {
    switch slot.kind {
        case SOLID:    { return slot.solid_or_origin; }
        case LINEAR:   { let t = fold(linear_t(p_local, slot), slot.spread);
                         return textureSample(lut, lut_samp, vec2(t, slot.extra.y)); }
        case RADIAL:   { /* ... */ }
        case CONIC:    { /* ... */ }
        case IMAGE:    { let uv = image_uv(p_local, slot);
                         return textureSample(images, img_samp, uv, page); }
        default:       { return vec4(0.0); }
    }
}
```

Coverage stays where it is (`fwidth(d)` SDF) and is multiplied at the very
end against the premultiplied brush color. Branch is uniform-coherent
within a draw because the composer groups by `(scissor, brush_kind)`.

### Solid fast path

When both `fill.kind == SOLID && stroke.kind == SOLID`, the composer
routes the quad through a solid-only pipeline whose instance struct is the
slim 68 B `SolidQuad` — same bytes as today's `Quad`. The brush pipeline
(84 B `Quad`) is bound only when at least one slot is non-solid (mixed
solid-fill / gradient-stroke goes through the brush pipeline; that case
is rare and not worth a third pipeline). The two GPU structs sit side by
side in `renderer/quad.rs`:

```rust
#[repr(C)] #[derive(Pod, Zeroable)]
pub(crate) struct SolidQuad { rect: Rect, fill: Color, radius: Corners,
                              stroke_color: Color, stroke_width: f32 }   // 68 B

// `Quad` (84 B) above — the brush-pipeline instance.
```

Naming convention: `SolidQuad` for the slim layout, `Quad` for the
brush-capable layout. No "Legacy" prefix anywhere — both are first-class
and both stay in the codebase indefinitely.

### Cmd buffer wire

The encoder splits by kind too, not just the composer:

```rust
pub(crate) enum DrawCmd {
    DrawRect       { rect, radius, fill: Color },                   // unchanged
    DrawRectStroked{ rect, radius, fill: Color, stroke: StrokeRgb },// unchanged
    DrawRectBrush  { rect, radius, fill: BrushSlot,
                     stroke: BrushSlot, stroke_width: f32 },        // new
    // Text, EnterSubtree, … unchanged
}
```

Solid panels keep emitting today's `DrawRect` / `DrawRectStroked` — the
solid-subtree bytes are identical to pre-brush builds. `DrawRectBrush`
only appears when the encoder resolves a non-solid `Brush`. (No encode
cache to amortize against today; this matters for the slice-1 byte-equality
test and for a future cache.)

## Cache, hashing, damage

- `GradientId` and `ImageId` are **content hashes**, not slot indices.
  `Brush::LinearGradient(id).hash()` therefore depends on the gradient's
  *content*, not on a frame-local position. Two consequences:
  1. Same gradient definition across frames → same id → encode cache
     hits correctly.
  2. Different gradients across frames → different ids → encode cache
     misses correctly. Frame-local `Vec<LinearGradient>` would have given
     the wrong answer in both directions.
- `Background::hash` already runs inside the per-node hash
  (`src/forest/tree/mod.rs:233`); replacing `Color` with `Brush` means
  changing a stop or swapping an image invalidates the measure cache
  automatically. `Hash for Brush` must be hand-written (the variants
  carry `f32` payloads — pick a canonical encoding for the gradient/image
  axes and reuse `Color`'s existing `f32`-bit hash strategy). Note
  `Brush::Solid(c).hash() != c.hash()` because of the discriminant byte:
  every subtree key changes once on rollout, then is stable.
- **LUT atlas rows are content-addressed** (see "Gradient LUT atlas") so
  that *if* an encode/compose cache ever returns, cached `BrushSlot`s with
  `lut_row = R` stay valid: the only way `lut_row = R` exists is for the
  same content the row was baked from. Eviction means "row free for a
  different hash to claim" — no in-place overwrite of live content. With
  no caches today this is belt-and-braces; cheap to keep, painful to
  retrofit.
- Image atlas: pages aren't evicted within a frame; `release_image` is the
  only path that frees an atlas slot. If a cache returns, the renderer
  drops dependent cache entries on `release_image`.
- `Damage::Partial` paths still work because the brush eval is per-fragment;
  any rect the damage region covers re-runs the same shader against the
  current LUT/atlas state.

## Allocation discipline

- `Ui::brushes: BrushArena` is `FxHashMap<GradientId, LinearGradient>` (and
  the radial / conic / image-registration analogues), keyed by content
  hash. Across frames, repeat insertions of the same gradient are O(1)
  hash-lookup no-ops — no `Vec::push` churn, no per-frame `truncate(0)`.
  Eviction policy: drop entries unreferenced for N frames (start with
  N = 60, tune on workload).
- `Stop` lists are `ArrayVec<Stop, 16>` — heap-free, hard-capped at 16.
  `LinearGradient::new` `assert!`s the count.
- `BrushSlot` is `Pod` and lives inline in `Quad`; no per-frame heap.
- Stop interpolation buffers (256 × `Rgba8`) live in a renderer-side scratch
  `[u8; 1024]`, not allocated each bake.
- Image uploads are explicit and one-shot — never a per-frame surface.

## Testing

- `src/primitives/brush.rs` unit tests: hash stability across frames
  (`GradientId(content_hash)` is deterministic), `Brush::Solid → Brush::Solid`
  Animatable lerp, snap on cross-brush morphs.
- `button_hover_color_lerp_unchanged` (in widget tests) — pins that the
  `Brush::Solid` migration doesn't break Button's hover/press color tween.
- **Slice 1 pinning test** (`solid_panel_emits_legacy_quad_bytes`) asserts
  a solid `Background` records to today's exact `Quad` bytes (still 68 B
  in slice 1; `BrushSlot` doesn't exist yet).
- **Slice 2 pinning test migration**: the slice-1 test moves from
  "asserts `Quad` bytes" to "asserts `SolidQuad` bytes *and* asserts the
  composer routed the panel to the solid pipeline". `size_of::<Quad>() == 84`
  and `size_of::<SolidQuad>() == 68` get their own assertions in
  `renderer/quad.rs`.
- `src/renderer/backend/gradient_lut/tests.rs` golden test bakes a known
  3-stop gradient and pixel-compares the LUT row.
- LUT row collision test: insert two gradients designed to hash to the
  same row, verify probing places them on distinct rows and both render
  correctly.
- Showcase tabs land per-slice (slice 2: linear gradient grid; slice 3:
  radial + conic; slice 4: image fills + stroke-with-gradient). Slice 1 is
  rename-only — no new visuals to eyeball, that's expected. CLAUDE.md's UI
  rule applies from slice 2 onward.
- `benches/quad_throughput.rs` (new in slice 2) measuring 10k mixed-brush
  quads per frame; flag a regression if solid-only path slows.

## Rollout

Five slices, each shippable on its own. See "Status" at top for what's
landed; slices 1–3 are done, 4–5 remain.

1. **Brush type + Solid migration.** ✅ Introduce `Brush` enum with only the
   `Solid(Color)` variant, plus `impl From<Color> for Brush`. Replace
   `Color` on `Background`, `Stroke`, and **all six** coloured `Shape`
   variants (`RoundedRect.fill`, `Line.color`, `CubicBezier.color`,
   `QuadraticBezier.color`, `Text.color`, `Mesh.tint`), plus
   `PolylineColors::Single`. `PerPoint`/`PerSegment` stay `Color`-typed
   (per-vertex brush eval is out of scope). Hand-write `Hash for Brush`
   and `Animatable for Brush` (solid-solid lerp, snap otherwise) — write
   both **before** flipping the field types so the existing derives on
   `Background`/`Stroke` recompile cleanly. `Stroke` drops `bytemuck::Pod`
   and rewrites its hand-`Hash` impl off `bytes_of`. `Quad`/`BrushSlot`
   don't exist yet — composer stays on today's 68 B `Quad`, calling
   `brush.as_solid().unwrap()` (debug `expect`) to extract the color. Pin
   with the byte-equality test (the existing `size_of::<Quad>() == 68` at
   `quad.rs:36` covers half of it) and a new button-hover-lerp test.
   **Risk: rename churn touches every widget.** Acceptable per the
   project's "break things freely" posture; `From<Color>` keeps theme /
   widget / showcase call sites unchanged.

2. **Linear gradient.** ✅ — landed without the `SolidQuad`/`Quad` split
   (single 92 B `Quad`) and with inline gradient structs (no arena indirection).
   Add `LinearGradient` variant, `BrushArena`
   (FxHashMap, content-keyed), `GradientLutAtlas` (256×256
   content-addressed rows), `SolidQuad` (renamed today's `Quad`) and the
   new 84 B `Quad`, `BrushSlot`, `DrawRectBrush` cmd, branched shader,
   composer routing solid-only batches to `SolidQuad` and any non-solid
   batch to `Quad`. Migrate the slice-1 pinning test. Showcase tab gets a
   linear-gradient row. Spread modes from the start — they're three lines
   of WGSL each.

3. **Radial + conic.** ✅ Same atlas, new `kind` arms in the shader, conic AA
   via analytic angular derivative. Pin a conic seam test.

4. **Image fills.** ⏳ next slice candidate. `ImageAtlas`, `PalantirRenderer::upload_image`
   (renderer-owned, not on `Ui`), `Brush::Image(ImageId)`,
   `ImageRegistration` in `BrushArena`. Composer sorts draws by atlas page;
   one bind group per page. Magenta debug fill for unresolved ids. Start
   with `Stretch` and `Tile`; `Fit` / `Cover` are CPU-side `uv` adjustments
   on top of `Stretch`.

5. **Polish.** OKLab interpolation default (matches CSS Color 4), gradient
   along stroke if any workload demands it, `release_image` + atlas
   defragmentation if memory pressure shows in benches.

Each slice lands with `cargo fmt && cargo clippy && cargo test` clean and
the showcase tab updated.

## Future work: gradient morph animation

Gradient ↔ gradient lerping (e.g. a card whose linear gradient brightens
on hover by interpolating stop colors) is **deliberately snapped in v1**
because `Brush::Linear(Arc<LinearGradient>)` is immutable once
allocated; lerping would have to `Arc::new` per frame and violate the
steady-state alloc-free contract. Same for radial / conic.

If a workload demands smooth gradient morphs, three paths are open in
roughly increasing complexity:

1. **Accept per-frame `Arc::new` during animation only.** N allocs per
   frame where N = currently-animating gradients. Idle UI stays
   alloc-free; an in-flight hover transition burns ~200 B/frame of
   churn through a single allocator bucket. Defensible reading of the
   alloc-free rule (it targets idle redraw, not user-requested motion)
   but a strict reading rejects it. Gated by a `gradient_animation`
   bench that counts allocs per frame.

2. **Per-frame `BrushArena` scratch.** Add `Ui::brush_scratch:
   BrushArena` cleared at `post_record`, capacity-reused. `Brush::Linear`
   becomes `enum LinearHandle { Shared(Arc<…>), Frame(u32) }`; lerps
   push onto the scratch and return `Frame(idx)`. Truly alloc-free
   after warmup. Cost: `Hash` / `is_noop` / `PartialEq` on `Frame`
   variants need access to the arena, which means either pervasive
   `&Ui` plumbing on `Brush` methods, a thread-local arena (brittle),
   or an unsafe `*const LinearGradient` (lifetime via Ui invariant).
   Each option has real complexity.

3. **Inline gradient data.** Drop the `Arc`; carry `ArrayVec<Stop, N>`
   inline in the `Brush` enum. `sizeof(Brush)` jumps to ~170 B for
   N=8, ~330 B for N=16. Lerp becomes free and zero-alloc. Cost: every
   `Background` / `Stroke` / `Shape` grows correspondingly, hitting
   the `chrome` sparse column and the per-frame shape buffer.

Path 1 is cheapest to implement; path 2 is the principled answer; path
3 is the maximalist answer. Pick the cheapest that the workload needs.

## Open questions

- Do we want a `Brush::Pattern` (small repeating image with tint) ahead of
  full image fills? Cheaper to implement, addresses checkerboard /
  stripe / dot-grid use cases without a full atlas. Defer unless a workload
  asks.
- Should `interp` default to `Oklab` (modern, perceptually correct, slight
  bake cost) or `Linear` (simpler, what most engines do)? Lean Oklab —
  matches CSS Color 4 default and hides the muddy-midpoint footgun. Pin the
  choice in a test so a careless flip is visible.
- Stop count cap: 16 feels right (covers all real UI; LUT bake stays cheap).
  Hard-assert in `LinearGradient::new` rather than silently truncating.
- Should `BrushArena` live on `Ui` or on the renderer? Currently on `Ui`
  with content-hashed ids and N-frame eviction — gradient registration is
  authoring-time, not GPU-time, so it logically belongs alongside
  `StateMap`. The LUT atlas (renderer-side) is keyed by the same content
  hash, so the two halves stay in sync without a cross-side handshake.
  Eviction needs to hook into the same `removed` slice `post_record` uses
  for `StateMap`/`AnimMap`/`TextShaper`/`MeasureCache`, or it leaks.
- LUT row collision overflow: linear-probe within the 256 rows or fall
  back to a small "extras" row appended on demand? Probe is fine for
  expected workloads (few dozen distinct gradients/frame); revisit if
  collisions become a measurable miss rate.
- Gradient ↔ gradient and gradient ↔ solid morph animations are snapped
  in v1; see "Future work: gradient morph animation" above for the three
  upgrade paths when a workload demands lerping.
