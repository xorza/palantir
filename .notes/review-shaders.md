# Review: shaders and shader-related code

Scope: every `.wgsl` under `src/gpu`, the prelude and `ShaderBody` assembly,
the pipeline modules that build and feed them, and the wire types they read
(`Quad`, `FillKind`, `FillAxis`, `CurveInstance`, `ImageInstance`,
`RasterQuad`).

Whoever addresses an item deletes it.

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
