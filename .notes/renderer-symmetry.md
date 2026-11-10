# Renderer: one geometry, one paint

A proposal for one piece of work. When it lands, each lasting decision folds into `AGENTS.md`
("Shapes and paint") and the module docs it names, and this file is deleted.

`AGENTS.md` already states the rule: "Coverage is geometry, paint a function the fragment calls. A
pipeline is one geometry and one paint." The public API and the WGSL template follow it. The layers
between them do not: a record, a payload, a composer packer, a wire instance and a pipeline key each
tell the same closed shape in a different way. A box and a triangle hold their paint, and an image
and an effect hold their shape.

## Status

| Batch | State |
| --- | --- |
| B0 Docs | next |
| B1 Geometry | done, `acd517c11` |
| B2 Records and payloads | done, `a6efe204a` |
| B10 Backdrop bound | next, a bug |
| B3 Packer | to do |
| B4 Turns | to do |
| B5 Wire | to do |
| B6 Keys | to do |
| B7 Effect forms | to do |
| B8a Material composite | to do, a bug |
| B8b One composite | waits for D1 |
| B9 Strokes | to do |

Each decision below starts with its state: **[done]**, **[part]** with what is left, or the batch
that does it.

## Issues this plan fixes

Every entry of `.notes/ISSUES.md` that touches the renderer, investigated, with the batch that
deletes it from the log.

- **A material's fill and border overcount the edge: B8a.** Confirmed in
  `effect/material.wgsl`. With `o` and `i` the outer and inner edge coverage, `palantir_composite`
  writes `(o − i)·Bα + o·Fα·(1 − (o − i)·Bα)`: the fill takes `o` and the border composites over it,
  two coverages of one edge multiplied. An opaque box under an opaque border at its outer edge,
  `o = 0.5`, `i = 0`, reads `0.5 + 0.5 · 0.5 = 0.75` where its coverage is 0.5.
  `palantir_composite_outside` does the same from the other side: at `o = 0.75`, `i = 0.25` it reads
  `0.5 + 0.75 · 0.5 = 0.875` where the covered area is `1 − i = 0.75`. The fix needs no decision:
  `AGENTS.md` already fixes a material's model, the fill over the border box, and the exact form of
  that model weights by area, the border over the fill in the band and the fill alone past it:
  `(o − i)·(B + F·(1 − Bα)) + i·F` inside, `(o − i)·(B + F·(1 − Bα)) + (1 − o)·F` outside. Test: the
  two functions' values at those coverages, hand-computed, in the effect tests, failing on the
  current form. Goldens of every material edge move.
- **`FillKind::FAST_BIT`'s doc claims a shader path: B6.** Confirmed: only the internals reference
  `quad_pipeline/reference.wgsl` reads `FILL_FLAG_FAST`; production reads the bit back on the CPU
  alone, in `ComposeSession::record_opaque_cover`, where `PackedQuad::is_pixel_aligned` gives the
  same answer. B6 deletes the bit, its doc and the reference shader's fast branch, which the
  reference, the SDF form the visual suite compares with, has no need of: the fast path is
  bit-identical to the SDF by its own comment. `record_opaque_cover` reads the alignment it already
  has.
- **`.notes/PERF.md` names `QuadForm::BackdropLens` and `BackdropSpectrum`: B0.** Confirmed, and
  worse than a stale name: the entry states a decision the code has reversed. Glass is the stock
  effect `effect/stock/glass.wgsl`, one program with its spectrum behind a branch, as the
  `gpu::backdrop` module doc says under "One glass", and its cost on V3D is not measured
  (`.notes/TODO.md`). Fix: the entry moves out of "Decisions" as superseded, its Pi numbers kept as
  the measurement of the form refused, and the open measurement goes under "Open".
- **The `gpu::backdrop` module doc states a material form the code left: B0.** Found on the way.
  Its "A material draws through its effect's material stages" says a backdrop effect compiles a
  second pair of stages for a material, lays a solid fill, and draws a gradient fill as its own
  quad above. The effect's one fragment stage branches on `PALANTIR_MATERIAL` (`effect/contract.rs`,
  `MATERIAL_BRANCH`), and `effect/material.wgsl` samples a gradient fill in the material's own
  draw, as `AGENTS.md` decides ("A material reaches under the border, in one draw", which refuses
  both the quad and a stage compiled apart). Fix: the bullet states the branch and the one draw,
  and points at `AGENTS.md` for the refusals.
- **`TreeSource::narrowest_side` measures a material box with its drop shadow's halo: B10.**
  Confirmed, and a bug past the measure: the backdrop's `rect` is its shape's paint rect, the box
  grown by its shadow's halo `h`, and the footprint is that rect grown by `LensOptics::margin` of
  its narrowest side. The margin falls one pixel for each pixel the narrowest side grows, while the
  bezel bounds the depth, so a halo that widens the side by `2h` takes `2h` off the margin and adds
  back only `h` of rect: the footprint ends up to `h` short of what the lens reads past the box, and
  the composite clamps those reads to the footprint's edge. By the margin's arithmetic, a bezel of
  4 px, a lean of 20 and a box 10 px across need 16 px past the box (`4 + 20 − 10 + 1`, and a pixel
  for the bilinear read); a halo of 4 makes the side 18, its margin 8, and the footprint reaches
  `4 + 8 = 12`. Not yet reproduced in a test; B10's test is that reproduction. Fix: a backdrop's rect is its shape's
  own covering rect, lifted to the screen, not its paint row's (`ClosedGeometry::local_bounds`,
  through the shape's lift and clip); the narrowest side and the footprint follow it. A shadow is
  no part of a backdrop. Test: a glass box narrow enough that the margin is under the reach, with a
  drop shadow, whose footprint holds the box's own margin, failing on the current rect.

## Findings

The tables and lists below state the code before B1.

The same closed shape, told three ways. Paths are from `src/`.

| Layer | Fill of a box or a triangle | Image | Effect, material |
| --- | --- | --- | --- |
| Authoring pair (`shape/draw.rs`) | `Draw::Box { geometry, paint }`, `Draw::Triangle { shape, paint }` | `Draw::ImagePaint { geometry, paint }` | `Draw::EffectPaint { geometry, paint }` |
| Record (`shape/record/mod.rs`) | `ShapeRecord::Quad(QuadShape::{Box, Triangle})`: geometry outside, paint fields inside | `ShapeRecord::ImagePaint { geometry: ClosedGeometry, … }`: paint outside, geometry inside | `ShapeRecord::EffectPaint { geometry: ClosedGeometry, … }` |
| Geometry type | `BoxGeometry` (has `coverage`), `TriangleGeometry` | `ClosedGeometry` (box has no `coverage`) | `ClosedGeometry` |
| Payload (`renderer/frontend/payload/`) | `DrawQuadPayload { geometry: QuadGeometry, fill, border, fill_axis }` | `DrawImagePayload { rect, geometry: Option<PlacedClosed>, … }` | `DrawEffectPayload { geometry: EffectGeometry, … }` |
| Placed shape | `QuadGeometry::{Rect, Triangle { a, b, c, … }}`, spin inside | `PlacedClosed::{Box, Triangle}`, spin beside | `EffectGeometry::{Box { coverage, placed }, Triangle}`, spin beside |
| Composer packer (`composer/`) | `PackedQuad::pack` | `TemplateShape::of` | `PackedQuad::pack` on a fake transparent quad (`DrawEffectPayload::quad`) |
| Wire instance (`render_buffer/`, `quad.rs`) | `Quad`: shape lanes mixed with paint lanes, radii f16 | `ImageInstance`: shape lanes at other offsets, radii f16 | `EffectInstance`: radii `[f32; 4]` |
| Shape kind on the wire | `FillKind::TRIANGLE_BIT`, `OUTSIDE_BIT`: geometry in the paint word | `ImageForm` column beside the row | `EffectInstance::flags`, read back by `corner_count()` |
| Pipeline key | `QuadForm`, a flattened shape × brush product; `QuadPipeline` derives `QuadDraw` from `fill_kind` at upload | `ImageForm`: shape only | `(EffectId, images, corner_count)`; outside is a runtime branch |
| Triangle's corner count in WGSL | `form_is_triangle()`, a pipeline constant | entry points `vs_box`, `vs_triangle` | `instance.flags`, a runtime value |
| Instance → `PalantirShape` in WGSL | `instance_shape` (`quad_pipeline/template.wgsl`) | `shaped_vertex` (`image_pipeline/shaped.wgsl`) | `palantir_vs` (`effect/wrapper.wgsl`) |
| Fill and border composite | `composite`, `composite_outside` (`gpu/pipeline/prelude.wgsl`): the fill stops at the border's inner edge | none | `palantir_composite`, `palantir_composite_outside` (`effect/material.wgsl`): the fill covers the border box, the border lies over it |

Smaller faults of the same origin:

- **[fixed in B2] A shadow is a `DrawQuadPayload` with a kind tag.** `FillKind::SHADOW_DROP` or `SHADOW_INSET`
  selects it, and `fill_axis` carries `(offset, σ, spread)` under a gradient's name. A border on a
  shadow and a shadow on a triangle are representable, and `QuadForm::of` panics on them.
- **[B6] `FillKind::FAST_BIT` reaches the wire, and no production shader reads it.** Only the internals
  reference (`quad_pipeline/reference.wgsl`) reads it. The CPU reads it back in
  `ComposeSession::record_opaque_cover`, where `PackedQuad::is_pixel_aligned` gives the same answer.
- **[open] A box's fill and border have two homes**: `DrawQuadPayload` without a material,
  `MaterialStage` inside `DrawEffectPayload` with one.
- **[B6] `QuadForm::BoxOutside` merges two brushes** and reads the tag at run time, while the inside
  forms pin it. No triangle form covers its outside, by contract, but no type says so.

### Turns

Every shape turns today, a triangle and an arc among them (`ui/tests/paint_animation.rs`): a paint
animation's `rotation` reaches each payload as a `Spin` about the owner's center, one meaning in
every family. It is held four ways:

| Payload | Where the spin is | Pivot frame | Composer's cull bound of a turned draw |
| --- | --- | --- | --- |
| quad (box, triangle, shadow), image, effect | a `spin` field beside the shape | the encoder's logical frame | `Lowering::turned_rect`: the turned rect's own bound, grown by the antialiasing reach |
| icon, text | a `spin` field | the logical frame | `Rect::turned_cover` about the pivot |
| mesh | a `spin` field | relative to the payload's `origin`, the owner's position | `Rect::spun_cover`: the square the bbox sweeps at every angle |
| curve, arc, polyline | inside `StrokeBounds::Spun`, with that square's radius | relative to `origin` | `StrokeBounds::cull_rect`, the same swept square |

The fade reaches every payload through the gate's parameter, `PaintSink::draw(payload, alpha)`, "so
an encoder arm cannot forget to fade". The turn has no such guard: each arm passes it by hand, as
`turned(spin)` on a quad and a field elsewhere. A node's chrome passes none, as it does not animate.

### Geometry types

A closed shape has a type at each stage, and a wrapper over the two kinds:

| Stage | Box | Triangle | Over both |
| --- | --- | --- | --- |
| Authored | `RectShape`, `CircleShape` | `TriangleShape` | — |
| Record, unplaced | `BoxGeometry` | `TriangleGeometry` | `ClosedGeometry`; `QuadShape` for a fill |
| Placed | `PlacedBox` | `PlacedTriangle` | `PlacedClosed` |
| Payload arguments | `RectQuad` | `TriangleQuad` | — |
| Encoder bundles | `RectPaint`, `BoxPaint` | — | — |
| Composer | `PackedQuad`, `TemplateShape` | `TemplateShape`, `GrownTriangle` | — |

A stroke has `ArcGeometry`, `BezierGeometry`, the polyline's fields inline in
`ShapeRecord::Polyline`, then `PlacedStroke`, `CurveBasis` and `StrokeBounds`. The authored types are
the public API and stay. The record and placed stages pass a box or a triangle alone nowhere but
into the wrapper, so each wrapper and its two structs are three types for one value. Where a shape
lays out is told one way already: `local_rect: Option<Rect>`, with a `Fit` beside it for a point
shape (`PointPlacement`).

## Pipeline families that cannot merge

A pipeline is a vertex stage, a fragment stage, a bind-group layout and a blend state. Two draws
need two pipelines when one of these differs and the difference is not a branch the project
measured as free.

### Geometry families: the vertex stage

| Family | Draws | Its own because | Merge refused |
| --- | --- | --- | --- |
| **Closed template** | a box, a circle, a triangle; inside or outside | per corner a ring, per edge a band, an interior fan (`primitives/wgsl/shape_template.wgsl`) | — this is the family every closed paint shares |
| **Rect** | a sharp rect, turned or not: a plain image, a `GpuView` | four vertices and no distance varyings | the template's 138 vertices and three varyings for a shape with no corner (`AGENTS.md`, "A plain image keeps its one quad") |
| **Blurred box** | a drop or inset shadow | a grid of cells around a hole, a closed-form Gaussian and the cutout tables (`quad_pipeline/shadow.wgsl`) | its coverage is no distance to an edge, so no template region holds it; Graphite also draws it as its own geometry, `kAnalyticRRectBlur` ([`Geometry.h`](https://skia.googlesource.com/skia/+/main/src/gpu/graphite/geom/Geometry.h)) |
| **Stroke** | a cubic, an arc, a polyline segment, a join | chord strips over a shared index buffer (`curve_pipeline/`) | a stroke is centered on a path, which no convex polygon describes |
| **Mesh** | app triangles | the app's vertex and index buffers, per-vertex color | the geometry is data, not a closed form |
| **Glyph atlas** | text runs, icons | whole-pixel quads into an atlas raster at device size (`raster_pipeline/`) | a glyph is rasterized on the CPU, not shaded |

The passes outside the main pass are their own pipelines for the same reason: the backdrop blur
(`gpu/backdrop/`), the cutout bake and the backbuffer blit.

### Paint families over a closed shape: the fragment stage and the bindings

| Family | Its own because |
| --- | --- |
| **Fill**, solid or gradient | binds only the frame group. Solid and gradient are separate pipelines: VideoCore holds 54 temporaries for every form and 22 for a solid (`AGENTS.md`, "Coverage is geometry") |
| **Image** | binds a texture group per run, and keys its runs by texture |
| **Effect**, a material among them | the app's own module, its image, backdrop and word groups, and the dual-source blend `One, Src1` (`effect_pipeline/mod.rs`, `BLEND`); a material is a runtime branch of a backdrop effect's pipeline, as decided ("a material stage compiled apart" refused) |

What can merge is not a pipeline. It is the geometry half that each closed family repeats: the
record type, the payload type, the packer, the wire layout and the WGSL reader. The proposal below
makes these one, and keeps the pipelines above.

## Decisions

- **[part: the wire and WGSL in B5] A draw is a geometry and a paint, geometry outside, at every layer.** A record, a payload and a
  row each hold a shape and an enum of the paints its family draws. As Skia Graphite records a draw
  as its geometry and its `PaintParams`, and makes the pipeline from the `RenderStep` the geometry
  picks and the paint's key
  ([`Renderer.h`](https://skia.googlesource.com/skia/+/main/src/gpu/graphite/Renderer.h)), and as
  WebRender draws a quad with a `Pattern`
  ([`pattern.rs`](https://github.com/servo/webrender/blob/main/webrender/src/pattern.rs)). Refused:
  the paint owning its shape, as Impeller's `Contents::GetGeometry` does
  ([`color_source_contents.h`](https://github.com/flutter/flutter/blob/master/engine/src/flutter/impeller/entity/contents/color_source_contents.h)),
  which `DrawImagePayload` and `DrawEffectPayload` follow now.
- **[done] The pair picks the renderer family, and the record keeps the pair.** An icon of a rect lowers
  to the glyph-atlas family; a box's shadow lowers to the blurred-box family under or over its
  fill. As Graphite's `RendererProvider` picks a `Renderer` from the geometry and the style.
  Refused: a record per renderer family, which puts the renderer's split in the authoring model.
- **[part: `ShapeHeader` and `PalantirShape` in B5] One closed-shape type per stage.** `ClosedGeometry` in the record, the box's side in it;
  `PlacedClosed` in the payload; `ShapeHeader` on the wire; `PalantirShape` in WGSL. A paint that
  draws no outside panics at `add_shape`, as an image's does now, and the record store asserts it.
  Named `PlacedClosed`, not `PlacedShape`: a shape is any geometry, a stroke's too (`AGENTS.md`, "A
  shape is geometry"). Refused: `QuadGeometry` and `EffectGeometry` beside it, and a box without its
  side in `ClosedGeometry`, routes to one shape. Landed in B1 over the variant structs
  (`BoxGeometry`, `PlacedBox`, …), which B2 inlines; the payload's paint word names the brush
  alone, and the composer writes the outside and triangle bits into the `Quad` from the shape.
- **[done] Records.** `ShapeRecord::Closed { geometry: ClosedGeometry, paint: ClosedPaint }`, with
  `ClosedPaint { Fill(LoweredPaint), Image { … }, Icon { … }, Effect { … } }`. An icon takes a sharp
  box that covers its inside, checked at `add_shape` and asserted at record. The authoring `Draw`
  takes the same shape: `Draw::Closed { geometry, paint: ClosedPaintRef }`. Landed in B2. Cost: the
  record grows from 136 to 144 B, the triangle's geometry beside a box's paint with its shadow.
- **[done] One payload for every closed paint, generic over the paint.** `DrawClosedPayload<P> {
  geometry: PlacedClosed, spin, paint: P }`, `P` one of `ClosedFill`, `ClosedImage` and
  `ClosedEffect`, each a `ClosedPaintPayload` that fades, says whether it paints over its shape and
  names its sink method; a material is a `ClosedEffect` whose `stage` is `Some`.
  `DrawShadowPayload { source: PlacedBox, spin, shadow }`, so a shadow on a triangle or with a
  border is no value. Text, icon, mesh and stroke keep a payload each. Landed in B2: a fill draw
  stays 84 B and an image draw 120 B. Refused: an enum of the paints in one payload, which would
  carry an effect's 150-odd bytes in every fill a frame draws; one payload of a geometry enum and a
  paint enum, whose product holds pairs no family draws.
- **[done] An image's own rect is part of its paint.** `ClosedImage { rect, uv_min, uv_size, tint,
  handle, flags, view }`: where the image lies inside its shape, past which it shows nothing, as
  CSS's `background-repeat: no-repeat` lays an image in its box. Every image draw carries its
  shape, and the composer draws one quad over the image's rect where the shape is a
  square-cornered box, which cuts nothing past it. Landed in B2.
- **[part: curve and polyline in B4] A spin belongs to the draw, never to its geometry.** It is a paint animation's turn of the
  whole draw, so every payload holds `spin: Option<Spin>` beside its shape, a curve's and a
  polyline's too. Refused: a spin inside `QuadGeometry`'s variants (gone in B1) or inside
  `StrokeBounds`.
- **[B4] A turn reaches every draw through the gate, as a fade does.** `PaintSink::draw(payload,
  PaintMod { alpha, spin })`, and `PaintPayload::turned` beside `faded`, so no encoder arm can
  forget to turn. Refused: a turn passed by hand in each arm.
- **[B4] One pivot frame, the encoder's logical frame.** `Spin::about_owner` loses its `origin`
  parameter; a payload with an `origin` (mesh, curve, polyline) subtracts it where it turns its own
  points. Refused: a pivot whose frame depends on the payload, two readings of one field.
- **[B4] The composer culls a turned draw by its bound at this frame's angle; the cascade keeps the
  swept one.** The cull asks whether the draw paints in this frame's damage, which the turned
  bound answers: a rect's turned bound, a curve's turned control points or arc, which the composer
  turns already. The cascade's paint rect must hold across an animation's frames, which the swept
  square (`spun_cover`) does. Refused: the swept square in the composer, as meshes and strokes use
  it now, which admits draws that paint nothing in the damage; `StrokeBounds`, a cull bound stored
  where the composer derives it from the geometry. Cost: the polyline keeps its record's `bbox` in
  its geometry, as a scan of its points is O(n).
- **[part: closed shapes done; strokes in B9, `PackedQuad` in B3] One enum per family and stage, over a struct per kind that owns its math.** Record:
  `ClosedGeometry { Box(BoxGeometry), Triangle(TriangleGeometry) }`, and `StrokeGeometry {
  Bezier(BezierGeometry), Arc(ArcGeometry), Polyline { … } }`. Placed: `PlacedClosed { Box {
  placed: PlacedBox, coverage }, Triangle(PlacedTriangle) }` and `PlacedStroke`. As `std`'s
  `IpAddr { V4(Ipv4Addr), V6(Ipv6Addr) }`: each kind's struct holds the methods of its kind, its
  placement, bounds and hash, and travels alone where one kind is meant, a stroke's dot as a
  `PlacedBox`, a triangle into the composer's `TemplateShape::triangle`. What goes are the bundles
  that repeat them: `QuadShape`, `RectQuad`, `TriangleQuad`, `RectPaint`, `BoxPaint` (gone in B2),
  `EffectGeometry`, `QuadGeometry` (gone in B1), `PackedQuad`'s second route, `StrokeBounds`.
  Refused, after B2 was planned on it: the variants inline, which turns each kind's methods into
  enum matches and a triangle's placement into a free function of four parameters, the struct
  again; one geometry enum for every family, whose pairs with paints no family draws would be
  representable.
- **[B3] One packer.** `TemplateShape::of(PlacedClosed, spin)` packs every closed draw, with one edge
  rounding rule; `PackedQuad` keeps the fill's own lanes. Refused: `DrawEffectPayload::quad`,
  which builds a transparent quad to reach a second packer.
- **[B5] One wire header.** `ShapeHeader { rect, points, radii: Corners, inradius, rotation, clip }`
  opens `Quad`, `ImageInstance` and `EffectInstance` at the same attribute locations, and one
  WGSL function reads it into a `PalantirShape`. The radii are f16 in all three, as the effect's
  `[f32; 4]` widens the same f16 values. Refused: three layouts and three readers that must
  agree by hand.
- **[B6, effects in B7] A pipeline key is a closed-shape form and a paint specialization.** `ShapeForm { Rect, Box,
  BoxOutside, Triangle }` is shared by every closed family and pinned as a constant in each:
  fill `(ShapeForm, Solid | Gradient)`, image `ShapeForm`, effect `(EffectId, ShapeForm)`. Each
  pipeline is built on the first frame that draws it, as a `ClipVariant` half is. Refused: the
  corner count as instance data, as an effect reads it now, which puts a runtime `% n` in the
  template's vertex decode where the constant folds it, and which no bench measured; the
  flattened `QuadForm`, where `BoxOutside` merges two brushes.
- **[B6] The key lives in a column beside the instance, set once by the composer.** Every family whose
  rows pick a pipeline stores `Soa<Row { key, instance }>`, as `ImageDrawRow::form` does now. A
  shadow's key says shadow, and the backend picks its entry from the cutout plan it alone holds,
  as `QuadDraws` does now. Refused: `QuadPipeline` deriving `QuadDraw` from `fill_kind` bits at upload, and
  `EffectInstance::corner_count` reading flags.
- **[part: the payload's word since B1; the `Quad`'s in B6] `FillKind` is a brush tag.** It keeps solid, linear, radial, conic, ramp and the spread. The
  outside and triangle bits move to `ShapeForm`; the shadow tags move to `QuadDraw::Shadow`;
  `FAST_BIT` goes, and `record_opaque_cover` reads `PackedQuad::is_pixel_aligned`. Refused: a
  paint word that carries geometry and a composer flag.
- **[part: kept; its named lanes in B6] Shadows stay in the quad stream.** Inside a group, the quad lane replays in record order, and
  a drop shadow paints under its box and an inset shadow over its fill. A stream of its own would
  move shadows to a tier, and each shadowed box would close a group. The shadow's lanes are named
  as a shadow's (`offset`, `σ`, `spread`), not borrowed from `fill_axis`.
- **[B9] One stroke payload.** `DrawStrokePayload { path: Cubic | Arc | Polyline, stroke }` feeds the one
  curve pipeline that draws all three, with `ShapeRecord::Stroke { path, stroke }` behind it.
  Refused: `DrawCurvePayload` and `DrawPolylinePayload`, and three stroke records, for one family.

## Plan

Each batch is a refactor that moves no golden, except B8a, B8b and B10, which fix what they say.
The visual suite and the composer's captured streams prove it.

0. [ ] **B0 Docs.** The `PERF.md` glass entry moves to superseded, and the `gpu::backdrop` doc's
   material bullet states the one stage's branch and the one draw.

1. [x] **B1 Geometry**, done. `ClosedGeometry` with coverage, `PlacedClosed` with coverage in
   every closed payload; spin out of `QuadGeometry`, which goes with `EffectGeometry`.
2. [x] **B2 Records and payloads**, done. `ShapeRecord::Closed`, `Draw::Closed`,
   `DrawClosedPayload<P>`, `DrawShadowPayload`; `QuadShape`, `RectQuad`, `TriangleQuad`,
   `RectPaint`, `BoxPaint`, `DrawQuadPayload`, `DrawImagePayload`, `DrawEffectPayload` and
   `DrawEffectPayload::quad` go. `PackedQuad` packs a closed shape and a shadow apart from their
   lanes, which a fill and a shadow each write.
3. [ ] **B3 Packer.** `TemplateShape::of` for every closed draw, a fill's and an effect's too;
   `PackedQuad` goes.
4. [ ] **B4 Turns.** `PaintMod` through the gate, one pivot frame, the turned cull bound in the
   composer; `StrokeBounds` goes.
5. [ ] **B5 Wire.** `ShapeHeader` in Rust and WGSL; one reader replaces `instance_shape`,
   `shaped_vertex` and the reader in `palantir_vs`.
6. [ ] **B6 Keys.** `ShapeForm`, `FillKind` as a brush tag, the key column for quads, `FAST_BIT` out.
7. [ ] **B7 Effect forms.** Effect pipelines per `ShapeForm`; `EffectInstance::TRIANGLE` and
   `OUTSIDE` go.
8. [ ] **B8a Material composite.** `palantir_composite` and `palantir_composite_outside` take the
   exact border-box form, with a test of their values; no decision waits.
   [ ] **B8b One composite**, after decision D1: the quad's `composite` and `composite_outside`
   take the model D1 picks, and one copy serves both families.
9. [ ] **B9 Strokes.** `StrokeGeometry`, `ShapeRecord::Stroke`, `DrawStrokePayload`; `ArcGeometry`
   and `BezierGeometry` go.
10. [ ] **B10 Backdrop bound.** A backdrop's rect, narrowest side and footprint from its shape's
    covering rect, its shadow left out, with a test; independent of the rest, and a bug, so early.

B6 and B7 change pipeline counts: a frame with a triangle effect builds a second pipeline of that
effect. Bench `atomic/main_pass/gpu/*` and `integration/frame/gpu/*` against the parent before
each lands.

## Open decisions

- **D1: one fill-and-border composite.** A quad's fill stops at the border's inner edge, and the
  two coverages sum. A material's fill covers the border box, and the border lies over it. A
  translucent border shows what is behind the box on a plain box, and the fill on a material box,
  so one `Paint` paints two ways. The material's form is also inexact at its edges, which B8a fixes
  first ("Issues this plan fixes"). Pick:
  CSS's border box (`background-clip: border-box`, the CSS default), weighted by area, `(o − i) ·
  (B + F · (1 − Bα)) + i · F` with `o` and `i` the outer and inner coverage and `B`, `F`
  premultiplied: the border over the fill in the band, the fill alone inside it, exact, and free of
  the seam `composite`'s comment names. Cost: every golden of a plain box with a translucent
  border moves. Refused: the quad's model for a material, which `AGENTS.md` refused for the
  sharp content it shows through the border; the material's form as it is.
- **D2: one ordered lane of runs for every family.** The quad lane replays in record order and
  every other family in a fixed tier order after it (`PaintTier`), closing a group where order
  would break. WebRender batches every kind in one list with a look-back. This removes the
  quad-first asymmetry and the tier conflict test, but it is structural and no workload asks for
  it. Gate: draw count and main-pass time on the showcase pages
  (`.notes/research/2026-11-10-draw-calls.md`).
- **D3: the rect form for a sharp fill.** A sharp unturned solid box draws the template's 138
  vertices where an image draws four. Gate: the main pass on V3D, where the vertex floor is
  4.8 ms (`.notes/PERF.md`, "The board's vertex floor").

## TODO items this answers

- "quad and triangle — different pipelines? unify?": one geometry family, a `ShapeForm` each.
- "Draw::Effect — do we need it if we have box?": yes, as a paint of a closed shape, not a shape.
