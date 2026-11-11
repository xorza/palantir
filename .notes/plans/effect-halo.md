# Halos and shadows on the shape template

A color effect that paints around a box, as the showcase's ring does, runs its fragment stage
over the box it surrounds too, and a shadow draws a grid of its own beside the template every
other closed shape draws. This plan makes the shape template the region between two rounded
outlines with its interior optional, so a box's fill, an effect's halo, an inset shadow and a
drop shadow draw from one vertex stage and one index buffer, and none of the three outer ones
runs a fragment where it paints nothing.

## What the code does now

- **The template** (`primitives/wgsl/shape_template.wgsl`, `palantir_template_vertex`): per
  corner a ring from the inner chain, inscribed in the arc `depth` inside the shape, to the outer
  chain, circumscribing the arc `PALANTIR_AA_HALF_WIDTH` outside it; per edge a band; then the
  interior. It draws indexed over its points (`shape_template::TemplateIndices`), the interior's
  triangles last: a box's 36 ring and band triangles, then 10 of interior. A fragment reads its
  region and a closed-form distance (`palantir_template_distance`). A transparent solid fill
  already collapses its interior to a point (`template.wgsl`). Quads, shaped images and effects
  draw it.
- **An effect** (`effect/wrapper.wgsl`, `palantir_vs`, `palantir_fs`) draws its shape's template
  through its own pipelines, keyed by `EffectKey { effect, form }`. `EffectInstance` carries the
  shape (`ShapeHeader`), the clock, the parameter offset, flags (`MATERIAL`) and a material's
  border width, which no color effect reads. `EffectInput` holds the shape's rect and corners, the
  time and the scale. The composer (`ComposeSession::effect`) culls and orders the draw by the
  shape's rect, and the cascade grows an effect's paint rect by the antialiasing reach alone
  (`cascade/paint_rect.rs`).
- **The ring** (`palantir-showcase/src/pages/effects`) is an `EffectPaint` over the card grown by
  `RING_REACH` (28 px) on every side, the card drawn over it, the card's inset and radius in its
  params. In the 260 px cell the card is 54 % of the effect's rect.
- **A shadow** (`gpu/pipeline/quad_pipeline/shadow.wgsl`, `vs_shadow`) draws 48 vertices, the 8
  cells of a 3 × 3 grid. A drop shadow's quad is its source moved by the offset and grown by its
  halo, `4σ + max(spread, 0)`; its hole, the middle cell, is the source inset by its largest
  radius and `PALANTIR_AA_HALF_WIDTH`, so a pill or a circle, inset by half its height, has
  none, and the hole's rounded corners and the quad's square ones run the fragment stage for
  nothing. The fragment stage zeroes a drop shadow under its source by a threshold on the
  source's coverage, as the opaque box over it hides the edge, and an inset shadow's hole is its
  core, where its form is zero. The fragment stage reads the pixel center, not a varying the
  geometry hands it, so the geometry only bounds it.
- **The bench** (`gpu/pipeline/effect_pipeline/bench.rs`, landed with this plan):
  `atomic/effect_pass/gpu/<arm>`, the main pass of 28 cards at 2560 × 1440, each glowing 24 px
  past its edge with the ring's shader, or casting a drop shadow as far:

  | arm | apple | board |
  | --- | --- | --- |
  | `none`, the cards alone | 74 µs | 2.68 ms |
  | `rect`, a glow over the grown rect | 232 µs | 12.50 ms |
  | `shadow`, a drop shadow, σ 6 | 292 µs | 15.62 ms |

  The cards are 47 % of the glows' rects; on the board the glows take 9.8 ms of the pass.

## Decisions

- **The template is the region between two rounded outlines, its interior optional.** The
  inner chain lies on one outline and the outer chain on another, each a convex polygon with a
  radius at each corner, and the two share their corners' normals, so the strip between the chains
  tiles the region. A box's fill takes the shape inset by its border and the antialiasing reach
  and the shape grown by the reach, as now; a halo, an inset and a drop shadow draw the rings and
  bands alone, the index range that ends before the interior. As Skia Graphite's
  `AnalyticRRectRenderStep` builds a fill and a stroke from one vertex template of outer outsets,
  outer anchors, an inner curve and center vertices that a stroke leaves collapsed
  ([source](https://skia.googlesource.com/skia/+/main/src/gpu/graphite/render/AnalyticRRectRenderStep.cpp)).
  Refused: shadows' 3 × 3 grid for halos, which needs a second pipeline per effect, runs the
  fragment stage in the hole's corners and the quad's, and leaves a pill no hole; a second vertex
  stage per use.
- **The halo is a paint's, of the shape it surrounds**: `EffectPaint::outside(reach)` paints
  outside the shape out to `reach` logical px, as CSS's outset `box-shadow` is drawn outside the
  border box only, and as a drop shadow is clipped by its source. Chosen by the user. Refused: a
  shape of two edges, Skia's `drawDRRect`
  ([example](https://skia.googlesource.com/skia/+/main/docs/examples/Canvas_drawDRRect_b.cpp)),
  Android's `drawDoubleRoundRect`
  ([commit](https://gerrit.omnirom.org/plugins/gitiles/android_frameworks_base/+/adfe1d991bbc65dbf80f16f493b88a72adc701d6%5E%21)),
  React Native Skia's `DiffRect`
  ([docs](https://www.mintlify.com/shopify/react-native-skia/shapes/rectangles)): a shape kind
  more, and an effect that reads the outer rect is told the inner one through its params.
- **A halo's effect reads its shape's frame**: `EffectInput` holds the box's rect and corners and
  `pos` runs negative outside it, so an effect measures its distance to the box from its input
  alone; the ring's `inset` and `radius` leave its params. The contract's doc says a halo's `pos`
  lies outside `0..size`.
- **A halo's coverage is the template's, inverted**: `palantir_edge_coverage(−d)` of the region's
  closed-form distance, exact and antialiased at the shape's edge whatever the effect returns,
  with no fragment inside it. The inner and outer chains of a halo share their corners' centers,
  so the regions' distances stay the shape's own.
- **A shadow keeps its fragment stage; the template only bounds it.** Its coverage is worked out
  from the pixel center as now, so a region's distance is not read and the outlines need not
  share centers. An inset shadow's outlines are the blurred hole's core and the source grown by
  the antialiasing reach; a drop shadow's the source and the shadow's box grown by its halo.
  Where the outer outline does not hold the inner, a drop shadow whose offset passes its halo,
  it draws the shadow box's template whole, interior included, with the source clip as now.
- **Only where the bench approves**: shadows move to the template only if `effect_pass/gpu/shadow`
  and the guard cases improve beyond the floor on one host and regress on none; the template's
  vertex stage cost a fifth of V3D's main pass before it was indexed (`.notes/PERF.md`), and a
  shadow's would work out two outlines. Otherwise shadows keep the grid, and its hole takes the
  largest rect inside the source rather than the source inset by its largest radius: each side
  inset by `max(PALANTIR_AA_HALF_WIDTH, r(1 − 1/√2) + PALANTIR_AA_HALF_WIDTH/√2)` over its two
  corners, so a pill and a circle get a hole.
- **Color effects, boxes and circles.** `outside` is on `EffectPaint<U, ColorEffect>`; a backdrop
  effect's halo waits for its capture to take the reach in. A triangle with a halo panics in
  `check`, as a triangle's shadow does: its effect reads no corners, so it cannot find its edge,
  and the halo's outer chain is no rect grown by the reach there. A halo's reduced paint
  (`ReducedPaint::Effect`) draws on the same halo.
- **The reach reaches every reader of the draw's rect**: the cascade's paint rect, the composer's
  cull, its order and its clip tests (`RoundedClip::holds`) take the shape's rect grown by the
  reach, which for a box's 90° corners bounds the outer chain exactly.
- **A halo is a pipeline constant, as a form is**: `EffectKey` takes a `TemplateCover`, `Whole`
  or `Rim`, beside its form, and a halo's pipeline pins `PALANTIR_HALO`, which folds its outer
  reach from the instance and its outside coverage in; a plain draw's pipeline reads neither.
  The instance carries the halo's reach in physical px. Refused, measured in step 3: the halo as a
  bit of the instance's flags in one pipeline, whose every fragment then read the flags, a varying
  more: the bench's plain glows +4.2 % on the apple, +1.0 % on the board.

## Steps

Each step lands on its own, with the gate green, and its own commit.

### 1. The bench — done (`0669dfb16`)

- `atomic/effect_pass/gpu/<arm>` and `atomic/effect_pass_batch/gpu/<kind>/<arm>`
  (`gpu/pipeline/effect_pipeline/bench.rs`), arms `none`, `rect` and `shadow`; it asserts each
  arm's paint and its effect draws.
- `atomic/effect_pass` joins the GPU row of `palantir-showcase/benches/AGENTS.md`.

### 2. The template between two outlines — done

- `palantir_template_vertex(o, u)` over `PalantirTemplateOutlines { inner, depth, outer, reach }`:
  the inner chain `depth` inside `inner`'s corners, the outer chain `reach` outside `outer`'s,
  each corner worked out by `palantir_shape_corner`, the regions' distances `inner`'s. A fill's
  outlines are `palantir_fill_outlines(s, border_width)`, its shape twice with the depth and the
  reach the one `depth` and the constant reach were; the quad, the shaped image and the effect
  wrapper take them.
- The rings-and-bands range of `TemplateIndices` moves to step 3, where a halo first draws it, as
  a range nothing draws would be dead code.
- Tests: `a_halos_outlines_place_its_chains`, a probe of a halo's chains and distances against
  hand-computed values, and of an outer outline moved apart from the inner; the cross-check against
  the trigonometric template holds; `cargo visual` passes.
- Not bit for bit, as planned: the rewrite is equal in IEEE arithmetic, but Apple's fast-math
  compiler schedules it otherwise and moved 120 of 1,408 vertex lanes of six shapes by at most
  1.8e-6 of the value (a one-off probe against the step-1 function), and one pixel of
  `glass_matches_golden` by one level, whose local golden was rewritten. Guard, ABBA against
  `284cf1c1e`: board `main_pass/gpu/scrolling` +0.02 %, `frame/gpu/scrolling` +0.00 %, `resizing`
  −0.02 %, `effect_pass/gpu/*` within ±0.07 %; apple main pass −0.46 %, frames −0.09 and −0.20 %,
  `effect_pass/gpu/*` −0.7 to +2.1 %, within its GPU floor, the shadow arm's pipeline untouched.
  On V3D every template vertex shader keeps its instruction count (`V3D_DEBUG=shaderdb`, 367 in
  binning, 441 to 505 rendering), the corner a fill works out twice folded into one.

### 3. The halo — done

- `EffectPaint::<U, ColorEffect>::outside(reach)`, `reach` a length (`domain`); a triangle's
  halo panics in `check`.
- The reach rides `ClosedPaint::Effect` (hashed into its content) and `ClosedEffect`;
  `cascade/paint_rect.rs` grows the shape's rect by it; `ComposeSession::effect` culls, orders and
  clips by the shape's rect grown by it (`TemplateShape::halo`) and writes a `Rim` key and the
  reach in physical px, at least the antialiasing reach, to `EffectInstance::reach` (100 B, its
  pin moved). `palantir_vs` takes the reach and `palantir_fs` the outside coverage
  (`palantir_template_outside_coverage`) where `PALANTIR_HALO` is pinned; `TemplateIndices::draw`
  takes the cover and draws a rim's rings and bands alone.
- The bench gains `halo`, the glow as the card's halo over the card's own frame (`HaloGlow`).
- Tests: on the GPU, a halo's pixels (inside its box, past its edge, at its reach, around a
  circle's arc, and the effect reading its shape's frame); a moving halo's damage and a reach that
  grows, each rect hand-computed; a triangle's halo panics; a halo's row at scale 2, culled by
  its reach; a key's slot over form and cover; a color effect builds a halo and a backdrop
  effect's is refused; the rim's index ranges.
- Measured, ABBA against step 2 and the arms in one binary:

  | case | apple | board |
  | --- | --- | --- |
  | `effect_pass/gpu/rect` → `halo` | 230 → 152.5 µs, −34 % | 12.48 → 7.84 ms, −37 % |
  | the glows alone, less `none` | 160 → 82 µs, −49 % | 9.80 → 5.16 ms, −47 % |
  | `effect_pass/gpu/rect` against step 2 | −0.1 to +1.7 % | −0.01 % |
  | guard: main pass, frame | within the floor | −0.05 %, −0.29 % |

### 4. The ring

- The showcase ring draws `Shape::rect().at(card).corners(16.0)` with
  `EffectPaint::new(RING, …).outside(RING_REACH)`; `ring.wgsl` measures from `input.pos`,
  `input.size` and `input.corners`, its params the colors and the glow.
- `cargo shot` of the Effects page before and after.

### 5. Shadows on the template, measured

- A spike in a worktree: `vs_shadow` builds the two outlines of each kind and draws the rings
  and bands, or a drop shadow's whole template where its outer outline does not hold its source.
  ABBA of `effect_pass/gpu/shadow`, `integration/backdrop_frame/gpu/*` and the guard cases on the
  apple and the board.
- Taken: the grid leaves `shadow.wgsl`, `SHADOW_GRID_VERTICES` goes, and the reference build's
  one-quad shadow (`ShadowCells::Whole`) stays the visual suite's reference. Tests: the shadow
  goldens byte-identical; a probe that a drop shadow whose offset passes its halo takes the whole
  template.
- Refused: the grid stays and takes the larger hole of the decision above, with a test of a pill's
  hole and the same goldens.

### 6. Docs

- `AGENTS.md`: the template's two outlines beside "A box, a circle and a triangle draw as a
  template", the halo beside "An effect is a program", and the shadows' geometry as step 5 left
  it.
- `cargo drive surface` regenerates `.notes/API_SURFACE.md`.
- The plan folds into `AGENTS.md` and `.notes/PERF.md` and is deleted.

## Benching

ABBA per case as `.notes/PERF.md` says, on the apple and the board: `effect_pass/gpu/rect` against
`halo` for steps 3 and 4, `effect_pass/gpu/shadow` and `integration/backdrop_frame/gpu/*` for
step 5, and the guard, `integration/frame/gpu/*` and `atomic/main_pass/gpu/*`, against the base
for every step. The guard's page draws no halo and no shadow, so step 2 must leave it within its
floor: a vertex stage that works out two outlines where it worked out one.

## Gate at the end

The gate chain of `AGENTS.md`, `cargo visual`, `cargo drive surface` and the shot of step 4.
