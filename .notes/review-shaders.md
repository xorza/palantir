# Review: shaders and shader-related code

Scope: every `.wgsl` under `src/gpu`, the prelude and `ShaderBody` assembly,
the pipeline modules that build and feed them, and the wire types they read
(`Quad`, `FillKind`, `FillAxis`, `CurveInstance`, `ImageInstance`,
`RasterQuad`).

Whoever addresses an item deletes it.

## Wire layouts: the shaders restate numbers that Rust says it owns

- [ ] Wire tags have three different shapes. `FillKind` is a
  `repr(transparent)` newtype with named constructors. The curve basis is
  loose `u32` constants (`src/renderer/render_buffer/curve.rs:20-31`,
  `CurveInstance::kind: u32`). Image flags are loose `u32` bits
  (`src/renderer/render_buffer/image.rs:111-124`, `ImageInstance::flags:
  u32`). `CurveInstance::cap_lanes(start: u32, end: u32)` (`curve.rs:126`)
  takes raw discriminants, not `LineCap`. Target: a `repr(transparent)`
  newtype for each, after the `FillKind` model (`CurveKind`, `ImageFlags`,
  a cap-pair type built from two `LineCap`s).
- [ ] The cap lane can hold a state that the shader draws incorrectly. It
  holds a cap for each end (`curve.rs:98-106`), but the shader makes one
  `FLAG_ROUND_CAP` if either end is Round (`curve_pipeline/shader.wgsl:293-295`)
  and rounds every `cap_t > 0` zone (`:406`). Start Round with end Square
  gives two round caps. The composer emits only `(user cap, Butt)` pairs
  today (`composer/session.rs:770-771`), so no frame shows it. Target: either
  a round flag for each end, or a lane that models the real data (one cap,
  plus which ends get it).

## Shader-module construction is repeated at each pipeline

- [ ] `quad_pipeline/mod.rs:201-225`, `curve_pipeline/mod.rs:78-94`,
  `image_pipeline/mod.rs:57-67`, `mesh_pipeline/mod.rs:65-68`,
  `blit_pipeline/mod.rs:28-31`, `raster_quad.rs:86-99`. Each one calls
  `ShaderBody::X.specialize(&[..])` and then writes the same
  `create_shader_module` descriptor with its own label. `ShaderBody` already
  names each file. Its constants and its label are facts about the same
  body, but they are kept in six other files. Target:
  `ShaderBody::module(self, device) -> wgpu::ShaderModule`, with the label
  and the constant list in the `ShaderBody` match. Then a test can specialize
  every body completely without a device, and a missing marker fails at
  test time and not at pipeline build.

## Docs that contradict the code

- [ ] Conic direction. `src/primitives/paint/brush/gradient/conic_geometry.rs:14`
  (public API) and `quad_pipeline/shader.wgsl:279` say "counter-clockwise".
  `atan2` in y-down pixel space increases clockwise on screen, as
  `CurveInstance` says correctly (`curve.rs:57`). Target: "clockwise" in
  both places.
- [ ] `quad_pipeline/shader.wgsl:1-5` and `curve_pipeline/shader.wgsl:54-57`
  say that the gradient LUT stores straight alpha. The bake stores
  premultiplied texels (`renderer/gradient_atlas/bake.rs:3, 35`), and
  `eval_fill` unpremultiplies them (`:298-302`).
- [ ] The drop-shadow lanes. `FillKind::SHADOW_DROP`
  (`fill_kind.rs:91-97`) and `FillAxis` (`fill_axis.rs:8-9`) say that the
  lanes are `(0, 0, σ, spread)` and that the shader runs `shadow_coverage`.
  The composer sends `(offset.x, offset.y, σ, spread)`, the shader reads the
  offset from `.xy` (`quad_pipeline/shader.wgsl:386`), and no
  `shadow_coverage` exists.
- [ ] `src/renderer/quad.rs:23` and `fill_kind.rs:60` say `fill: RgbaF32`.
  The field is `RgbaF16`.
- [ ] `src/renderer/render_buffer/curve.rs:8-16, 18-19` and
  `curve_pipeline/shader.wgsl:35-38` say "lockstep" and "bump together" for
  `SEGMENTS_PER_INSTANCE` and the `KIND_*` tags. Both are substituted, so
  nothing must be changed by hand to match.
- [ ] `curve_pipeline/shader.wgsl:146` says `flags` holds a "join metric in
  bits 4..6". The join look is now two independent flags
  (`FLAG_JOIN_BEVEL`, `FLAG_JOIN_MITER`).
- [ ] `src/gpu/pipeline/curve_pipeline/mod.rs:1-2` lists cubics and arcs
  only. The pipeline also draws polyline segments and join chrome.

## Comments that tell history (coding guide)

- [ ] These comments tell what the code did before, not why it is as it is
  now. Keep the "why" and remove the history:
  `quad_pipeline/shader.wgsl:246` ("byte-identical to the pre-brush
  behaviour"), `:207-214` ("the original `sign(d.y)`…"),
  `quad_pipeline/mod.rs:203-206` ("only happened to equal the tag"),
  `curve_pipeline/shader.wgsl:181-184` ("the standalone `cubic` /
  `cubic_tangent` pair recomputed them"), `:302-305` ("With 0 at the body
  edge the zero landed…"), `prelude.wgsl:60` ("Sampling at `u = t` instead
  read…"), `blit_pipeline/shader.wgsl:30-31` ("The visual goldens caught
  it…").
