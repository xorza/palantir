# One effect engine, and an analytic rounded clip

Two pieces of work. Part 1 gives a material and a backdrop `EffectPaint` one engine. Part 2
replaces the rounded-clip stencil with a coverage term that each draw computes, so a rounded clip
gets an antialiased edge and stops closing batches. Part 2 lands after Part 1's step 3, so the
clip enters the effect stage through the one effect payload and the one composer path.

# Part 1: one effect engine

A material and a backdrop `EffectPaint` reach the same GPU stage by two record, cascade, payload
and composer paths. This part keeps the two public forms and gives them one engine underneath, so
a material is to a backdrop effect what `PressEffect` is to a color effect: theme data that lowers
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

- Done: one `BackdropCapture` (an f32 σ, and a `CaptureReach`: a fixed reach in logical px or a
  lens, whose margin the cascade derives from the box's narrowest side) and one `EffectBackdrop`
  (`Capture(BackdropCapture) | Ground`) in `shape/paint/backdrop_capture.rs`. `LoweredMaterial`
  and `ShapeRecord::EffectPaint` both hold the `EffectBackdrop`; `BackdropReader` is gone, and
  the cascade reads one `ShapeRecord::backdrop_capture` or `LoweredMaterial::capture`.
- `LoweredMaterial` stays 40 B with an f32 σ, so no σ moved to f16: a material's σ is no longer
  rounded to f16. An effect's composite now reads its reach rounded out to whole physical px, the
  margin its capture is planned with. `EffectCaptureDraw` is `Capture { ticket, reach } | Ground`,
  so a draw over the ground holds no reach. No golden moved.
- Tests: `CaptureReach::margin` for a fixed reach and a lens; the backdrop, material and effect
  tests pass unchanged.

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
- Part 1 leaves this file when step 3 lands; the file is deleted when Part 2's last step lands.

## Gate at the end of Part 1

- The palantir gate.
- `cargo visual`: the same pass and fail set as before step 1, every `actual.png` byte-identical.
- `cargo drive surface`: no change to `.notes/API_SURFACE.md`.
- `hot_struct_sizes` holds `ShapeRecord` 136 B and `ChromeRow` 104 B.

# Part 2: an analytic rounded clip

## What the code does now

- **The composer** (`composer/session.rs`, `push_clip`) gives a rounded clip a scissor (its rect
  clamped to the parent's) and a chain: the parent's `RoundedClip`s copied whole, plus its own,
  a span into `RenderBuffer::rounded_clips`. `DrawGroup` and `TextBatch` each carry a chain.
  `Ordering::break_for_clip` closes the open text batch where the chain changes and the group
  where the scissor or the chain changes.
- **The schedule** (`gpu/frame/schedule`) stages each distinct chain into `MaskPlan`'s mask quads.
  Before a group of another chain it clears the stamped one (one `MaskClear` draw at reference 0)
  and stamps the new one (a `SetStencilRef` and a `MaskStamp` draw per level, `fs_mask` discarding
  past the shape: a hard edge at distance 0).
- **The pass**: a frame with any rounded clip attaches a `Stencil8` the size of its target
  (`gpu/surface/stencil`, and the damage canvas's own), and every draw of that frame binds the
  stencil-test twin of its pipeline (`StencilVariant`, `clip_stencil::test_state`): an `Equal`
  test on every fragment of the frame, inside a clip or not. Every family builds two pipelines a
  format, an app effect builds both at once (`effect_pipeline/mod.rs`, `ensure` for `Off` and
  `Test`), and the quads build the stamp and clear pipelines.
- **Measured** (`.notes/research/2026-11-10-draw-calls.md`): 4 mask draws a page on Tabs, Dock,
  Clip & transform and Motion; a rounded clip is the last thing that closes a text batch (4 on
  Tabs, 4 on Dock, 2 on Motion). No bench draws a rounded clip (`.notes/PERF.md`, "Rejected").

## Decisions

- **A rounded clip is a coverage term each draw multiplies in**, as Chromium's fast rounded
  corners put a `MaskFilterInfo` on the quad and draw no render surface
  ([`cc/trees/effect_node.h`](https://chromium.googlesource.com/chromium/src/+/HEAD/cc/trees/effect_node.h),
  `is_fast_rounded_corner`), and Skia carries an analytic clip in a draw's `PaintParams`
  ([Graphite `ClipStack.h`](https://skia.googlesource.com/skia/+/main/src/gpu/graphite/ClipStack.h)).
  The coverage is `palantir_edge_coverage`, the AA model every shape takes, of
  `palantir_sdf_rounded_rect`, the SDF the one-quad reference form takes, which the template's
  distances equal region by region; so a child that fills its clip draws the edge its parent's
  rounded background draws. The scissor keeps the clip's rect part, as now. Refused:
  - the stencil, which keeps or drops a whole pixel, so the edge is hard, closes the text batch
    at each chain change, and puts a second pipeline of every family and a stencil test on every
    fragment of the frame;
  - clipping the geometry, arcs cut into triangles, which gives the hard edge back, cannot cut a
    mesh, a curve, a glyph or a turned quad in a vertex stage, and errs by its tolerance, as the
    refusal of Impeller's chords in "A box, a circle and a triangle" says;
  - a mask texture each draw samples, as WebRender's clip tasks
    ([`ps_quad_mask.glsl`](https://searchfox.org/mozilla-central/source/gfx/wr/webrender/res/ps_quad_mask.glsl)),
    a pass and a texture a clip;
  - hardware clip distances (`Features::CLIP_DISTANCES`), which are planes and cut no arc;
  - an offscreen layer masked whole, as WebRender does since
    [bug 1823578](https://bugzilla.mozilla.org/show_bug.cgi?id=1823578), the exact edge for a
    group at a pass and a target a clip.
- **Cost: the edge conflates.** Each draw takes the clip on its own, so where two clipped draws
  overlap a pixel of coverage `c`, the lower one shows through by `c(1 − c)`, at most 25 % at
  `c = 0.5`. Skia and Chromium accept it for an antialiased clip on each draw; only the offscreen
  layer refused above is exact. Today's hard edge has no such blend and has a staircase instead.
- **An instance names its chain** in one `ClipChain`: a `u32` holding the chain's start in the
  frame's clip pool (24 bits) and its depth (8 bits, `MAX_ROUNDED_CLIP_DEPTH`, which bounds the
  fragment's loop), `ClipChain::NONE` for none. Each of `Quad`, `RasterQuad`, `ImageInstance`,
  `MeshInstance`, `CurveInstance` and `EffectInstance` takes it, its varying flat. A draw call
  then joins draws under different chains. Refused: the clip inline in each instance, as GPUI's
  `content_mask` is, 24 B a mask and one mask a draw; a chain per draw call in the immediates,
  which keeps a batch per clip. Cost: 4 B an instance in every frame, rounded clip or not, a glyph
  included; accepted. AGENTS.md "A draw call joins what it can" still refuses a clip rect in each
  glyph, 8 B a glyph, as the CPU cut of a run handles a rect clip; the chain serves the arc, which
  no CPU cut can follow.
- **One frame group at group 0 of every color pipeline**: the gradient atlas, its sampler and the
  clip pool. wgpu binds again from the first slot whose layout differs at a pipeline switch
  (`wgpu-core` 30.0.1, `command/bind.rs`, `update_expectations`), so a group 0 every pipeline
  shares is bound once a pass, as Vulkan practice puts the set that changes least at 0
  ([vkguide, descriptors](https://vkguide.dev/docs/chapter-4/descriptors/);
  [Vulkan samples, descriptor management](https://docs.vulkan.org/samples/latest/samples/performance/descriptor_management/README.html)). The
  family groups move up one: the raster atlases and an image's texture to 1, an effect's words to
  3; the shadow's cutouts stay at 1. The bind group count stays 4, `downlevel_defaults`'s limit.
  `primitives::wgsl` holds the frame group's WGSL once, as it holds the SDF. Refused: the clip
  pool at a free slot of each family, a different slot per family and a rebind at each switch; a
  fifth bind group, past the limit palantir asks for.
- **A draw inside every mask of its chain names none.** The composer tests the draw's pixel rect
  against each mask on the CPU and writes `ClipChain::NONE` where every pixel center lies at least
  `AA_HALF_WIDTH` plus a slack inside, so its coverage is exactly 1. The test is exact: a rounded
  rect is convex, so a rect lies inside when its four corners do. The slack holds the GPU's f32
  distance on the CPU's side of the threshold, as `ComposeSession::DECAL_SLACK_PX` holds an image
  edge. As Skia's `ClipStack` drops a clip element that contains the draw.
- **A mask that holds the next one leaves the chain.** At `push_clip`, an ancestor mask that holds
  the new mask's rect grown by `2 × AA_HALF_WIDTH` plus the slack covers 1 wherever the new mask
  covers more than 0 (the new mask covers past its edge by `AA_HALF_WIDTH`, and the ancestor covers
  1 from `AA_HALF_WIDTH` inside its own), so a product with it changes no bit, and most chains are
  one mask deep. As Skia's
  `ClipStack` drops an element another contains.
- **An occluder covers only where its clip covers 1.** `record_opaque_cover` intersects the cover
  with each mask's inscribed rect, deflated by `AA_HALF_WIDTH`: an occluder at a clip edge hides
  `c` of what it covers, and pruning there would change the pixel.
- **Nested masks multiply.** The fragment takes the product of the chain's coverages, as Skia
  multiplies its clip coverage terms.
- **An effect takes the clip outside the app's code.** The wrapper writes `color · c` and
  `under · c + (1 − c)`, which the dual-source blend `src·One + dst·Src1` turns into
  `c · (color + dst · under) + (1 − c) · dst`.
- **What goes**: `StencilVariant`, `clip_stencil`, `gpu/surface/stencil`, the damage canvas's
  stencil, `MaskPlan`, `RenderStep::{SetStencilRef, MaskStamp, MaskClear}`,
  `PassState::establish`, `mask.wgsl` and the mask pipelines, `PipelineCensus::stencil`,
  `DrawGroup::rounded_clips`, `TextBatch::rounded_clips`, `RenderBuffer::chains_equal`, and
  AGENTS.md's "Rounded-clip stencil".
- **Clips with no scissor wait for a measurement** (step 10). With every clip in the instance, a
  group would close only at an overlap or a barrier, as GPUI clips. `.notes/PERF.md` refused the
  rect half before: 340 ns a group on Vulkan against a 2.1 ms frame.

## Steps

Each step lands on its own, with the gate green, and its own commit. Steps 5 and 6 change no pixel.

### 5. Benches and counts first

Every case lands before the change, so the base and the candidate builds run the same ones.

- **Counts.** `PassRecorder` counts what it records under `internals`: draws, pipeline binds,
  group binds, scissors, stencil references, in a `PassCounts` a test and a bench read. It
  replaces the uncommitted patch the draw-call research used.
- **A clip scene**, a fixture in `bench`: 48 rounded cards at 2560 × 1440, each a scroll list of
  rows (a label, an icon, an image thumbnail, a polyline), so every family draws under a rounded
  clip. Its arms:
  - `rect`: the cards clipped by `clip_rect`, the control;
  - `inside`: rounded clips, the content inset past every arc;
  - `edge`: rounded clips, the content crossing every arc;
  - `nested`: three masks a card, none holding the next.
  Each arm asserts its counts, so a fixture that stops doing what its name says fails the case.
- **Cases**, ids as `src/bench/mod.rs` states:
  - `atomic/record_pass/gpu/rounded/per_item` beside `groups/per_item` (`gpu/bench.rs`): the CPU
    time of the main pass record, which the masks' stamps, clears and reference sets add to.
  - `atomic/clip_pass/gpu/<arm>`: the device time of the main pass over the clip scene, a whole
    repaint.
  - `atomic/clip_pass_batch/gpu/<pipeline>/<arm>`: each pipeline's share, from in-pass timestamps;
    none on an Apple GPU or a tiler.
  - `atomic/composer/cpu/clips/<arm>/<count>`: the compose of the scene's paint stream, which pays
    for the chains and, after step 8, the classification.
  - `integration/clip_frame/<backend>/<arm>`, `cpu` and `gpu`: whole frames over the clip scene,
    `scrolling` (a whole repaint) and `hovering` (a partial frame), as `backdrop_frame` runs.
  - The guard: `integration/frame/*` and `atomic/main_pass/gpu/*` over the Controls page, which has
    no rounded clip, read what every frame pays: the wider instances and the clip branch.
  - `atomic/schedule/cpu/walk/rounded_chain/*` stays on the base; it leaves in step 7.
- **Counts of the showcase**: the method of `.notes/research/2026-11-10-draw-calls.md` again on
  Tabs, Dock, Clip & transform and Motion, through `PassCounts`, into a test that pins each page's
  draws.
- **The base numbers** go into `.notes/research/<date>-rounded-clip.md` before step 6, per host.

### 6. The frame group

- `FrameBinding`: the gradient atlas, its sampler and an empty clip pool, at group 0 of every
  color pipeline. The family groups move up one; the effect's words move to group 3.
- `primitives::wgsl` takes `frame.wgsl` in place of `gradient.wgsl`'s binding lines, and every
  color shader and the effect wrapper include it.
- `PassRecorder` binds group 0 once a pass.
- Tests: `PassCounts` on the showcase pages: the group binds fall by the pipeline switches that
  rebound the gradient, the draws stay. `cargo visual` byte-identical.
- Bench: `atomic/record_pass/*` and `atomic/main_pass/gpu/*` against step 5, ABBA.

### 7. The analytic clip

- **The composer**: `push_clip` builds the chain as now, copied whole; each draw writes its
  `ClipChain` from the clip in force; `break_for_clip` no longer closes the text batch; a rounded
  clip still sets its scissor. `record_opaque_cover` keeps to the chain's covered-1 rect.
- **The pool**: `RenderBuffer::rounded_clips` uploads as it is, 24 B a mask, through a
  `DynamicBuffer` that decays as every per-frame buffer does.
- **The shaders**: one `palantir_clip_coverage(px, chain)` in `primitives::wgsl`; every fragment
  entry that draws under a clip multiplies by it where `chain != NONE`: `fs_template`, `fs_shadow`,
  `fs_shadow_tables`, the reference `fs`, the mesh, image, shaped image and curve `fs`, the raster
  `fs` and `fs_spun`, and `palantir_fs` by the rule above. The quad's fast path applies it too.
- **The pass**: no stencil attachment, one pipeline per family and format, the census without a
  stencil mode.
- **What goes**: the list under "What goes".
- **Hot structs**: the pins move by 4 B each: `Quad` 100 → 104, `RasterQuad` 32 → 36,
  `ImageInstance` 108 → 112, `MeshInstance` 32 → 36, `CurveInstance` 76 → 80, and
  `EffectInstance`, 100 B and unpinned, 104 and pinned.
- **Tests**, exact values with the math shown:
  - the coverage: a GPU probe (`ShaderBody`'s probe group) of `palantir_clip_coverage` against a
    CPU twin of `palantir_sdf_rounded_rect`, at the center, on each edge, on each arc and outside
    each corner, for unequal radii; and against the template: a box of rect `R` and radii `r`
    matches a cover of `R` under a clip `(R, r)` at every sampled pixel, within one step of the
    target format, the f32 rounding apart of the two distance formulas;
  - each family under a clip, one table: an opaque fill crossing a corner reads its fill inside,
    the clear outside, and on the arc the blend its coverage gives;
  - an effect: color and under at an edge pixel, against the blend above;
  - nesting: the product at a pixel two masks cut;
  - occlusion: an occluder at a clip edge prunes nothing under its edge band
    (`composer/tests/pruning.rs`);
  - batching: a text run under a rounded clip joins the batch before it
    (`composer/tests/batching.rs`); the showcase pages' draws fall by the mask draws and the text
    closes;
  - pipelines: a frame with a rounded clip builds no second pipeline
    (`host/offscreen/tests/lazy_pipelines.rs`); a recovery rebuilds one set
    (`host/offscreen/tests/recovery.rs`);
  - removed with their code: `schedule/tests/stencil.rs`, `surface/stencil/tests.rs`, the stencil
    parts of `canvas.rs`.
- **Goldens**: the goldens with a rounded clip move, and only in the 1 px band of a clip's edge:
  `corners` (`clip corners(9999)`), `widgets`, `shadow`, `tabs`, and the ones the run names. Each
  diff is read as a mask of moved pixels before `UPDATE_GOLDEN=1`.
- **The darkroom check**: darkroom's inspector clips rounded; `darkroom_shot` of a project shows it.
- Bench: every case of step 5 against step 6, on every host below.

### 8. Fewer masks a draw

- The classification and the chain pruning of "Decisions", with a CPU SDF on `RoundedClip` that
  step 7's probe holds to the GPU's.
- Tests: chains for the nested cases of `composer/tests/clipping.rs` (outer `(10, 10, 200, 200)`
  r8, inner `(20, 20, 100, 100)` r4: the inner holds its rect grown by 1 px inside the outer, so
  the chain is the inner alone); a draw inside, on the slack's boundary and across an edge; and the
  oracle: `cargo visual` byte-identical to step 7, on each host.
- Bench: `clip_pass/gpu/inside` and `composer/cpu/clips/*` against step 7. Kept only where the
  GPU cases move past the noise floor; else refused in `.notes/PERF.md` with its numbers.

### 9. Docs

- AGENTS.md: "Rounded-clip stencil" becomes "Rounded clip", the decisions above; "Scratch decays"
  loses the window's stencil; "A draw call joins what it can" states the 4 B chain in each glyph
  beside its refusal of the 8 B rect: the chain serves the arc, the CPU cut the rect.
- `ClipMode::Rounded` ("Costs a stencil pass") and `Configure::clip_rounded` ("Rounded-corner
  stencil clip") say what the clip costs now. Docs only: `cargo drive surface` shows no change.
- `.notes/research/<date>-rounded-clip.md` holds the numbers; `.notes/PERF.md` the kept and the
  refused.

### 10. Spike: clips with no scissor

- Every clip, a rect too, goes into the pool, and the instance's chain does the rect part; the
  scissor stays for the damage and the pass. A spike on the quad and text families first.
- Its cost: a draw that crosses a clip shades the fragments past it, which the scissor rejected
  for free. A plain image keeps its CPU crop, as a `GpuView` has; a rounded quad cannot.
- Bench: `clip_pass`, `clip_frame` and the guard, against step 8 or 7. Kept only past the floor on
  two hosts; else refused in `.notes/PERF.md` beside the earlier refusal.

## Benching

As `.notes/PERF.md` "Measuring" and `palantir-showcase/benches/AGENTS.md` state:

- **Two worktrees**, `base` at the step before and `cand` at the step. The base binary is built
  once per base commit and kept; `cargo bench -p palantir-showcase --bench palantir --no-run`
  names it.
- **ABBA per case**: `--exact <case> --measurement-time 3 --warm-up-time 1`, legs A, B, B, A,
  medians compared; a change counts only when both comparisons report it.
- **Pinned runs, never pinned builds**: `taskset -c N setarch -R` in front of the binary on Linux,
  `ProcessorAffinity` on Windows; governor `performance` where the host allows it.
- **Hosts**: the fanless Apple laptop (Metal, a tiler: the stencil's load and test and the
  fragment's new branch weigh most there), local (NVIDIA, Vulkan), the handheld (RADV), windows
  (AMD, Vulkan) and the intel (anv). At least the fanless, local and one AMD host each step.
- **One build at a time** where the peaks do not fit together: the fanless builds alone, the
  intel's 6.0 GiB peak beside nothing.
- **A result is a median against the host's noise floor** (`.notes/PERF.md`); inside it reads as
  no change. A fanless surprise is confirmed by interleaved short runs.
- **Beside the times**: the counts per case and per showcase page; the stencil bytes freed, 3.5 MiB
  a window at 2560 × 1440 (`Stencil8`, 1 B a pixel) plus the damage canvas's; and the pipelines a
  frame with a rounded clip builds.

## Profiling

- **GPU**: `clip_pass_batch` for each pipeline's share on the Vulkan hosts. `RADV_DEBUG=shaderstats`
  for each fragment entry's VGPRs and waves before and after: today 24 to 48 VGPRs and 20 to 32
  waves a SIMD with no spill, and the clip may not lower a wave count. `INTEL_DEBUG=wm` with
  `MESA_SHADER_CACHE_DISABLE=true` for SIMD width and spills: an entry that drops from SIMD32 is
  reported. The fanless has no Metal trace, so its numbers are pass times alone.
- **Diagnostic builds, one ABBA each**: the coverage forced to 1, the ALU ceiling of the clip; the
  branch removed, what a frame with no rounded clip pays for it.
- **CPU**: `FILTER='^atomic/composer/cpu/clips/' palantir-showcase/benches/bench-perf.sh` and
  `FILTER='^atomic/record_pass/'`, LBR call graphs on the intel, `perf annotate` on the
  classification and on `PassRecorder`.

## Keep or stop

- **Step 7 stops and reports** where a guard case moves past the floor on two hosts, or a clip
  arm runs slower than the base past the floor. An antialiased edge is a reason to keep it at a
  small cost, which the report states for a decision.
- **Steps 8 and 10 stay only for a measured gain**, as above.

## Gate at the end of Part 2

- The palantir gate, then the darkroom gate, as darkroom clips rounded.
- `cargo bench-once`, as the steps change bench code.
- `cargo visual`: the moved goldens of step 7 alone, each read; step 8 byte-identical to step 7.
- `cargo drive surface`: no change to `.notes/API_SURFACE.md`.
- `hot_struct_sizes` holds the pins of step 7.
- `cargo shot` of the Clip & transform, Tabs and Dock pages against a run before step 7.
