# Questions from executing `.notes/PERF_REDESIGN.md`

## Q1. Read the last frame by position instead of by id

**Item:** C4, the per-widget lookups of last frame's data.

**Background.** Each widget's id reaches several independent hash maps in
every frame. After C1 (`SeenIds` in step with the last frame), the ones left
on `cached_cpu` are: `Ui::response_for` probing `Cascade::by_id` (3.2%),
`AnimMap::animate` (1.8%), `Cascade::is_within` (1.0%, two probes per call),
and the state map for stateful widgets. While a pass is in step, a widget's
entry in `SeenIds` has the same position as last frame's entry for the same
id, and that entry already holds last frame's endpoint. So these reads could
take last frame's row by position, and probe only out of step.

**Why it needs a decision.** It reaches the cascade, input and animation
subsystems, not only the id tracker. A two-pass frame needs care: during
pass B, `Cascade::by_id` holds pass A's rows, while `SeenIds::prev` holds the
last painted frame's, so a positional read must name which snapshot it
reads.

**Options.**

1. **Positional reads with a probe fallback.** `ResolvedId` carries the
   entry position. `response_for` and the look animation read last frame's
   row through it while in step and the cascade snapshot belongs to that
   frame, and probe otherwise. Estimated gain: 3–5% of `cached_cpu`.
2. **One slot per widget.** A stable dense slot index, kept across frames
   like a generational arena, that every per-widget store (cascade row,
   animation, state) indexes instead of hashing the id. The largest gain and
   the largest change.
3. **Leave the lookups as they are.**

**Recommendation:** option 1, after C2. It keeps every store as it is and
only adds a faster path, as C1 did.

**Blocked:** the positional part of C4. The other C4 items go on.

## Q2. A table for the shadow corner cutouts

**Item:** G3, the cutout cache.

**Background.** The shadow coverage is now the sharp box less four corner
cutouts (done, `scrolling_gpu` 7.33 → 6.05 ms). The cutout is one function
`U(q; r, σ)` for all four corners, zero more than `reach` from its `r`×`r`
square, so a table per `(r, σ)` could replace its 12 nodes with four texel
loads. The rest of the integral costs about 1.1 ms per full frame here
(6.05 ms against 4.91 ms before `e0500557`).

**Why it needs a decision.** The table needs new GPU structure, which
changes what a backend must support:

- a bake render pass before the main pass, with its own pipeline;
- an `R32Float` atlas texture as a render target, plus a second bind group
  (atlas and per-shadow table descriptors) on the shadow pipeline only.
  `R32Float` is color-renderable on Vulkan, Metal and Dx12, but on GLES 3.0
  only with `EXT_color_buffer_float`, and the crate supports GLES surfaces;
- a CPU mirror of the shader's spread and fit of the radii, to key the
  tables and point each shadow instance at its four.

**Options.**

1. **Bake every frame, no cache.** Each frame bakes the tables its shadows
   use, deduplicated by key, into a fixed atlas packed from scratch. About
   5 000 texels × 48 nodes per table, some 30× cheaper than shading the same
   corners analytically, so no frame pays more than another and there is no
   eviction or stability rule. A key whose table would pass a texel budget
   (small σ against a large radius), or an adapter that cannot render
   `R32Float`, keeps the analytic cutout. Precision: bake error about 10⁻⁴
   plus bilinear error, to be held under the current 1.1·10⁻³.
2. **A persistent cache** with frame stamps, a bake budget and a two-frame
   stability rule, as the plan first described. Less GPU work per frame,
   more state and more rules.
3. **Stop at the analytic cutout.**

**Recommendation:** option 1. It removes most of the remaining 1.1 ms with
a uniform per-frame cost and no cache state, and it keeps the analytic
cutout as the fallback for every case it does not cover.

**Blocked:** G3's table. The docs step goes on.
