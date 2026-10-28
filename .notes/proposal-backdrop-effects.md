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
- **The pane reads past its edge.** The blur is the exact convolution of
  what is painted around the rect, so it reads the footprint, the rect grown
  by the blur's exact reach (or the lens's), and content glows in before it
  slides under a bar. Mirror applies only at the surface edge, where nothing
  is painted. Refused: CSS's in-bounds read with mirrored edges (Filter
  Effects 2) and Android's clamped edges, which paint invented content near
  the edge; a mirrored edge still shifts as content scrolls under it.
- **Three looks, one type.** `Backdrop { blur, saturation, noise, lens }`,
  plain data with `NONE` and the named constructors `blur(σ)`, `frosted(σ)`
  and `glass(lens)`. Blur is a Gaussian σ in logical px, as `Shadow::blur`
  and as CSS `blur()`, whose length is σ (Apple's `blur(17.5px)` is σ 17.5,
  not halved as a `box-shadow` radius is). Frosted adds a saturation factor
  and grain (Apple's 1.8; Acrylic's tint and noise). Glass adds a `Lens {
  bezel, thickness, refractive_index, dispersion, highlight }`. Refused:
  Acrylic's exclusion blend (its replications drop it, as its effect does
  not show), a free light direction (a fixed top-left light, as every
  system material uses; a field later if a workload needs it).
- **Linear light.** The capture, the blur and the composite run in linear
  light, as the rest of the pipeline blends, so a blur keeps the energy of
  bright detail. The levels are `Rgba16Float`; a device that cannot render,
  blend and filter it takes `Rgba8UnormSrgb`, which still blends linear and
  costs only precision. Refused: blurring sRGB-encoded values to imitate a
  browser's look; a pane drawn as its tint alone on such a device.
- **A windowed rect with a backdrop blurs outside its window.** The
  composite samples by screen position, so a spotlight over a blurred page
  costs nothing more.

## The blur

- **Exact variance from a pyramid and a residual Gaussian.** Under σ = 2
  physical px the capture is full resolution and the composite reads it
  texel for texel. From σ = 2 the capture is half resolution, a tent `[1 3
  3 1]` downsample (four bilinear fetches at ±0.75 texel) halves it while σ
  in the current level's texels exceeds 4 (Skia's threshold), a separable
  Gaussian with linear sampling (Rákos) runs at the last level, and the
  composite reads that level through a bicubic B-spline (four bilinear
  fetches, Sigg and Hadwiger). Each stage adds a fixed variance at every
  phase (the B-spline's by Marsden's identity), so the residual Gaussian
  takes exactly what the capture's 2 px box, each tent's 0.75 texel² and
  the B-spline's 1/3 texel² leave of σ²; that remainder is at least 0.58
  texel² at the first level (σ = 2) and 3.4 texel² from the second. The residual's taps are solved so that their discrete,
  truncated variance is the remainder, not their nominal σ, which a `ceil(3σ)`
  cut narrows by 1.3 %. `BlurPlan::new(σ)` is a plain fn, since `f32::exp`
  is not `const`. Refused: dual Kawase, whose strength steps by iteration
  count and offset and is no Gaussian (KWin: "not enough strength steps"); a
  full-resolution Gaussian, linear in σ per pixel; bilinear upscaling,
  which shows the lattice.
- **The reach is the plan's exact support.** `BlurPlan::reach` sums every
  stage's support (the residual's taps, each tent, the B-spline, the
  capture's box) in physical px. A rect's pixels then depend only on the
  footprint, so a capture of any larger region gives them bit for bit.
  Refused: 3σ of the whole kernel, which leaves the edge pixels dependent
  on how far the capture reached.
- **The lattice is the surface.** Each level renders into a retained
  texture of the surface's size divided by its scale, with a mip per level,
  scissored to the footprint snapped out to `2^levels` physical px. A pane
  that slides over a still page then blurs the same texels the same way
  every frame, and two captures of one place are the same arithmetic.
  Refused: footprint-sized targets with an offset, whose float subtraction
  moves a rasterized edge by the offset's rounding.
- **No banding.** The pyramid is `Rgba16Float`. Before an 8-bit target
  writes, the composite adds interleaved gradient noise of one encoded step,
  scaled to linear by the sRGB curve's slope at the pixel's value; the
  target's depth is a constant per pipeline set, so a float target adds
  none. Grain (`noise`) is separate and anchored to the rect's own frame, so
  it moves and turns with the pane.

## The capture

- **A capture replays the draws below the pane into its own target; the
  main pass never splits.** Each capture renders the groups before its
  barrier, scissored to its footprint, into the level-0 texture and the
  frame's clear, then blurs it. Breaking the main pass instead would store
  and load the whole attachment once per barrier on a tiled GPU (about 66
  MB at 4K for each barrier), force the backbuffer path on every frame with
  a pane, and widen damage to the footprint, as the backbuffer under the
  rect holds last frame's pane. The replay costs the below draws' vertex
  work again and a quarter of their fragments. This departs from every
  engine found (WebRender's phases on one surface, Impeller's blit,
  Chromium's readback), so Batch 2 ends on a measured gate against the
  split. Refused: last frame's pixels, which lag and include the pane
  itself.
- **The replay draws through capture pipeline sets.** A set is keyed by
  format and scale, as `FormatPipelines` is by format today. Its shaders
  take the scale and `AA_HALF_WIDTH / scale` as substituted constants, map
  `@builtin(position)` back to surface px through the scale, and force the
  raster atlas's resampling path, whose bilinear read at 2× is the exact
  2×2 box. Vertex shaders do not change: `clip_from_px` divides by the
  surface size, which a half-size target keeps. The main pass's shaders fold
  a scale of 1 away. Refused: the scale and AA width as immediates, which
  cost every main-pass fragment a load and the constant folding the quad
  shader relies on. Half resolution is the floor: past 2× the bilinear
  resample skips texels.
- **A capture is cached across frames.** The blurred last level (and the
  sharp capture a lens reads) is kept per render owner and pane key,
  `WidgetId` and shape ordinal, with its plan and captured region, in a
  retained atlas kept alive by a roster, as `GpuView` targets are, and
  dropped with `retire_render_owner`. A pane reuses its entry while its
  snapped footprint lies inside the captured region, its plan and scale
  are unchanged, no damage before it touched the region since, and the
  frame is not `Full`. A hover or a caret inside a frosted menu repaints
  its rect and samples the cache: no capture, no blur.
- **Damage takes the pane's rect, not its footprint.** Every raw damage
  rect carries its paint-order position. A removed widget's rects count as
  before every pane, a cost of one recapture at worst. The cascade lists
  each pane's footprint and position. `finish_region`, and the paint-only
  path, walk the panes in paint order: damage before a pane that touches its
  footprint adds the pane's rect, which counts as damage at the pane's
  position for the panes above it, so one forward walk is the fixed point.
  The encode filter also takes the footprints of recapturing panes, so the
  replay has its draws; the main pass's scissors stay the damage. Refused:
  Chromium's whole-surface rule applied to the footprint, which only the
  pass split needs.
- **The composer infers sharing.** A pane opens a barrier: groups and text
  batches end there, so no occlusion prune or coalesce crosses it. A later
  pane joins the open barrier when nothing painted since touches its
  footprint, and joined captures with one plan whose footprints overlap
  merge into one replay. The sharing Flutter and SwiftUI ask an author to
  declare falls out of paint order.

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
  capture in the surface's format, which holds what the surface holds at no
  more precision, made only for a lens or a σ under 2 px.

## Budgets

- **A still pane costs nothing; a frame pays only for panes whose backdrop
  changed.** No allocation after warmup: the level textures and the cache
  atlas grow with a blit, as the raster atlas does, and never in a steady
  state. While a blurred pane is on screen, the `Rgba16Float` levels with
  their mips add about two thirds of an 8-bit backbuffer, and a stencil of
  a quarter of its pixels; a lens adds a backbuffer.
- **Targets the benches hold on Apple M5 at 4K.** Capture plus blur of a
  1920×1080 physical footprint at σ = 40 px under 0.25 ms GPU; a cached
  frame with three panes within 5 % of the same frame without them; the
  damage walk and the composer's barriers under 2 % of the CPU frame.

## Out of scope

- A filter on a subtree's own pixels (CSS `filter: blur`): it needs isolated
  offscreen groups and has no workload; a modal's blurred page is a
  full-window pane.
- Progressive (varying) blur: the pyramid keeps it possible; Haze measures
  it at about 25 % over a plain blur.
- OS window materials (Mica, `NSVisualEffectView`): Palantir forces an opaque
  clear and draws its own background.

## Themes

- **The default themes keep opaque popups and menus.** A backdrop costs
  nothing until an author asks for one; the showcase's "Materials" page
  shows frosted ones. Refused: frosted defaults, which charge every app a
  capture and a blur under each open menu.

# Part 2: Plan

Each batch leaves `cargo drive gate` and the golden suite green. Batches 1–5
stay crate-private; the public API in Batch 6 waits for this proposal's
go-ahead. Each bench case asserts what its name claims through the backdrop
counters (captures, blurs, cache hits, barriers), as `src/bench/mod.rs`
requires, and a GPU case reads its passes' device time through pass-boundary
timestamps, which split nothing on a tiler.

## Batch 1: The blur engine

- [ ] `gpu/backdrop/`: `BlurPlan::new(σ_physical)` (levels, residual taps,
  reach), the level textures with their mips, the tent downsample and the
  two Gaussian passes, mirror at the surface edge, the cache atlas on the
  raster atlas's `ShelfPacker`.
- [ ] Tests: the plan's stage variances, the residual taken from its actual
  taps, sum to σ² within f32 rounding across σ from 0.25 to 400 and at each
  level boundary; the residual stays positive; the reach covers every tap. A
  GPU blur of an impulse at each phase of the lattice and of a step against
  the analytic Gaussian, within a tolerance from the hardware's bilinear
  weight precision and `f16` storage; the blur at σ just under and just over
  each level boundary differs by less than that tolerance.
- [ ] Bench `atomic/backdrop_blur/gpu/<σ>/<side>`: σ 1, 4, 16, 48 and 160
  physical px (levels 0 to 5) over 512² and 2048² footprints; device time.

## Batch 2: The barrier and the replay

- [ ] `RenderBuffer.backdrops` (key, footprint, plan, barrier position);
  the composer ends groups and text batches at a barrier.
- [ ] Capture pipeline sets: scale and AA width substituted, frag-coord
  reads mapped through the scale, the raster resample path forced.
- [ ] `RenderStep::Capture`: the schedule replays the groups before a
  barrier into the level-0 texture, with a stencil of its size for rounded
  clips; captures run in paint order before the main pass.
- [ ] Tests: schedule order for one, two and nested panes; a half-scale
  replay equals the full-resolution frame averaged over 2×2 boxes, within
  the stated tolerance, for quads, shadows, text, icons and images; a
  replay of a larger region equals a replay of the footprint bit for bit
  inside it.
- [ ] Bench `atomic/backdrop_capture/gpu/<scene>/<side>`: the replay of the
  frame fixture and of a text-dense page into 1920×1080 and 3840×2160
  footprints; device time. Bench `atomic/schedule/cpu/backdrop`: the
  schedule walk with 1, 8 and 64 barriers.
- [ ] Gate: spike the main-pass split for a `Full` frame and run
  `atomic/backdrop_capture/gpu/split/*` against the replay on this host and
  the M5. If the split wins on both, stop and revise The capture before
  Batch 3; the spike stays on a local branch.

## Batch 3: The composite

- [ ] `QuadForm::Backdrop`, the capture parameter table, the B-spline read,
  saturation, grain, dither, tint from `fill`; a crate-private
  `RectShape::backdrop`.
- [ ] Goldens: a blurred pane over stripes and text, a frosted pane, a pane
  at the surface edge, a rounded bordered pane, a turned pane, a windowed
  pane, a pane over a pane.
- [ ] Bench `atomic/backdrop_composite/gpu/<look>/<side>`: `blur` and
  `frosted` over 512² and full-surface panes; the composite batch's device
  time, on adapters with in-pass timestamps, as `image_pipeline` does.

## Batch 4: Damage and the cache

- [ ] Paint positions on raw damage rects; the cascade's pane rows; the
  forward walk in `finish_region` and the paint-only path; the encode
  filter takes recapturing footprints.
- [ ] The capture cache, its roster, its containment check and its
  retirement per render owner.
- [ ] Tests: damage before a pane inside its footprint recaptures and adds
  its rect; damage after it inside its rect does not recapture; a caret
  blinking inside a pane does not recapture; a removed widget under a pane
  recaptures; two stacked panes chain in one walk; a pane sliding inside
  its captured region reuses it; a partial frame matches a full repaint
  within one 8-bit step in each case, the step from the hardware's filter
  weights at another atlas slot.
- [ ] Bench `atomic/damage/cpu/backdrop_walk`: 1, 8 and 64 panes against
  1, 16 and the budget's damage rects.
- [ ] Bench `integration/backdrop_frame/<backend>/<look>/<arm>`, `cpu` and
  `gpu`, looks `blur` and `frosted`, over the frame fixture with a sidebar,
  a toolbar and a popup as panes. Arms: `cached` (still), `hover` (damage
  above a pane inside its rect: no capture), `under` (a counter under a pane
  ticks: one recapture), `sliding` (a pane moves over a still page), and
  `scrolling` (the page scrolls under the panes: every pane recaptures).
- [ ] An allocation gate in `tests/alloc/gates/` for each look's `cached`
  and `under` frames.

## Batch 5: Sharing

- [ ] Joined barriers and merged replays.
- [ ] Tests: the exact barrier and replay counts for side-by-side panes,
  overlapping panes and a pane over a pane.
- [ ] Bench: `integration/backdrop_frame/*/*/scrolling` repeats with sharing
  on, and its counters assert one replay for the sidebar and toolbar.

## Batch 6: The public surface

- [ ] `Backdrop`, `Lens`, `Chrome.backdrop` with `with_backdrop`,
  `RectShape::backdrop`; their domain checks, serde and `Animatable`;
  regenerate `.notes/API_SURFACE.md`. Check `size_of::<Chrome>()` and the
  record and cascade benches for the wider `Chrome`.
- [ ] A showcase "Materials" page: the three looks over a moving page, and a
  frosted menu and popup.

## Batch 7: Glass

- [ ] The lens in `QuadForm::Backdrop`: refraction, dispersion, highlight;
  the sharp capture.
- [ ] Tests: the displacement's closed form against a numeric ray trace;
  goldens of a glass pill and a glass panel.
- [ ] Benches: `atomic/backdrop_composite/gpu/glass/<side>`, with and
  without dispersion; `integration/backdrop_frame/*/glass/*` on every arm;
  the allocation gate for `glass`.

## Batch 8: Measure and close

- [ ] Bench `integration/backdrop_frame/gpu/modal/<arm>` at 4K: a
  full-window frosted pane over the showcase's heaviest page, `cached` and
  `scrolling`, and the split spike of Batch 2 against it; keep the replay
  unless the split wins there.
- [ ] Fold the lasting decisions into the `gpu/backdrop` and `damage` module
  docs, delete this proposal and the TODO item.

## Batch 9: Optimize

- [ ] Run `/optimize-perf src/gpu/backdrop` with the backdrop paths of
  the composer, the damage walk and the quad shader, over every case under
  `'backdrop'`, with the M5 named as a bench host, against the budgets
  above. Candidates to journal first: a capture margin, so a sliding pane
  reuses its region longer; `Rg11b10Ufloat` levels, which halve the
  bandwidth of an opaque capture, if a banding golden holds; a recapture of
  only the damaged part of a region, grown by the reach; the main pass
  pruning draws a pane covers while the replay keeps them, a redesign the
  loop spikes; the separable passes as one compute pass with the level in
  workgroup memory.
