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

Backdrop effects landed with batch 8: `EffectShape::reach`'s doc holds the read bound, and
`gpu::backdrop`'s "The pane is palantir's own backdrop effect" the one backdrop path.

Image slots landed with batch 7: the docs of `EffectSource`, `EffectShape::images`,
`TextureBinding::images` and `EffectImageGroups` hold them.

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

### 7. Image slots — done

Up to 4 slots, declared at load and bound per shape through one group of all its images, kept
by the image store for each set drawn; an app's own binding is refused.

### 8. Backdrop effects, `BackdropFilter` on them — done but the benches

`Shape::backdrop_effect` and the built-in panes on the effect pipeline, the three quad forms
deleted, every golden unchanged. Left: the A/B on the `gpu/backdrop` benches (the handheld, the
Apple laptop, the AMD iGPU, V3D), which wait, as the owner decided on 2026-11-08.

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

