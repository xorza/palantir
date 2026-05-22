# Depth prepass for overdraw reduction

GPU-side Z-only prepass that writes a conservative depth value for
every opaque occluder, so the main color pass can early-Z-kill
fragments that would otherwise run an SDF / gradient / shadow shader
just to lose the blend.

Status: **design only**. Nothing landed. Treat the cost/benefit
section as the gate — ship only if a real workload moves on it.

## Why it could help here

The fragment shaders in `backend/quad.wgsl` are not cheap:

- Every solid rect runs `sdf_rounded_rect` (length + min/max) and
  the AA `clamp(0.5 - d, 0, 1)` band.
- Gradients add an LUT sample (`textureSample`) + `apply_spread`.
- Shadows add `erf_approx` (a 5-term polynomial + `exp`) per fragment.
- Stroke path doubles the SDF eval (`inner_d = d + stroke_width`).

These run **even when the pixel is fully covered by an opaque quad
drawn later**. Today the only mitigation is the composer's
`prune_occluded_quads` CPU sweep
(`renderer/frontend/composer/mod.rs:243`), which removes quads
*fully* contained inside a later opaque quad. Partial overlap — the
common case for dense card / panel UIs — gets nothing.

A GPU depth prepass kills those partially-overlapped fragments before
the expensive fragment shader runs. The shape of the win:

- Best case: dense overlapping opaque panels with heavy fragment
  shaders (gradients, shadows). Several layers stacked, each running
  the full SDF.
- Worst case: simple flat UIs with little overlap — prepass adds
  upload + draws and removes no fragment work. **Net regression.**

So the feature must be either always-on-and-measurably-wins or
gated. Measurement-first.

## Best practices survey

- **Z-prepass for transparency-heavy 2D is unusual.** Most 2D GPU
  UI engines (Skia, Iced, Quirky, egui) draw back-to-front with
  alpha blending and rely on coarse occlusion / scissor culling
  rather than depth. Vello / piet-gpu side-step it entirely with
  compute path rendering. The closest precedent in the references
  is Slint's GPU backend which uses a Z buffer purely for in-pass
  state ordering, not occlusion (no prepass).
- **3D engine intuition transfers partially.** A prepass works when
  (a) opaque coverage is high relative to total pixels, (b) the main
  shader is meaningfully more expensive than the depth-only shader.
  Modern tile-based GPUs (Apple, ARM Mali, Adreno) get the most
  benefit because they HiZ-cull at tile granularity before the
  fragment shader dispatches.
- **Conservative bounds are mandatory.** Any AA / SDF edge that
  produces sub-opaque alpha must be excluded from the depth write or
  the prepass will punch holes through a final pixel that should
  have been blended. The standard trick is to write depth for an
  **inset** cover rect that is fully inside the opaque interior of
  the shape.

## What we already have

Composer already computes the conservative inset cover for opaque
solid quads — exactly the geometry a prepass needs:

- `Composer.opaque_in_group: Vec<Occluder>` with a `cover: Rect`
  per entry (`renderer/frontend/composer/mod.rs:97`).
- Sharp-cornered: `cover == Quad.rect`. Then deflated 1px to dodge
  the AA fringe — currently the composer's prune *does* the
  containment test against the un-deflated rect because the CPU
  prune only fires for full containment, where 1px doesn't matter.
  The GPU prepass needs an extra 1px deflate (or accept that the
  outer AA fringe of an underlying translucent quad gets
  depth-killed — visible only on translucent-over-opaque seams).
- Rounded: `cover = rect.deflated(per-side, max(adjacent_radii) *
  (1 - 1/√2))` — already the inscribed-square offset.

That `Occluder` list **is** the depth prepass instance buffer.

## Design

### Attachment

- Today: lazy `Stencil8` only when a frame uses rounded clip
  (`backend/mod.rs:299` `ensure_stencil`).
- Promote the lazy `Stencil8` attachment to `Depth24PlusStencil8`
  (combined ds is the only path wgpu offers without a feature flag;
  `Depth32FloatStencil8` is gated, `Depth16Unorm` is standalone-only
  and can't share with stencil). Depth precision is overkill at any
  width — the field is effectively a draw-order index over O(10²)
  layers, six orders of magnitude under 24-bit headroom.
- **Keep the laziness.** Allocate the depth-stencil only when the
  frame has at least one occluder cover to write (`!depth_covers.is_empty()`
  is the analogue of today's `has_rounded_clip`). The flat-UI control
  bench — no rounded clips, no occluders — must not regress on per-frame
  attachment clear / view binding / pipeline state cost, and the cleanest
  way to guarantee that is to keep the no-depth code path live. The
  cost gate (§ Cost / benefit) re-verifies this, but the lazy default
  removes the risk by construction.
- Memory when allocated: 4 B/pixel × 4K (3840×2160) ≈ 33 MB. Acceptable;
  comparable to the backbuffer color.

### Z assignment

- Composer emits opaque covers in record order (back-to-front).
  Assign `z = (N - 1 - i) / N` so front-most draws get smallest z.
- This lets the **prepass itself** benefit from HiZ: draw the
  prepass front-to-back so each rect rejects pixels already claimed
  by a closer rect.
- The main color pass runs in normal back-to-front record order
  with `LessEqual` test — opaque covers exactly match the prepass z,
  underlying fragments fail.

### Prepass pipeline

- New `DepthPrepassPipeline` next to `QuadPipeline`. Shares the
  viewport bind group, no color attachment write (pipeline desc:
  `targets: &[]`, `fragment: None` — depth-only pipelines skip the
  fragment stage entirely on most backends).
- Vertex shader: position + per-instance `(rect, z)` packed as
  `vec4`. Emit `clip.z = z`. No fragment shader.
- Instance buffer: shared with the composer's existing opaque
  cover stream.
- **Draw shape: per-group, not one-instanced-draw-per-pass.** Each
  group has its own `scissor` (and live `rounded_clip` ancestor),
  and the cover must respect both — see "Cover correctness" below.
  Easiest correct emission is one instanced prepass draw per group,
  bracketed by the same `SetScissor` the main pass would issue.
  Per-group prepass is still cheap (1 draw, no FS) but matters for
  the cost gate: it's not literally free against existing work.
- Stencil: prepass keeps `stencil_compare = Always, pass_op = Keep`.
  Cover rects are constrained at compose-time (see below) to lie
  inside every active ancestor rounded clip, so they don't need the
  stencil-mask check either.

### Main pass changes

- Every color pipeline (`quad`, `mesh`, `image`, `curve`, glyphon
  text) gets `depth_compare: LessEqual`, `depth_write_enabled:
  false`.
- Opaque quads in the main pass *also* run their normal AA SDF
  shader against the conservative cover — the cover is strictly
  inside the painted rect, so the AA fringe still draws (its z is
  > prepass z, so `LessEqual` passes? **No** — fringe pixels are
  outside the cover rect, so prepass never wrote depth there. Test
  passes against the cleared `1.0`).
- Translucent draws (shadows, gradients with sub-opaque stops,
  text) likewise test against the depth buffer and skip if a closer
  opaque is already there. **This is the actual overdraw win.**

### Stencil interaction

- Rounded-clip stencil still uses the same DS attachment. Stencil
  ops as today; depth ops as above. No conflict — depth and
  stencil are independent slots in the same `DepthStencilState`.
- `StoreOp::Discard` for depth at pass end (we never re-read across
  passes on a single frame, just within a pass).
- Per-pass `LoadOp::Clear(1.0)` for depth, same lifecycle as the
  existing stencil clear. Partial damage passes already do one
  clear per pass — fine.

### Damage / partial passes

- Each damage rect is a fresh `LoadOp::Clear` on depth. Prepass and
  main pass run inside the same `begin_render_pass`, so the depth
  clear is once per pass, not once per rect.
- Wait — the existing model is **one main pass spanning all damage
  rects**, with per-rect scissor inside the pass. Stencil is
  cleared once at pass open. Same for depth: one clear at pass open,
  per-rect scissor restricts both prepass and main draws to the
  rect. Disjoint rects → no cross-rect contamination.
- Schedule changes: emit a `DepthPrepass { range }` step before the
  first `Quads` step of each damage rect (or once, at pass start,
  for Full). New `RenderStep::DepthPrepass` variant.

### Composer changes

- The composer already maintains the cover list per group; promote
  it to a **frame-global** list keyed by `(group_idx, record_order)`.
  Each `RenderBuffer` grows a `depth_covers: Vec<DepthCover>` field
  (`{rect, z, group}`) so the schedule can emit one prepass draw per
  group under that group's scissor.
- Composer's existing **full-containment** prune
  (`prune_occluded_quads`) stays — it's strictly better than GPU
  depth prepass when applicable (no draw at all, vs. fragment
  killed but vertex work done).
- The cover stream produced for prepass is *not* deflated by the
  AA-fringe 1px today. Add per-side `EPS_AA = 1.0` deflate in the
  cover computation, applied only when emitting depth covers (the
  composer's containment test keeps its current bounds because it
  checks for full containment, where the fringe is also contained).

### Cover correctness across nested clips

The within-group `prune_occluded_quads` is geometric only: writing a
cover into a parent rounded clip's corner-cutout is harmless because
the under-quad it would occlude is inside the same clipped region too.
A cross-group depth prepass **loses that property** — a depth value
written in a pixel the ancestor stencil would actually mask out will
later punch a hole through a translucent draw that *should* have
painted there. So the cover emitted for the prepass must be
constrained to pixels the panel genuinely owns:

```
depth_cover = inscribed_for_corners(quad.rect, quad.corners)
              ∩ ancestor_scissor
              ∩ inscribed_for_corners(ancestor_clip.rect,
                                       ancestor_clip.corners)   // for each
                                                                // rounded
                                                                // ancestor
              .deflated(EPS_AA)
```

The intersections collapse cheaply at the `DrawRect` site: the
composer already maintains `clip_stack: Vec<ClipFrame>` with each
frame's `scissor` and optional `rounded` — fold the inscribed rect of
every rounded frame into the cover at push time, then `min`/`max` with
the active scissor. If the result is paint-empty, drop the occluder
(no prepass entry, prune still wins within-group via the un-narrowed
`cover` it keeps for its own contains test).

This is the load-bearing correctness step the rest of the design
hangs on; don't skip it because "current prune doesn't need it."

## Cost / benefit gate

Land this only if a benchmark moves. Procedure:

1. Build a "deep overlap" showcase scene: 8–12 layers of opaque
   gradient-filled panels with sub-1px overlap fringes, plus
   shadow drop on each, sized to fill the viewport. The
   `frame_bench` resizing arm + a new bench scene mirroring real
   scenarium node-graph density.
2. Capture baseline with `WinitHostConfig::collect_gpu_stats = true`,
   reading `GpuPassStats::last_pipeline_stats` (FS invocations) and
   `last_kind_ms(BatchKind::Quads)`.
3. Implement Phase 1 (prepass without main-pass depth test) and
   measure FS invocations alone — the prepass is free-add, no
   correctness risk if the main pass ignores depth.
4. Enable Phase 2 (main-pass `LessEqual`) only if Phase 1 shows
   ≥30% FS-invocation reduction on the bench scene.
5. Reject if `last_pass_ms` regresses on the flat-UI control
   bench (`alloc_free` baseline).

If the win is <10% on real scenarium frames after Phase 2: revert
and document. This is **classic premature optimization territory**
for the current workload — most palantir frames have low overdraw
because UIs are mostly text + thin chrome over a single big
background.

## Implementation slices

Ship in order. Each slice ends with `cargo test` green + bench
delta recorded.

1. **Depth attachment plumbing.** Promote the lazy `Stencil8` to
   `Depth24PlusStencil8`. Rename `ensure_stencil` → `ensure_ds`;
   keep the lazy trigger (allocate when the frame has either rounded
   clips *or* depth covers). All existing color pipelines pick up
   `depth_compare: Always, depth_write: false` so behavior is
   byte-identical on rounded-clip frames; flat-UI frames stay on
   the no-attachment path. **Measurable check before slice 2:** flat-UI
   control bench (`alloc_free`) within noise of pre-change baseline.
2. **Prepass pipeline.** New `DepthPrepassPipeline`, vertex-only,
   instanced. Schedule emits `DepthPrepass { range }` at pass
   start (Full) or per-rect (Partial). Depth covers default to
   empty — no FS-invocation change yet.
3. **Composer cover export.** Pipe `opaque_in_group` covers into
   `RenderBuffer.depth_covers`, intersected at the `DrawRect` site
   with every rounded-ancestor inscribed rect + active scissor (see
   "Cover correctness across nested clips") and deflated 1px for AA.
   Front-to-back sort within group, z assigned. Prepass now writes
   meaningful depth, but main pass still uses `compare: Always` —
   still no pixel change. **Pinning test:** translucent quad over a
   sharp-cornered child filling a rounded parent — must still blend
   in the parent's corner-cutout region (regression catch for the
   cover-correctness rule).
4. **Main-pass `LessEqual`.** Flip `depth_compare` on all color
   pipelines to `LessEqual`. **Real measurement point.** Bench
   the worst-case scene + control scene.
5. **Gate or land.** Either ship (and document) or revert slices
   3–4 leaving the plumbing for a later workload.

## Open questions

- **Pipeline count explosion.** Every color pipeline already has a
  Plain + Stencil-test variant. Adding "depth-test" doubles that
  again unless we make depth always-tested (`LessEqual` with z=1
  clear means everything passes when there's no prepass write).
  Always-tested is correct and avoids the variant split — verify
  it actually has no perf cost in the no-prepass case (clear→1.0,
  every test passes, depth-write off → behavior identical to no
  depth attachment? Drivers vary; needs benching).
- **Combined Depth+Stencil format on backends.** Verify
  `Depth24PlusStencil8` is supported on all targets palantir cares
  about (notably WebGPU + Vulkan + Metal). `Depth32FloatStencil8`
  needs a feature flag in wgpu.
- **MSAA.** Currently no MSAA. If we add it later, depth attachment
  must match sample count. Just a note — not blocking.
- **Stencil-Discard interaction.** Today rounded-clip-discarding
  fragments skip stencil writes via `fs_mask`. With depth, the
  mask-write pipeline must also skip depth-writes (it doesn't have
  meaningful z anyway — set `depth_write_enabled: false` on the
  mask-write pipeline).
- **Is the CPU occlusion prune redundant once we have depth?**
  Probably keep both. CPU prune skips vertex+upload+draw entirely
  for full containment; depth prepass needs the vertex to write
  the depth. Composing both is strictly better than either alone.

## Files touched (forecast)

- `backend/stencil.rs` → `backend/depth_stencil.rs` (rename, add
  depth state helpers).
- `backend/mod.rs` — `Backbuffer.stencil` → `ds`, `ensure_ds`,
  `run_main_pass` attachment construction.
- `backend/quad_pipeline.rs`, `mesh_pipeline.rs`,
  `image_pipeline.rs`, `curve_pipeline.rs` — `depth_compare:
  LessEqual` on all variants, drop the `None`-DS pipeline variants
  if always-on lands.
- `backend/depth_prepass_pipeline.rs` — new.
- `backend/schedule.rs` — `RenderStep::DepthPrepass`.
- `renderer/render_buffer.rs` — `depth_covers: Vec<DepthCover>`.
- `renderer/frontend/composer/mod.rs` — emit covers into
  `depth_covers`, front-to-back sort, z assignment, 1px AA deflate.
- `text_backend/` (inlined glyphon) — depth state on its pipeline
  variants, parallel to the existing stencil split.
- New benches in `benches/` for the deep-overlap scene.
