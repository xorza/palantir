# One effect engine for materials and effect paints

A material and a backdrop `EffectPaint` reach the same GPU stage by two record, cascade, payload
and composer paths. This plan keeps the two public forms and gives them one engine underneath, so a
material is to a backdrop effect what `PressEffect` is to a color effect: theme data that lowers
through the effect path.

## Decisions

- **Two front doors, one engine.** `Material` stays the theme form of a backdrop effect: plain
  data a theme file saves and a transition blends, over `MaterialInput`, with palantir's stage
  (fill, border, luminance bounds, dither). `EffectPaint` stays the runtime form of any effect:
  any parameters, images, a clock. Below them, one lowered effect, one capture, one payload and
  one composer path. As Skia's `SkRuntimeEffect`, Android's `RuntimeShader` and Flutter's
  `FragmentShader` are one program whose placement decides its use, and as SwiftUI and WinUI keep a
  theme material (`Material`, `AcrylicBrush`) beside their custom shader route. Refused: one public
  backdrop type with any parameters (Flutter's `BackdropFilter`, CSS `backdrop-filter`), which puts
  type-erased bytes in a `Chrome` no theme file holds and no tween blends, as the `Material` doc
  refuses; a closed set of material looks (SwiftUI, WinUI), which drops app materials.
- **The public API does not change.** `cargo drive surface` shows no difference at the end.
- **Pixels do not change.** Every golden renders byte-identical to its render before step 1 on
  the same adapter, the stale ones included: compare `actual.png` before and after, as the review
  of `ce6a6616f` did.
- **The hot structs keep their pins**: `ShapeRecord` 136 B, `ChromeRow` 104 B. A step that would
  grow one packs the shared part instead, or stops and reports.

## Steps

Each step lands on its own, with the gate green, and its own commit.

### 1. One backdrop read

`MaterialRead` (`Capture(EffectId) | Ground(EffectId)`) and `EffectBackdrop`
(`Capture(EffectCapture) | Ground`) both answer what a backdrop effect reads under the
reductions, and `MaterialRead::resolve` and `RecordStore::record_effect` each decide
`ReducedBackdrop::Nothing` and `Ground` for themselves.

- Done: one `BackdropMode` (`Capture | Ground`) and one resolve, `ReducedBackdrop::mode`, which a
  material's `MaterialRead` (now its effect and its mode) and `record_effect` both take. The
  effect record still carries its capture in `EffectBackdrop::Capture`, as the material keeps its
  σ and lens apart: step 2 gives both one capture and one `EffectBackdrop`.
- Tests: a table over the choice × the setting in `reduced_backdrop.rs`; the material's and the
  backdrop effect's reduced-transparency tests pass unchanged.

### 2. One capture

`BackdropReader::{Material, Effect}` exists because the two keep their capture apart: a material's
σ as f16 bits and its lens, from which the cascade derives how far past the box it reads; an
effect's σ and reach as f32.

- One capture type: the σ, and what sets the reach, a fixed reach in logical px or a lens whose
  margin the cascade derives from the box's narrowest side. `BackdropReader` collapses to it, and
  `PlaneCtx`'s cascade arm reads one type for a chrome and a shape.
- Packed so `LoweredMaterial` stays 40 B. If the effect's σ and reach move to f16 lanes, as the
  material's σ already is, the step measures that no golden moves and notes the rounding (a
  quarter px below 1024 px) on `EffectPaint::blur` and `EffectPaint::reach`.
- Tests: the existing backdrop plan tests (`cascade`, `backdrop_captures`) with exact footprints for
  a material and an effect of the same σ and reach, now one path.

### 3. One payload and one composer path

`DrawBackdropPayload` (a material: geometry with coverage, fill, border, material) and
`DrawEffectPayload` (an effect: placed geometry, params, images, time) both become one
`EffectDrawRow`, through `ComposeSession::backdrop` and `ComposeSession::effect`, which repeat the
listing of the backdrop, the ordering and the instance.

- One payload: the geometry (a box with its coverage, or a triangle), the effect, its params and
  images, its capture draw, its opacity, time and turn, and the material's stage as an optional
  part (fill, fill axis, border, luminance bounds). `PaintSink::backdrop` goes; `paint_backdrop`
  and the effect arm of `PlaneCtx` both build the one payload.
- One packing: the box through `PackedQuad`, which already holds coverage and border width, the
  triangle through `TemplateShape::triangle`, as both paths do now.
- What the composite reads: a material reads its whole footprint, an effect its rect grown by its
  reach and clamped to the footprint. The step makes them one rule only if no golden moves; if one
  does, it keeps the two as data of the one payload and reports why.
- Tests: the composer's backdrop and effect tests (`tests/backdrops.rs`, `tests/brushes.rs`)
  through the one payload, with the same exact instances; one new case shows a material and an
  app backdrop effect over the same rect differ only in the stage flag and the look.

### 4. Docs

- AGENTS.md: one decision under "Shapes and paint" for the three layers (an effect is a program;
  `Material` and `PressEffect` are theme data that name one; `EffectPaint` is the one draw), which
  replaces the parts of "Blur, frosted and glass are stock effects" and "A material reaches under
  the border" it repeats.
- The `Material` and `EffectPaint` docs each say what the other is for, in one sentence.
- This plan is deleted when step 3 lands.

## Gate at the end

- The palantir gate.
- `cargo visual`: the same pass and fail set as before step 1, every `actual.png` byte-identical.
- `cargo drive surface`: no change to `.notes/API_SURFACE.md`.
- `hot_struct_sizes` holds `ShapeRecord` 136 B and `ChromeRow` 104 B.
