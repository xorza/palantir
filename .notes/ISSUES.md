# Open issues

- **A material's fill and border overcount the outer edge.** `palantir_composite` and
  `palantir_composite_outside` (`src/effect/material.wgsl`) give the fill `outer_aa` and composite
  the border over it, so a pixel half inside an opaque material box with an opaque border reads
  alpha 0.75 where its coverage is 0.5.
- **`FillKind::FAST_BIT`'s doc says the shader skips the SDF and composite path**
  (`src/primitives/packed/fill_kind.rs`). No production shader reads `FILL_FLAG_FAST`; only the
  internals reference `quad_pipeline/reference.wgsl` does.
- **`.notes/PERF.md` "Decisions" names `QuadForm::BackdropLens` and `BackdropSpectrum`**, which do
  not exist: glass is an effect program now.
- **`TreeSource::narrowest_side` measures a material box with its drop shadow's halo**
  (`src/cascade/engine.rs`): the backdrop's screen rect and the lifted bound it compares are both
  its paint rect, which `LoweredPaint::bbox_local` grows by the shadow, so a shadowed glass box's
  narrowest side is its shadow's.
- **The `gpu::backdrop` module doc's "A material draws through its effect's material stages"
  states a form the code left** (`src/gpu/backdrop/mod.rs`): it says a backdrop effect compiles a
  second pair of stages for a material, lays a solid fill, and draws a gradient fill as its own
  quad above; the effect's one fragment stage branches on `PALANTIR_MATERIAL`
  (`effect/contract.rs`, `MATERIAL_BRANCH`), and `effect/material.wgsl` samples a gradient fill in
  the material's own draw.
