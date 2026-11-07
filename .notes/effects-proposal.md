# Proposal: app-written effects and the power-saver preference

Apps write their own WGSL effects: backgrounds, press ripples, rings past a widget's edge, water
over content. A new system preference follows the OS power-saver mode, so an app can swap to
cheaper effects. Facts and sources: `.notes/research/2026-11-08-custom-shaders.md` and the
"Power saving" section of `.notes/research/2026-10-31-system-preferences.md`.

When this lands, each lasting decision moves into the doc comment of the module it governs, or
into `AGENTS.md` where it binds the whole crate, and this file is deleted. Every item adds public
API, so each batch also runs `cargo drive surface`.

## Power-saver preference

The preference itself landed with batch 1: its decisions are in `AGENTS.md` ("Reduced power
follows the OS power-saver mode") and in the doc of `Ui::is_power_reduced`.

- **Every frame runs at most at 30 Hz while power is reduced**, input frames included, as
  WebKit limits its rendering update in Low Power Mode (WebKit bug 215745). The interval is the
  smallest whole number of refresh periods at or above 1/30 s, so each frame lands on a vsync:
  every 4th at 120 Hz, every 5th at 144 Hz. Refused: decorative motion alone, which saves
  nothing on a frame that input asks for; no limit, which saves nothing for an app with no
  fallbacks.
- **The winit host reads the mode with its own platform code**, beside `mundy`, which reads no
  power state. Linux reads the portal over `zbus` and reads the first value itself (GLib #2481
  missed it); macOS reads `NSProcessInfo` through `objc2-foundation`; Windows reads
  `GUID_ENERGY_SAVER_STATUS`, and `SYSTEM_POWER_STATUS::SystemStatusFlag` before 24H2.
  Windows' standard and high-savings levels both read `Reduced`.
  `zbus` 5 and `objc2-foundation` 0.3 become direct dependencies, both in `Cargo.lock` already.
  Refused: waiting for `mundy` upstream; `ashpd`, a new package for one boolean.

## Effects

### Contract

- **An app writes one WGSL fragment function.** Palantir owns the vertex stage, SDF coverage,
  AA, rounded clips, `frag_origin` and the output, so an effect stays correct under partial
  repaint and clips. As SwiftUI's stitchable functions, Skia's SkSL and Bevy's `UiMaterial` with
  its default vertex shader. Refused: a whole vertex and fragment module (Qt `ShaderEffect`,
  Godot), which leaves each app shader to keep those rules; vertex distortion goes with it.
- **Two kinds, each with its own signature.** A color effect reads its position, size, corners,
  time, parameters and images. A backdrop effect also reads the pixels painted below it, through
  the capture `BackdropFilter` uses. Deferred: effects that read their own subtree (SwiftUI
  `layerEffect`), which need an offscreen pass per node; stateful effects, which `GpuPaint`
  already serves.
- **Plain WGSL behind palantir's prelude.** Palantir puts its prelude and the effect contract in
  front of the app's text. An app that wants modules runs WESL or naga_oil itself, as both emit
  WGSL. Refused: WESL or naga_oil inside palantir, a public dependency every app pays for.
- **The output is a color and a weight for what is under it.** The pipeline blends
  `src·One + dst·Src1` (dual-source blending), so one pipeline gives over, add, multiply and
  screen, and the prelude names each. Coverage applies exactly, as the blend is linear in it.
  A blend that is not linear in the destination is a backdrop effect. Refused: a pipeline per
  blend mode (Godot render modes); source-over alone, which sends multiply and screen through a
  copy. wgpu 30 has no framebuffer fetch (wgpu#10148).

### Identity and parameters

- **The name is the identity.** `EffectName<U, K = Color>` holds its name inline, at most 63
  bytes, and a phantom parameter type `U` and kind `K`, as `FontFamily` holds a name. It is
  `Copy`, made by a `const fn`, and serializes as its name. Refused: an RAII handle beside a
  string id, two routes to one effect.
- **One method loads, replaces and reloads.** `Ui::load_effect(name, source)` validates the WGSL
  with `wgpu::naga`, checks its parameter struct against `U`, and compiles. The same name again
  replaces the effect; on an error the old one keeps painting and the error returns. An effect
  stays loaded for the app's life, as a font does.
- **Parameters are a `Pod` struct**, checked against the WGSL at load and by rustc at each
  shape. They go into a per-instance storage buffer, so the instances of one effect share a draw.
  At most 256 B an instance. Refused: values by name (Qt, Skia), a lookup and a check per value
  per frame; a float array by index (Flutter, cranpose), with no check.
- **A name nothing loaded is a logic error.** A shape that names one panics with the name.
  Refused: painting nothing, which is quietly wrong; `FontFamily`'s fallback, which has no
  equivalent for an effect.
- **Errors are cases.** `EffectError`: parse, validation, missing entry function, parameter
  layout, parameters too large, image slots past the limit. A foreign naga error is the
  `source`.

### Paint

- **An effect is a shape of its own.** `Shape::effect(rect, corners, name)` for a color effect,
  `Shape::backdrop_effect(rect, corners, name, max_read_offset)` for a backdrop one. It paints in
  record order, and its rect may exceed its node, as a ring does. Refused: a `Brush` variant,
  which every brush family would have to draw; a `Chrome` field, which no ring past the rect
  fits.
- **Clipping ancestors clip it**, as they clip a drop-shadow halo and as CSS clips ink overflow.
  Refused: Android's projection onto the nearest ancestor with a background, a new paint route;
  a higher layer, which paints over neighbors.
- **A backdrop effect states how far it reads, per shape**, as SwiftUI's `maxSampleOffset`. A read
  past it clamps to the captured area. Refused: one bound per effect, which the largest instance
  sets for all; the whole surface (Flutter without a `ClipRect`).
- **Up to 4 image slots**, declared at load and bound per shape. A draw splits where instances
  bind other images. Refused: a bindless array, which GL and older Metal tiers lack.
- **Every pipeline compiles at load**, for each live swapchain format, with its rounded-clip twin,
  and again for a new format or device. Refused: the first draw, a stutter on the first press,
  the reason Flutter wrote Impeller.

### Time

- **A shape-local clock.** A shape names its start and a motion: `Still`, `Once(duration)` or
  `Loop(period)`, with optional steps. The shader reads the time since the start, wrapped at the
  period, so it keeps its precision. Palantir samples it at encode time and wakes only while it
  moves, as `PaintAnimation` does. Refused: one global clock (Godot `TIME`, Shadertoy), which
  wraps and loses precision; time passed by the app (SwiftUI `TimelineView`), which damage
  cannot see.
- **Reduced motion:** a `Once` effect does not paint, and a `Loop` effect holds at its rest time,
  0 unless the motion names one.

### Power saver and themes

- **A power-saver alternative on the load.** The source names what paints while power is
  reduced: the same effect, another `EffectName<U, K>`, or nothing. Palantir resolves it each
  frame, as `BackdropFilter` carries its own fallback. Refused: theme slots for each mode, which
  double every slot; a branch at each call site.
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

### 2. The 30 Hz limit

- While power is reduced, no frame starts sooner than the smallest whole number of refresh
  periods at or above 1/30 s after the last, in every host and for every cause of a frame.
- Tests on the stepped clock: the interval at 60, 120 and 144 Hz (2, 4 and 5 periods), and input
  that arrives inside the interval paints at its end.

### 3. The winit host reads the mode

- `zbus` and `objc2-foundation` as direct dependencies.
- `host/winit/platform/{linux,macos,windows,other}.rs` read and follow the mode into
  `SystemPreferences`; `other.rs` reports none.
- Tests: the mapping from each OS answer, Windows' three levels included. CI cannot change an
  OS mode, so each OS is checked by hand once and the result goes into the research file.

### 4. The effect registry

- `EffectName<U, K>`, its kinds, the effect source with its image slot count and power-saver
  alternative, `Ui::load_effect`, `EffectError`.
- Validation through `wgpu::naga`, the parameter check against `U`, the 256 B limit, the run-time
  check of `(U, K)` behind each name.
- Tests: one per error case, a load that replaces, a failed reload that keeps the old effect.

### 5. Color effects paint

- Confirm `DUAL_SOURCE_BLENDING` on lavapipe, which CI runs, before anything else; then request
  it, and refuse a device without it with a reason that names it.
- `Shape::effect`, its record, lowering and hash; a new `PaintTier`, ordered by the existing
  conflict flush; culling and damage over its rect; ancestor clips.
- The effect pipeline family: the prelude and contract, the blend, the per-instance storage
  buffer, compile at load, again on a new format and after device loss.
- Tests: the parameters and coverage the encoder writes; goldens of over, add, multiply and
  screen; a ring past its node under a clipping scroll.

### 6. Effect motion

- `Still`, `Once`, `Loop` and steps; encode-time sampling on paint-only frames; wakes; reduced
  motion; the power limit from batch 2.
- Tests: the time each frame passes, the wrap at the period, no wake when still, the reduced-
  motion cases.

### 7. Image slots

- Up to 4 slots per effect, bound per shape, a split where the images differ.
- Tests: the binding, the split, an `ImageHandle` that is not resident after device loss.

### 8. Backdrop effects

- The backdrop signature, `Shape::backdrop_effect` and its read bound, the capture grown by it,
  damage when content under the grown rect changes, the clamp past the bound.
- Tests: the captured rect, damage from content under the bound, a golden of a displacement.

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
- **Windows 24H2 and the older read.** Whether the effective power mode API reports Energy saver
  too, so one read serves every version, is unverified; batch 3 checks it on a machine.
- **Reduced transparency and backdrop effects.** A distortion adds no transparency, so whether
  the preference applies to it stays open until batch 8.
