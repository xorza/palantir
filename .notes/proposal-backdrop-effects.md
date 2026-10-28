# Proposal: backdrop blur, frosted glass and glass (2026-10-28)

Covers the TODO item "glass, blur". Facts and sources are in
`.notes/research/2026-10-28-backdrop-effects.md`.

# Part 1: Decisions

## The contract

- **A backdrop is a property of a rect, and a pane shows what is painted
  before it.** `RectShape::backdrop(Backdrop)` and `Chrome.backdrop` make the
  rect's interior show the pixels painted before it in paint order, blurred
  and shaded, under its own fill and border; the fill is the tint, as CSS
  pairs `backdrop-filter` with `background-color`. Every earlier layer and
  every earlier pane are behind it, so glass over glass is exact. Refused: a
  separate backdrop shape (a second quad over the same pixels and a tint
  that cannot fuse); an explicit group widget as Flutter's `BackdropGroup`
  and SwiftUI's `GlassEffectContainer` need (the composer infers sharing,
  below); "glass cannot sample glass" as SwiftUI rules (a correctness loss
  for a saving the cache already gives).
- **The pane reads past its edge.** A blur reads its footprint, the rect
  grown by 3σ (or by the lens's reach), as Firefox and Safari do, so content
  glows in before it slides under a bar. Mirror applies only at the surface
  edge. Refused: Chrome's former in-bounds read with duplicated edges, which
  flickered on a 1 px scroll (research: CSS `backdrop-filter`).
- **Three looks, one type.** `Backdrop { blur, saturation, noise, lens }`,
  plain data with `NONE` and the named constructors `blur(σ)`, `frosted(σ)`
  and `glass(lens)`. Blur is a Gaussian σ in logical px, as `Shadow::blur`.
  Frosted adds a saturation factor and grain (Apple's 1.8 and blur 17.5 px;
  Acrylic's tint and noise). Glass adds a `Lens { bezel, thickness,
  refractive_index, dispersion, highlight }`. Refused: Acrylic's exclusion
  blend (its replications drop it, as its effect does not show), a free
  light direction (a fixed top-left light, as every system material uses;
  a field later if a workload needs it).
- **Linear light.** The capture, the blur and the composite run on linear
  `Rgba16Float`, as the rest of the pipeline blends, so a blur keeps the
  energy of bright detail. Refused: blurring sRGB-encoded values to imitate
  a browser's look.
- **A windowed rect with a backdrop blurs outside its window.** The
  composite samples by screen position, so a spotlight over a blurred page
  costs nothing more.

## The blur

- **Exact σ from a pyramid and a residual Gaussian.** The capture renders at
  half resolution once σ reaches 2 physical px, a tent `[1 3 3 1]`
  downsample (four bilinear fetches) halves it while σ in the current
  level's texels exceeds 4 (Skia's threshold), a separable Gaussian with
  linear sampling (Rákos) runs at the last level, and the composite reads
  that level through a bicubic B-spline (four bilinear fetches, Sigg and
  Hadwiger). Variances add, so the residual Gaussian takes exactly the σ²
  the capture's 2 px box, each tent's 0.75 texel² and the B-spline's 1/3
  texel² did not; under σ² = 1/3 a direct 3×3 Gaussian replaces the
  B-spline. The plan is a `const fn` of σ and is unit tested. Refused: dual Kawase, whose strength steps by iteration count and
  offset and is no Gaussian (KWin: "not enough strength steps"); a
  full-resolution Gaussian, linear in σ per pixel; bilinear upscaling, which
  shows the lattice.
- **The lattice is anchored to the surface.** A footprint's origin snaps to
  a multiple of `2^levels` physical px, so a pane that slides over a still
  page blurs the same pixels the same way every frame.
- **No banding.** The pyramid is `Rgba16Float`; the composite adds
  interleaved gradient noise of one output step before an 8-bit target
  writes. Grain (`noise`) is separate and anchored to the rect, so it moves
  with the pane.

## The capture

- **A capture replays the draws below the pane into its own target; the
  main pass never splits.** Each capture renders the groups before its
  barrier, clipped to its footprint, into a scratch target at half
  resolution, through the same schedule walk with a viewport scale and
  offset in the shared immediates and an AA width scaled to match, then
  blurs it. Breaking the main pass instead would store and load the whole
  attachment once per barrier on a tiled GPU (about 66 MB at 4K for each
  barrier), force the backbuffer path on every frame with a pane, and widen
  damage to the footprint (below). The replay costs the below draws' vertex
  work again and a quarter of their fragments. Refused: the pass split as
  the one path, kept as the fallback to measure if a full-window blur over a
  heavy scene loses (Batch 8); last frame's pixels, which lag and include
  the pane itself.
- **A capture is cached across frames.** The blurred level (and the sharp
  level a lens reads) is kept per pane key, `WidgetId` and shape ordinal,
  with its σ, scale and lattice origin, in a retained atlas kept alive by a
  roster, as `GpuView` targets are. A pane recaptures only when damage from
  content before it touches its footprint, or its key, σ, scale or origin
  changes, or the frame is `Full`. A hover or a caret inside a frosted menu
  repaints its rect and samples the cache: no capture, no blur.
- **Damage takes the pane's rect, not its footprint.** The cascade lists each
  pane's footprint and paint-order position; `finish_region` walks them in
  paint order, and damage from earlier content that touches a footprint adds
  the pane's rect, which counts as damage at the pane's position for the
  panes above it, so one forward walk is the fixed point. The encode filter
  also takes the footprints of recapturing panes, so the replay has its
  draws; the main pass's scissors stay the damage. Refused: Chromium's whole
  surface rule applied to the footprint, which the pass split would need.
- **The composer infers sharing.** A pane opens a barrier: groups and text
  batches end there, so no occlusion prune or coalesce crosses it. A later
  pane joins the open barrier when nothing painted since touches its
  footprint, and joined captures with one σ whose footprints overlap merge
  into one target. The sharing Flutter and SwiftUI ask an author to declare
  falls out of paint order.

## The composite

- **`QuadForm::Backdrop`, a fourth quad pipeline.** The pane is an ordinary
  `Quad`: SDF, corners, border, AA, turn, rounded clips and scissors all
  hold. The form keeps the backdrop's texture reads and lens math off the
  solid form's registers. `fill` is the tint, and `fill_lut_row` indexes a
  per-frame table of capture parameters (atlas origin, level, saturation,
  noise, lens). A gradient tint draws as its own quad above. The pane
  composites opaque within its coverage, occludes nothing before its
  barrier, and drops its capture when pruned.
- **Glass refracts by a closed form.** The lens height over the bezel is a
  squircle `(1 − (1 − x)^4)^(1/4)` of the inward distance; its slope along
  the SDF gradient is the normal, Snell's law with the pane's index gives
  the bent ray, and thickness times its tangent is the displacement (kube.io
  precomputes the same into a map). Dispersion samples red and blue at their
  own index inside the bezel only. The highlight is Schlick's Fresnel times
  the normal's facing to the light. A lens reads a sharp full-resolution
  capture, made only for a lens or a σ under one texel.

## Budgets

- **A still pane costs nothing; a frame pays only for panes whose backdrop
  changed.** Targets the benches hold on Apple M5 at 4K: capture plus blur
  of a 1920×1080 physical footprint at σ = 40 px under 0.25 ms GPU; no
  allocation after warmup; the capture atlas grows with a blit, as the
  raster atlas does, and never in a steady state.

## Out of scope

- A filter on a subtree's own pixels (CSS `filter: blur`): it needs isolated
  offscreen groups and has no workload; a modal's blurred page is a
  full-window pane.
- Progressive (varying) blur: the pyramid keeps it possible; Haze measures
  it at about 25 % over a plain blur.
- OS window materials (Mica, `NSVisualEffectView`): Palantir forces an opaque
  clear and draws its own background.

## Open questions

- Do the default themes keep opaque popups and menus (recommended: no cost
  unless asked for), or ship frosted ones?

# Part 2: Plan

Each batch leaves `cargo drive gate` and the golden suite green. Batches 1–5
stay crate-private; the public API in Batch 6 waits for this proposal's
go-ahead.

## Batch 1: The blur engine

- [ ] `gpu/backdrop/`: `BlurPlan::new(sigma_physical)` (levels, residual σ,
  linear-sampled taps), the `Rgba16Float` scratch atlas with mip levels and
  `2^levels`-aligned shelves (the raster atlas's `ShelfPacker`), the tent
  downsample and the two Gaussian passes, mirror at the surface edge.
- [ ] Tests: the plan's variances sum to σ² exactly across σ from 0.5 to
  200; a GPU blur of an impulse and of a step against the analytic Gaussian,
  with the tolerance and its reason stated.
- [ ] Bench: `backdrop/blur` at σ 4, 16 and 48 over 512² and 2048²
  footprints.

## Batch 2: The barrier and the replay

- [ ] `RenderBuffer.backdrops` (key, footprint, plan, barrier position);
  the composer ends groups and text batches at a barrier.
- [ ] The shared immediates carry a viewport scale and offset and the AA
  width; every vertex shader applies them.
- [ ] `RenderStep::Capture`: the schedule replays the groups before a
  barrier into the scratch target at its scale, with a stencil of that size
  for rounded clips; captures run before the main pass.
- [ ] Tests: schedule order for one, two and nested panes; a replayed
  capture equals a crop of the full-resolution frame blurred on the CPU.

## Batch 3: The composite

- [ ] `QuadForm::Backdrop`, the capture parameter table, the B-spline read,
  saturation, grain, dither, tint from `fill`; a crate-private
  `RectShape::backdrop`.
- [ ] Goldens: a blurred pane over stripes and text, a frosted pane, a pane
  at the surface edge, a rounded bordered pane, a turned pane, a windowed
  pane.

## Batch 4: Damage and the cache

- [ ] The cascade's pane rows; the forward walk in `finish_region`; the
  encode filter takes recapturing footprints.
- [ ] The capture cache, its roster and its key check.
- [ ] Tests: damage before a pane inside its footprint recaptures and adds
  its rect; damage after it inside its rect does not recapture; two stacked
  panes chain in one walk; a partial frame matches a full repaint pixel for
  pixel in each case.

## Batch 5: Sharing

- [ ] Joined barriers and merged captures.
- [ ] Tests: the exact barrier and capture counts for side-by-side panes,
  overlapping panes and a pane over a pane.
- [ ] Bench: a frame with a frosted sidebar and toolbar, cold and cached.

## Batch 6: The public surface

- [ ] `Backdrop`, `Lens`, `Chrome.backdrop` with `with_backdrop`,
  `RectShape::backdrop`; their domain checks, serde and `Animatable`; regenerate
  `.notes/API_SURFACE.md`.
- [ ] A showcase "Materials" page: the three looks over a moving page, and a
  frosted menu and popup.

## Batch 7: Glass

- [ ] The lens in `QuadForm::Backdrop`: refraction, dispersion, highlight;
  the sharp capture.
- [ ] Tests: the displacement's closed form against a numeric ray trace;
  goldens of a glass pill and a glass panel.

## Batch 8: Measure and close

- [ ] Bench at 4K on M5: the replay against a main-pass split for a
  full-window blur over the showcase's heaviest page; keep the split only if
  the replay loses there.
- [ ] Fold the lasting decisions into the `gpu/backdrop` and `damage` module
  docs, delete this proposal and the TODO item.
