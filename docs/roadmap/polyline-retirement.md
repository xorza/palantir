# Polyline retirement

Retire the CPU stroke-tessellation path (`Shape::Line`,
`Shape::Polyline`, `stroke_tessellate::tessellate_polyline_aa`) in
favor of a single GPU pipeline shared with native bezier curves.

Status: **Not started.** Curves landed first (see `ShapeRecord::Curve`
+ `curve_pipeline.rs`); this is the natural follow-up that lets us
delete `stroke_tessellate` and the `polyline_points` /
`polyline_colors` per-frame arenas.

## Why

The curve pipeline already does on the GPU exactly what
`stroke_tessellate` does on the CPU: take centerline geometry +
width, emit a fringe-AA stroked strip, batch one draw call per
scissor group. Lines + polylines are special cases of curves —
the line `(a, b)` is a degenerate cubic
`(a, a + (b-a)/3, b - (b-a)/3, b)`; a polyline is N consecutive
degenerate cubics with joins.

Carrying both code paths means:

- Two stroke-AA implementations (CPU `tessellate_polyline_aa` +
  GPU `curve.wgsl`) that have to stay visually consistent — drift
  shows up as the polyline_round_caps and curve_caps goldens
  disagreeing on cap pixels at the same width.
- `stroke_tessellate` runs at compose time and writes into the
  shared mesh arena. Steady-state, this is the only mandatory CPU
  work between encode and draw for stroke-heavy frames.
- The mesh arena's per-frame growth is dominated by polyline
  tessellation (~10× vertex count vs. user-supplied meshes in the
  showcase). Retiring polylines moves all stroke vertices to a
  per-instance GPU buffer that's ~12× smaller (one 60-byte
  instance vs. ~10 × `MeshVertex`).

## Migration plan

Ship in three slices. Each slice keeps both paths working until
the cutover; no compatibility shims at the API surface.

### Slice 1 — line / polyline → curve, no joins

`Shape::Line { a, b, .. }` lowers to a degenerate cubic
`(a, lerp(a, b, 1/3), lerp(a, b, 2/3), b)`. Caps from the
existing `cap` field thread directly into `ShapeRecord::Curve.cap`.
Smallest change; closes ~60% of `stroke_tessellate` call sites
(every `Shape::Line` site + every 2-point `Shape::Polyline`).

Pin: `line_diagonal_aa` golden must hold within the existing
tolerance, otherwise the GPU stroke's fringe shape doesn't
match the CPU tessellator's. Likely needs `Tolerance` widening
(per-channel `2` → `4`) — the SDF and the fringe-vertex AA round
the half-px ramp slightly differently.

### Slice 2 — joins in the curve shader

Add `LineJoin` to `ShapeRecord::Curve`-equivalent paths so an
N-segment polyline lowers to N sub-curves sharing join state.
Two options:

- **Per-segment cubic with explicit join geometry.** The
  composer emits the join as either a Bevel triangle (1 quad
  added to the curve batch with `cap=Bevel` semantics) or a
  Round arc (SDF in fragment, same shape as Round cap). Miter
  needs the miter-limit fallback the polyline path already
  pins. This keeps the curve pipeline single-purpose and lets
  the join math live alongside the cap math in `curve.wgsl`.
- **Move to a generalized "stroked path" pipeline.** Replace
  `CurveInstance` with a `StrokeSegmentInstance` that carries
  control points + per-end join state (prev tangent for the
  leading end, next tangent for the trailing end). One pipeline
  serves curves, lines, and polylines; join state is computed
  CPU-side from neighboring segments.

Pick the second if a future Path primitive looks likely; first
if curves stay the only "long" stroked primitive. Decide before
writing the join SDF — the data layout difference is load-bearing.

Pin: `polyline_bevel_join`, `polyline_round_join`,
`polyline_round_caps` goldens. Bevel + Round both have
analytical answers; the GPU result has to match the CPU one
within the same tolerance as the cap-shape goldens.

### Slice 3 — delete the CPU path

Once every `Shape::Line` / `Shape::Polyline` lowering routes
through the curve pipeline:

1. Delete `src/renderer/stroke_tessellate/` (module + tests +
   bench).
2. Delete `ShapeRecord::Polyline` and the `DrawPolyline` cmd kind.
3. Drop `polyline_points` / `polyline_colors` from
   `FrameArenaInner`.
4. Drop the `polyline_scratch` vec from `Composer`.
5. Drop the `MeshBatch` overlap-tracking path that exists solely
   for polyline-tessellated meshes (curves track their own
   `above_text_rects`).

Net: ~600 LoC removed, one fewer per-frame arena, one fewer
compose-time mutable borrow on `FrameArenaInner`.

## Trade-offs

**Pro.**

- Single source of truth for stroke AA.
- Steady-state CPU stroke work goes to ~0 for the common case
  (text + rounded rects + lines).
- Curve shader already handles the harder case (non-linear
  tangent); polyline joins are strictly easier.
- Per-instance GPU storage is more memory-efficient than the
  expanded mesh-vertex form (~12× compression on the showcase's
  line-heavy tabs).

**Con.**

- Higher GPU work per pixel: the curve pipeline subdivides each
  sub-instance into 16 quads = 96 vertex invocations. For a
  short straight line that's ~10× more vertex work than the
  6-vertex polyline AA strip. Probably noise on any UI workload
  but worth measuring on the `stroke_tessellate` bench before
  the cutover.
- The CPU tessellator is currently the only piece of the
  pipeline that handles per-segment color (`PolylineColors::PerSegment`).
  GPU equivalent needs per-instance start/end color lanes; cheap
  but a real API decision.
- `LineCap::Round` joins under miter-limit fallback have to
  match the CPU bevel exactly, or graph-editing tools (the
  primary polyline consumer in scenarium) will show a one-frame
  visual flip at the cutover.

## Pre-work

Before starting Slice 1, run `cargo test --release stroke_tessellate
-- --ignored --nocapture` and save the baseline to
`benches/stroke_tessellate/baseline-pre-retirement.txt`. The bench
already exists and measures the CPU path under realistic node-graph
densities; the GPU equivalent needs a fixture rendering the same
geometry through the curve pipeline so we can compare end-to-end
frame time on the same workload.

## Open questions

- **Per-segment color.** Add it to `CurveInstance` (8 B more per
  instance) or split into a separate pipeline? The current curve
  pipeline ships solid color only; per-segment is a polyline
  feature with no curve consumer.
- **Round-join miter-limit semantics.** The CPU path bevels at
  `MITER_LIMIT = 4.0`; the GPU SDF needs to replicate that exact
  cutoff or the goldens flip. Pin the value at the WGSL level
  (matches the const in `stroke_tessellate`) before the shader
  starts handling joins.
- **`MultiDrawIndirect` for join-heavy frames.** Each join would
  emit one extra "join sub-instance" with a different cap_kind.
  At ~1000 polyline segments per frame that's still one draw call
  per group via instancing; no MDI needed for v1.
