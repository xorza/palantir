# Proposal: a turn paint animation on every shape kind (2026-10-28)

Covers the TODO item "turn paint animation on every shape kind". Facts are in
`.notes/research/2026-10-28-paint-rotation.md`.

# Part 1: Decisions

## The contract

- **A turn rotates any shape about its owner box's center, at paint time
  only.** The channel, pivot and sampling stay as they are; layout, hit test
  and the cascade transform never see the angle. Refused: a rotation in
  `TranslateScale`, whose axis-aligned rects the scissor and hit-test math
  rely on.
- **Every kind is bounded by its swept square.** Cascade damage and composer
  culling take `Rect::spun_cover` of the kind's existing bound, as strokes do
  today, so no angle reaches either and they agree at every frame. Refused:
  the rotated rect's tight box, which moves each frame and splits damage
  from cull.
- **The encoder hands every payload a `Spin`, not only the strokes.**
  `StrokeBounds`' spin moves to a field the quad, image, mesh and raster
  payloads share, so each composer arm reads one optional spin.

## Per kind

- **Mesh: a complex scale in the instance.** `MeshInstance.scale: f32`
  becomes `rotor_scale: Vec2` (`scale·cos`, `scale·sin`), and the vertex
  shader multiplies each owner-local position by it, about the pivot folded
  into `translate`. Four bytes an instance; the vertices stay shared and
  untouched.
- **Triangle: turned points.** The composer turns the three owner-local
  points about the pivot before it packs them, as it turns polyline points;
  the corner radius does not change under a rotation, and the shader is
  untouched.
- **Rect, shadow and a curve's dot: an angle in the quad.** `Quad` gains
  `rotation: f32` (60 → 64 B), an angle about its own center; the composer
  moves the center about the pivot. The vertex shader turns the shaded
  bounds and each shadow-grid vertex about the center; the fragment shader
  turns the pixel back into the quad's frame, where the SDF, the gradient
  axis and the cutout tables already work. A rotation keeps distances, so
  the AA width holds in local space, as WebRender extrudes in local space. A
  turned quad snaps to no pixel, occludes nothing in `OcclusionPruner`, and
  enters overlap tests by its swept square.
- **Image: an angle, and edge AA.** `ImageInstance` gains `rotation: f32`;
  the vertex shader turns the corners about the center and the uvs follow.
  A turned image fades its edges over `AA_HALF_WIDTH` by local distance,
  which an axis-aligned image does not need.
- **Text and icons: a turned raster variant.** A turned run or icon emits
  `SpunRasterQuad` (the `RasterQuad` fields with an `f32` center and the
  angle) into its own batch, drawn by a `vs_spun` entry that turns the quad
  and reads through the bilinear `resampled` path with a local-distance
  edge fade. Glyph centers are turned about the pivot on emit; a turned run
  takes the encoded-cache template and drops its pixel snap.

## Refused

- Tessellating quads and text into meshes, as egui turns text: loses the
  quad SDF's AA and the instanced batching.
- Drawing a turned shape into an offscreen target and compositing it turned:
  a pass and a texture per animated shape.
- Widening `RasterQuad` for every glyph (28 → 40 B) for a case that is rare.

# Part 2: Plan

Each batch leaves `cargo drive gate` and the golden suite green; the panic
narrows as kinds land, and `ShapeRecord::rotates` lists the kinds done.

## Batch 1: The shared spin and the bound

- [ ] Move `Spin` out of `StrokeBounds` into a field every payload carries;
  `plane_ctx.rs` builds it once from `paint_mod.rotation`.
- [ ] `cascade/paint_rect.rs`: `spun_if_animated` covers every arm (text's
  inked rect, a quad's `bbox_local` with its halo, the image and icon fit
  rects, the mesh paint bbox).
- [ ] Tests: each kind's damage under a turn is its swept square, and no
  pixel outside it changes over a turn.

## Batch 2: Mesh and triangle

- [ ] `MeshInstance.rotor_scale`; the mesh shader's complex multiply.
- [ ] Triangle points turned in the composer.
- [ ] `ShapeRecord::rotates` adds both; composer unit tests for the turned
  instance values; goldens at 30°.

## Batch 3: Rect, shadow and the curve's dot

- [ ] `Quad.rotation`; `QUAD_INSTANCE_ATTRS`, the pinned size, the shader's
  vertex and fragment turns, the shadow grid.
- [ ] No snap and no occlusion for a turned quad; overlap by its swept
  square.
- [ ] `shape/curve.rs`: the dot's doc no longer says it does not turn.
- [ ] Goldens: a rounded bordered rect, a gradient rect, a drop and an inset
  shadow, a square dot, each at 30°.

## Batch 4: Image

- [ ] `ImageInstance.rotation`; corner turn and edge fade in the image
  shader.
- [ ] Goldens: a fitted and a tiled image at 30°.

## Batch 5: Text and icons

- [ ] `SpunRasterQuad`, its vertex layout and `vs_spun`; a turned run's own
  batch; glyph centers turned on emit.
- [ ] Icons through the same variant.
- [ ] Goldens: a label and a color icon at 30°; a test that an unturned run
  still emits `RasterQuad` and hits the encoded cache.

## Batch 6: The public surface

- [ ] Delete `turn_unimplemented` and `ShapeRecord::rotates`, the
  `add_shape_animated` `# Panics` and the "polylines, curves and arcs only"
  line on `PaintChannel::turn`.
- [ ] A showcase page turns one shape of each kind; regenerate the API
  surface; delete the TODO item and this proposal, folding the lasting
  decisions into the `paint_animation` module doc.
