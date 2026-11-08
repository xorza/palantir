# Proposal: app-written effects and the power-saver preference

Apps write their own WGSL effects: backgrounds, press ripples, rings past a widget's edge, water
over content. A new system preference follows the OS power-saver mode, so an app can swap to
cheaper effects. Facts and sources: `.notes/research/2026-11-08-custom-shaders.md` and the
"Power saving" section of `.notes/research/2026-10-31-system-preferences.md`.

When this lands, each lasting decision moves into the doc comment of the module it governs, or
into `AGENTS.md` where it binds the whole crate, and this file is deleted. Every item adds public
API, so each batch also runs `cargo drive surface`.

## Power-saver preference

Landed with batches 1 to 3. The decisions are in `AGENTS.md` ("Reduced power follows the OS
power-saver mode", "Reduced power starts a frame at most at 30 Hz"), the docs of
`Ui::is_power_reduced` and `FrameRuntime::take_power_slot`, and each `PowerSaverWatch` in
`host/winit/platform/`.

## Effects

### Contract

Landed with batch 5: the contract, its blends and palantir's stages are in the docs of
`EffectSource` and `effect_pipeline`, and `effect/contract.wgsl` and `effect/wrapper.wgsl`.

- **Two kinds, each with its own signature.** A color effect reads its position, size, corners,
  time, parameters and images. A backdrop effect also reads the pixels painted below it, through
  the capture `BackdropFilter` uses. Deferred: effects that read their own subtree (SwiftUI
  `layerEffect`), which need an offscreen pass per node; stateful effects, which `GpuPaint`
  already serves.

### Identity and parameters

Landed with batch 4: the decisions are in the docs of `EffectName`, `EffectParams`,
`EffectSource`, `PowerSaverPaint`, `EffectError` and `Ui::load_effect`. The contract is a fixed
`fn effect(input: EffectInput, params: Params) -> EffectOutput`, the prelude after the app's text.

- **A name nothing loaded is a logic error.** A shape that names one panics with the name.
  Refused: painting nothing, which is quietly wrong; `FontFamily`'s fallback, which has no
  equivalent for an effect.

### Paint

Landed with batch 5 for color effects: `EffectShape`'s doc holds the shape and its clips, and
`WgpuEffectStore`'s the compile at load.

- **A backdrop effect states how far it reads, per shape**, as SwiftUI's `maxSampleOffset`. A read
  past it clamps to the captured area. Refused: one bound per effect, which the largest instance
  sets for all; the whole surface (Flutter without a `ClipRect`).
- **One backdrop path, `BackdropFilter` on it too.** A backdrop effect is a pane of the existing
  machinery: the copy out of the main pass at a barrier, the panes that share a break, the
  footprint and damage ledger and the result atlas. Frost, glass and spectrum become palantir's
  own backdrop effects on the effect pipeline, in place of the three quad forms (`Backdrop`,
  `BackdropLens`, `BackdropSpectrum`); `BackdropFilter` stays a property of a rect and lowers to
  them. The blur stays a pre-pass a backdrop effect asks for by σ, 0 for the raw capture, and the
  pane's border and gradient tint draw as a quad above it, as a gradient tint does now. Each
  built-in keeps its own pipeline, as the forms kept V3D's registers apart, and the dispersion
  tables reach the spectrum effect through an internal extension of the contract. The rewrite is
  measured against the benches behind `gpu/backdrop`'s decisions before the forms go. Refused:
  the capture shared and the composites apart, two composite paths to keep.
- **Up to 4 image slots**, declared at load and bound per shape. A draw splits where instances
  bind other images. Refused: a bindless array, which GL and older Metal tiers lack.

### Time

Landed with batch 6: the decisions are in the docs of `shape::paint_animation`, `PaintRepeat::Held`
and `EffectShape`. An effect's time is a `PaintAnimation`'s, through `Ui::add_shape_animated` and
`PaintAnimation::clock`, so it shares its sampling, wakes, damage and reduced-motion rule.

### Power saver and themes

The power-saver alternative landed with batch 6: the docs of `PowerSaverPaint` and
`EffectRegistry::resolve` hold it.

- **A clickable theme has a press-effect slot**, as Compose's theme supplies its `Indication`.
  The slot names a color effect over palantir's own `PressInput` (the press point, the release
  time, the rect) with the theme's color and enter and exit times. Widget core keeps at most 10
  live presses a widget, Android's `MAX_RIPPLES`, and a press past it starts none.
- **A theme file names an effect by its string.** Loading the theme refuses a name that nothing
  loaded. Two different effects do not blend in a theme transition, so the slot snaps.
- **Palantir ships two ripples** written in the public contract: Material's flat ripple
  (Compose `RippleAnimation`) and Android's sparkle ripple (`RippleShader`). The stock themes
  leave the slot empty, so a stock press looks as it does now.

## Batches

Each batch ends green on the gate, `cargo visual` where it paints, and `cargo drive surface`.

### 1. The power-saver preference — done

`UiPreferences::power`, resolved once at each frame's start. A move records every window again.

### 2. The 30 Hz limit — done

`FrameReport::earliest_repaint_at`, a slot on a grid of whole refresh periods; the winit host and
the harness hold every frame to it.

### 3. The winit host reads the mode — done

A `PowerSaverWatch` in each `host/winit/platform/` file, folded with `mundy`'s report by
`SystemFollow`.

### 4. The effect registry — done

`EffectRegistry` in the shared resources, `#[derive(EffectParams)]` in `palantir-derive` (renamed
from `palantir-anim-derive`), and `naga` 30 as a direct dependency. Pipelines compile in batch 5.

### 5. Color effects paint — done

`DUAL_SOURCE_BLENDING` is required (lavapipe, NVIDIA and V3D offer it); `Shape::effect` lowers
through `PaintTier::Effect`; `WgpuEffectStore` builds each effect at load, for each format and
device. The blends and the clip are held by exact-pixel GPU tests in place of goldens.

### 6. Effect motion — done

The effect's time rides `PaintAnimation` (`PaintAnimation::clock`, `PaintRepeat::Held` for a
reduced single pass), sampled in exact nanoseconds; the power-saver alternative resolves as a
shape is added.

### 7. Image slots

- Up to 4 slots per effect, bound per shape, a split where the images differ.
- Tests: the binding, the split, an `ImageHandle` that is not resident after device loss.

### 8. Backdrop effects, `BackdropFilter` on them

- The backdrop signature with the σ it asks for, `Shape::backdrop_effect` and its read bound, as
  panes of the existing capture, break sharing, ledger and atlas; the capture grown by the bound,
  damage when content under the grown rect changes, the clamp past the bound.
- Frost, glass and spectrum as built-in backdrop effects, the dispersion tables through an
  internal extension of the contract; `BackdropFilter` lowers to them, its border and gradient
  tint as a quad above; the three backdrop quad forms deleted.
- Bench the new composite against the old on the `gpu/backdrop` benches (the handheld, the Apple
  laptop, the AMD iGPU, V3D) before the forms go, and fold the result into `gpu/backdrop`'s
  decisions.
- Tests: the captured rect, damage from content under the bound, a golden of a displacement; the
  existing backdrop goldens unchanged, or each moved pixel explained.

### 9. Press effects

- `PressInput`, the theme slot with its color and times, the press list in widget core, stock
  flat and sparkle ripples, theme-file names and their check on load.
- Tests: the press list's bound, the release time, reduced motion and the power-saver swap.

### 10. Showcase, docs, cleanup

- A showcase page with a background, both ripples, a ring and water; a `cargo shot` script.
- Fold each decision into its module doc or `AGENTS.md`; delete this file.

## Open

- **Where a theme's names are checked.** `Theme` deserializes with no `Ui`, so the check runs where
  a theme is set or loaded; batch 9 picks the call and its error.
- **Reduced transparency and backdrop effects.** A distortion adds no transparency, so whether
  the preference applies to it stays open until batch 8.
