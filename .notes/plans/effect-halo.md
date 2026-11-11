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
- **A halo is a run of its own, not a pipeline**: the halo and the plain draws of one effect share
  its pipeline; the instance carries the outer reach (physical px; the antialiasing reach for a
  plain draw, which a material's border width already rides beside) and a `HALO` flag, and the run
  key, not `EffectKey`, splits them for the shorter index range.

## Steps

Each step lands on its own, with the gate green, and its own commit.

### 1. The bench

- `atomic/effect_pass/gpu/<arm>` and `atomic/effect_pass_batch/gpu/<kind>/<arm>`
  (`gpu/pipeline/effect_pipeline/bench.rs`), arms `none`, `rect` and `shadow`; it asserts each
  arm's paint and its effect draws. Written; it lands first so base and candidate run the same
  cases.
- `atomic/effect_pass` joins the GPU row of `palantir-showcase/benches/AGENTS.md`.

### 2. The template between two outlines

- `palantir_template_vertex(inner, outer, u)`: the inner chain on `inner`'s corners, the outer
  chain on `outer`'s, each corner worked out by `palantir_shape_corner`; a fill passes its shape
  inset by its depth and grown by the antialiasing reach, as the one `depth` and the constant
  reach do now.
- `TemplateIndices` gains the rings-and-bands range beside the whole one.
- Tests: the existing GPU cross-check holds every fill vertex bit for bit to its current output;
  a probe of a halo's points against hand-computed ones, its outer chain `reach` past the shape;
  `cargo visual` byte-identical.

### 3. The halo

- `EffectPaint::<U, ColorEffect>::outside(reach)`, `reach` a length (`domain`), panicking past
  its kind; `check` refuses a triangle.
- Lowering carries the reach to `ClosedEffect`; `cascade/paint_rect.rs` and
  `ComposeSession::effect` grow the shape's rect by it; `EffectInstance` takes the outer reach and
  `HALO` (its pin in `internals::hot_struct_sizes` moves); the effect batch's run key takes the
  flag; `palantir_vs` passes the outer reach and `palantir_fs` inverts the coverage on `HALO`.
- The bench gains `halo`: the same glow with `.outside(REACH)` over the card, its shader reading
  the card's frame.
- Tests: the paint rect of a box's halo, hand-computed; a triangle's halo panics; on the GPU, a
  halo's pixels against the `rect` glow with the box's outside coverage applied by hand, equal
  outside the box, none inside it, half at a pixel the edge halves.

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
