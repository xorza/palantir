# Brushes: gradients & image fills

Today every fill and stroke is a single `Color`. This doc proposes a `Brush`
surface that adds linear / radial / conic gradients and image fills, the GPU
encoding to support them, and an incremental rollout that does not regress
the steady-state alloc-free contract.

## Today

- `Background { fill: Color, stroke: Stroke, radius: Corners }`
  in `src/primitives/background.rs:25`. Stored as `Tree.chrome:
  SparseColumn<Background>` (`src/forest/tree/mod.rs:108`), filtered out when
  `is_noop()` so transparent panels stay zero-cost.
- `Stroke { color: Color, width: f32 }` (`src/primitives/stroke.rs:18`),
  `#[repr(C)] Pod` — flows straight onto the quad instance.
- `Shape::RoundedRect { fill, stroke, radius, local_rect }`
  (`src/shape.rs:10`). `Shape::Line { color, width }` exists but the encoder
  drops it (`src/renderer/frontend/encoder/mod.rs:127`).
- Encoder emits `DrawRect` / `DrawRectStroked`
  (`src/renderer/frontend/cmd_buffer/mod.rs:43`), composer lowers to a 68 B
  `Quad { rect, fill, radius, stroke }` (`src/renderer/quad.rs:15`), shader
  `src/renderer/backend/quad.wgsl` does the SDF rounded-rect with one or two
  AA bands.
- The only texture in flight is glyphon's text atlas
  (`src/renderer/backend/text.rs:76`). No user-image path.
- `Background` participates in the per-node hash
  (`src/forest/tree/mod.rs:194`), so cache eviction + damage are already
  wired.

## Goals

- One `Brush` enum that fills and strokes both consume — no separate
  `FillBrush` / `StrokeBrush` types.
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

pub struct GradientId(u32);   // index into Ui::brushes.gradients

pub struct LinearGradient {
    pub angle: Radians,             // 0 = →, π/2 = ↓
    pub stops: SmallVec<[Stop; 4]>, // ≤16 enforced at construction
    pub spread: Spread,
    pub interp: Interp,             // Srgb | Linear | Oklab | Oklch{hue}
}

pub struct RadialGradient {
    pub center: Vec2,               // object-space, 0..1
    pub radius: Vec2,               // object-space, 0..1
    pub stops: SmallVec<[Stop; 4]>,
    pub spread: Spread,
    pub interp: Interp,
}

pub struct ConicGradient {
    pub center: Vec2,
    pub start_angle: Radians,
    pub stops: SmallVec<[Stop; 4]>,
    pub spread: Spread,             // pad/reflect/repeat
    pub interp: Interp,
}

pub struct Stop { pub offset: f32, pub color: Color }

pub enum Spread { Pad, Repeat, Reflect }

pub struct ImageBrush {
    pub image: ImageId,
    pub uv: Rect,                   // sub-rect of source image, 0..1
    pub fit: ImageFit,              // Stretch | Tile | Fit | Cover
}
```

`Background` and `Shape::RoundedRect` carry `Brush` instead of `Color`. The
hot solid path is `Brush::Solid(Color)`, which the composer fast-paths to
the existing 68 B quad with `brush_kind = SOLID`.

`Stroke` becomes `Stroke { brush: Brush, width: f32 }` for symmetry.

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
    pub kind: u32,          // 0=solid, 1=linear, 2=radial, 3=conic, 4=image
    pub spread: u32,        // 0=pad, 1=repeat, 2=reflect (n/a for solid/image)
    pub solid_or_origin: [f32; 4], // solid: rgba; gradient: (cx,cy,rx,ry)/angle; image: uv_min
    pub extra: [f32; 4],    // gradient: (lut_row, lut_v, stop_count, _);
                            // image: (uv_max.x, uv_max.y, atlas_page, fit)
}
```

`BrushSlot` is the GPU wire encoding of a user-facing `Brush`. The composer
builds it; nothing outside `renderer/` constructs one. It is not exposed to
widget code, has no `From<Brush>` impl (the conversion needs `&mut
GradientLutAtlas` / `&mut ImageAtlas` to allocate slots), and is
`pub(crate)` only so the backend can bind-group it.

Net: 16 + 16 + 4 + 24 + 24 = 84 B per quad (was 68). Acceptable — measure
in `benches/` before merging.

### Gradient LUT atlas

`GradientLutAtlas` (`src/renderer/backend/gradient_lut.rs`):

- Single `Rgba8Unorm` texture, 256 px wide, N rows tall (start at 256 rows,
  grow on demand). Each row is one baked gradient. The shader samples with
  `linear` filter; spread mode is folded into `t` in shader before the
  sample.
- Key: `(stops_hash, interp, gradient_axis_kind)`. `gradient_axis_kind`
  separates linear/radial/conic only when their stop processing differs
  (e.g. premultiplied gamma path). LRU eviction; cap at 256 rows initially.
- Bake on CPU: convert each stop to OKLab (or selected interp space),
  interpolate per-texel, premultiply alpha, write to staging row, upload via
  `queue.write_texture` (single row, never re-create).
- The atlas lives on the renderer side; the brush surface only stores a
  `GradientId` that the encoder resolves to `(lut_row, stop_count)` while
  building the cmd buffer.

### Image atlas

`ImageAtlas` (`src/renderer/backend/image_atlas.rs`):

- `Vec<AtlasPage>` of `Rgba8UnormSrgb` 4096² pages. Shelf packer for
  uploads; sort draws by page so each page = one bind group.
- `ImageId` is opaque to the user (`Ui::upload_image(rgba, size)`),
  internally `(page: u16, rect: URect)`.
- Sub-page LRU is out of scope for v1 — once uploaded, an image lives until
  the `Ui` is reset or `Ui::release_image(id)` is called.

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
(84 B `Quad`) is bound only when at least one slot in the batch is
non-solid. This keeps the steady state of solid-color UIs at byte-parity
with pre-brush builds. The two GPU structs sit side by side in
`renderer/quad.rs`:

```rust
#[repr(C)] #[derive(Pod, Zeroable)]
pub(crate) struct SolidQuad { rect: Rect, fill: Color, radius: Corners,
                              stroke_color: Color, stroke_width: f32 }   // 68 B

// `Quad` (84 B) above — the brush-pipeline instance.
```

Naming convention: `SolidQuad` for the legacy-bytes layout, `Quad` for the
brush-capable layout. No "Legacy" prefix anywhere — both are first-class
and both stay in the codebase indefinitely.

## Cache, hashing, damage

- `Brush` is `Hash + Eq`; gradient/image IDs hash by their content key, not
  their slot index, so a recycled slot doesn't collide with a previous
  brush.
- `Background::hash` already runs inside the per-node hash
  (`src/forest/tree/mod.rs:194`); replacing `Color` with `Brush` means
  changing a stop or swapping an image invalidates the encode/compose/measure
  caches automatically. No new wiring.
- The gradient LUT and image atlas are **renderer-side**; their lifetime is
  decoupled from `WidgetId`. The encode cache holds onto `(lut_row,
  atlas_rect)` as part of the cached `RenderCmd` slice; if the atlas
  reuploads to the same row, the slice stays valid.
- `Damage::Partial` paths still work because the brush eval is per-fragment;
  any rect the damage region covers re-runs the same shader against the
  current LUT/atlas state.

## Allocation discipline

- `Ui::brushes: BrushArena` retains `Vec<LinearGradient>` /
  `Vec<RadialGradient>` / `Vec<ConicGradient>`, cleared per frame via
  `truncate(0)` (capacity preserved).
- `BrushSlot` is `Pod` and lives inline in `Quad`; no per-frame heap.
- Stop interpolation buffers (256 × `Rgba8`) live in a renderer-side scratch
  `[u8; 1024]`, not allocated each bake.
- Image uploads are explicit and one-shot — never a per-frame surface.

## Testing

- `src/primitives/brush.rs` unit tests for hash stability and `Brush::Solid`
  byte-for-byte equality with the legacy `Color` slot.
- A `src/renderer/backend/gradient_lut/tests.rs` golden test bakes a known
  3-stop gradient and pixel-compares the LUT row.
- A pinning test in `lib.rs` (`brush_solid_path_unchanged_quad_bytes`) that
  records a `Background { fill: Brush::Solid(red) }` panel and asserts the
  emitted `Quad` matches the legacy bytes for the same panel pre-migration.
- A showcase tab `Brushes` exercising linear, radial, conic, image, and
  stroke-with-gradient. CLAUDE.md's UI rule applies — start the showcase and
  eyeball it before declaring done.
- `benches/quad_throughput.rs` (new) measuring 10k mixed-brush quads per
  frame; flag a regression if solid-only path slows.

## Rollout

Five slices, each shippable on its own.

1. **Brush type + Solid migration.** Introduce `Brush` enum with only the
   `Solid(Color)` variant. Replace `Color` on `Background`, `Stroke`,
   `Shape::RoundedRect`, `Shape::Line`. Keep `BrushSlot` size at the legacy
   16 B path — same `Quad` bytes, same shader. Pin with the byte-equality
   test. **Risk: rename churn touches every widget.** Acceptable per the
   project's "break things freely" posture.

2. **Linear gradient.** Add `LinearGradient` variant, `BrushArena`,
   `GradientLutAtlas`, expanded `Quad`/`BrushSlot`, branched shader.
   Showcase tab gets a linear-gradient row. Spread modes from the start —
   they're three lines of WGSL each.

3. **Radial + conic.** Same atlas, new `kind` arms in the shader, conic AA
   via analytic angular derivative. Pin a conic seam test.

4. **Image fills.** `ImageAtlas`, `Ui::upload_image`, `Brush::Image`. Sort
   draws by atlas page in the composer; one bind group per page. Start with
   `Stretch` and `Tile`; `Fit` / `Cover` are CPU-side `uv` adjustments on
   top of `Stretch`.

5. **Polish.** OKLab interpolation default (matches CSS Color 4), gradient
   along stroke if any workload demands it, `release_image` + atlas
   defragmentation if memory pressure shows in benches.

Each slice lands with `cargo fmt && cargo clippy && cargo test` clean and
the showcase tab updated.

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
- Renderer-vs-record separation: should `GradientId` allocation live in
  `Ui::brushes` (per-frame, simple) or in a renderer-side cache that
  survives across frames? Per-frame is easier and the LUT cache already
  amortizes the real cost. Pick per-frame for v1; revisit if profiling
  shows bake overhead.
