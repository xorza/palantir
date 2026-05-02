# Damage-region rendering — investigation

Stage 3 from `incremental-rendering.md`. Stage 1 (frame skipping)
answers *should we render this frame at all?*. Stage 3 answers *how
much of the surface should we touch when we do?*.

## What "damaged" means in our architecture

A node is **dirty this frame** if any of:

1. Its **authoring data changed** — what the user wrote into widget
   builders this frame differs from last frame. Detected by hashing.
2. Its **arranged rect changed** — same authoring, but a sibling or
   ancestor's reflow moved/resized us. Detected by rect comparison.
3. It **appeared** this frame (no entry in last frame's state).
4. It **disappeared** this frame (was in last frame, not in this).

Any of those four must contribute to the damage rect: appeared and
moved nodes need painting at their *new* position; disappeared and
moved nodes need erasing from their *old* position.

The damage rect for a frame = union of all dirty nodes' (prev_rect,
curr_rect). Empty = nothing to do (Stage 1 already handles that).

## The three pieces

### A. Per-node change detection

Two signals per node, both compared frame-to-frame:

**A1. Authoring hash.** During recording (`Tree::push_node` +
`Tree::add_shape`), accumulate a u64 hash over:

- `LayoutCore` (size, padding, margin, align, visibility) — copy types.
- `PaintCore.attrs` (sense / disabled / clip — 1 byte).
- `ElementExtras` if present (transform, position, grid, gap, line_gap,
  justify, child_align, min/max size).
- All shapes attached: `RoundedRect { radius, fill, stroke }`,
  `Text { text, color, font_size_px, wrap, align }`, `Line { ... }`.

What **NOT** to hash:
- `WidgetId` — that's the *key*, not a value. We want changes to a
  same-keyed widget to register as a hash diff.
- `NodeId` — not stable across frames (tree rebuilt).
- Layout output (rect, desired) — output of layout, not authoring.
  Captured separately as the rect-comparison signal (A2).
- Children — each child has its own hash entry. Subtree-aggregated
  hashing isn't needed; the dirty-set is per-node.

Storage: a new `Vec<u64>` column on `Tree`, indexed by `NodeId`,
filled during `push_node` / `add_shape`. ~50 ns per node to compute
(FxHash is fast); ~10 µs total for a 100-node tree.

**A2. Rect comparison.** After layout, compare each node's current
rect to its previous rect. Drives the "sibling reflow moved me"
case where authoring is unchanged.

### B. Damage rect computation

After `end_frame` recording but before encoder runs:

```text
prev: HashMap<WidgetId, (Rect, u64)>      // persistent across frames
curr: per-NodeId hashes + WidgetIds + rects (current frame)

for each curr node:
    match prev.get(widget_id) {
        Some((prev_rect, prev_hash))
            if curr_hash == prev_hash && curr_rect == prev_rect
                -> not dirty
        Some((prev_rect, _))
            -> dirty, damage |= union(prev_rect, curr_rect)
        None
            -> dirty (added), damage |= curr_rect
    }

for each (widget_id, (prev_rect, _)) in prev:
    if not seen in curr:
        -> removed, damage |= prev_rect

prev = current's (widget_id -> (rect, hash)) map
```

**Heuristic fallback.** If `damage.area() > THRESHOLD * surface.area()`,
discard damage and do full repaint — the bookkeeping cost exceeds the
saving. Threshold ~50%. Same trick LVGL uses with `LV_INV_BUF_SIZE`.

**Padding for anti-aliasing.** SDF-AA quads bleed ~1 px outside their
rect; cosmic glyphs can extend a few px outside the text rect for
italics/descenders. Inflate damage by 2–3 px on all sides to be safe.

Storage: `HashMap<WidgetId, (Rect, u64)>` on `Ui`. Capacity retained
across frames. For 100 nodes: ~32 B/entry × 100 ≈ 3 KB.

### C. Pipeline filtering

Three places save work:

**C1. Encoder filter.** When walking the tree, if a node's rect
doesn't intersect damage **and** the node has no shapes that could
overflow (transform / negative margin / clip), skip emitting its
DrawRect/DrawText.

Simpler v1 rule: **always emit clip + transform Push/Pop** (so
descendant scissor/transform stays correct), but skip emitting
**leaf paint commands** (DrawRect, DrawText) for non-intersecting
nodes. Loses some saving on subtrees of pure containers; safe for
MVP.

**C2. Composer scissor.** Composer already wraps draws in scissor
groups. Backend pre-pass sets the outermost scissor to the damage
rect (intersected with each group's existing scissor so clips still
work).

**C3. Backend `LoadOp::Load` + persistent backbuffer.** This is
the structural change.

wgpu's swapchain surface doesn't reliably preserve contents across
frames (varies by platform). To do `LoadOp::Load` that means
"keep last frame's pixels," we maintain our **own** persistent
texture (`last_frame_tex`):

```text
each frame:
    1. Begin render pass on last_frame_tex with LoadOp::Load
    2. Set scissor to (intersected) damage rect
    3. Draw the filtered command stream
    4. End pass
    5. Copy last_frame_tex -> swapchain surface
    6. Present
```

Cost: one extra surface-sized texture (e.g. 1920×1080×4 = 8 MB),
plus a copy per frame (~0.5–1 ms on most GPUs).

The copy itself eats into the savings. Trade-off:
- Tiny damage (a button hover): copy cost dominates the actual
  paint cost, but **system-level GPU memory bandwidth** is well
  under what a full surface paint + present would be.
- Medium damage (a counter ticking): clear win.
- Large damage (>50% surface): heuristic falls back to full repaint
  → no copy, no scissor, regular path.

## Risks and edge cases

### Hash collisions

64-bit hash → ~2⁻³² collision probability per pair. For 100 nodes,
collision probability across the tree is ~2⁻²⁵ per frame → effectively
never. Acceptable.

### Layout cascade

A change at the top of the tree (e.g., padding on the root) cascades
down: all descendant rects change. Every node becomes dirty by rule
A2 (rect comparison). Damage rect = union of every rect = full
surface. Heuristic fallback triggers → full repaint. Correct
behavior.

### Animations through transforms

If a parent has `.transform(animated_translate)`, the parent's hash
changes (transform is in authoring). Child rects don't change (we
arrange in untransformed space; transform applies post-layout). So
only the parent is dirty by hash, but the *rendered* position of all
descendants changes. Damage rect = parent's rect — but children
paint outside the parent's untransformed rect under the transform.

**Fix:** when a node's transform changed, mark its entire subtree as
dirty for damage purposes (transform composes into children's
screen-space rects).

Or: cascade tracking — if a node's authoring hash changed AND the
change affects descendant rendering (transform, clip), recursively
add descendants' rects to damage.

For MVP: detect transform-change on a node → fall back to full
repaint. Conservative but correct. Refine later.

### Hover/press visual changes (no rect change)

A button hovers → `Visuals` switch from normal to hovered → fill
color differs → `Shape::RoundedRect.fill` differs → hash differs →
button is dirty → damage = button's rect. Exactly what we want.
No layout change needed.

### Text z-order edge

We have per-group text rendering with the composer splitting
groups on text→quad transition (recall the showcase z-order fix).
The encoder's filter must not break this — when we skip emitting
shapes for a non-dirty node, we still need to preserve the group
boundaries so dirty siblings render in the right z-order.

Safest rule: **skip leaf shape emission, never skip clip/transform
push-pop**. Group boundaries are scissor-driven (`PushClip`); as
long as those are emitted, composer creates the same groups it
would otherwise.

### First frame

`prev` is empty. Every current node is "added." Damage = full
surface. Heuristic falls back to full repaint. Correct.

### Resize

Surface size changes → `last_frame_tex` is the wrong size → must
be recreated. Detect on frame entry; recreate + force full repaint
on the resize frame. Subsequent frames work normally.

### Cosmic atlas changes

If text shaping produces glyphs that aren't in the atlas yet, the
atlas grows. Are previously-rendered atlas slots still valid?
Looking at `glyphon::TextAtlas`: glyphs are inserted, not
relocated. Existing UV coords stay valid. So damage-region
rendering doesn't conflict with atlas lifecycle.

### Pixel snap and DPI

Damage rect is in physical px (post-scale, post-snap). Compute it
*after* the layout pass and *after* the per-axis pixel-snap
transformation. Then intersect with each draw group's scissor.

## What NOT to do (deliberately)

- **Don't pre-clip widget paint in the widget code.** Widgets stay
  oblivious to damage. The encoder filters, not the widget. Keeping
  widgets pure is a load-bearing property.
- **Don't try to track which input event flipped which widget's
  state.** Stage 1 already runs the full record on input; the hash
  diff is the source of truth for "what changed." We never try to
  predict it.
- **Don't unify Stage 1 and Stage 3 gates.** Stage 1's
  `should_repaint` is a fast bool checked *before* recording;
  Stage 3's dirty set is computed *after* recording. Different
  decision points, keep them separate.
- **Don't ship Stage 3 without the heuristic fallback.** Without it,
  large-damage frames pay all the bookkeeping with none of the
  saving.

## Integration plan (current state — 2026-05-02)

What's landed (Steps 1–5 + 6a + 6b):

- **Step 1 — Tree authoring hash.** `Tree.hashes: Vec<u64>`, populated by
  `Tree::compute_hashes()` from `(LayoutCore, PaintCore, ElementExtras,
  shapes, GridDef)` per node. Lives in `src/tree/hash.rs`. ✅
- **Step 2 — `Ui.prev_frame: FxHashMap<WidgetId, NodeSnapshot>`.**
  Each `NodeSnapshot = { rect, hash }`. Rebuilt at the tail of
  `Ui::end_frame()` after `compute_hashes`, before damage compute.
  ✅
- **Step 3 — `Ui.damage: Damage`.** `Damage { dirty: Vec<NodeId>,
  rect: Option<Rect>, full_repaint: bool }`. Computed in
  `Ui::end_frame()` *before* `rebuild_prev_frame` so it reads
  last-frame snapshots. Lives in `src/ui/damage/`. ✅
- **Step 4 — `needs_full_repaint(damage, surface)`.** 50%-area
  threshold via `FULL_REPAINT_THRESHOLD = 0.5`. Sets
  `Damage.full_repaint`. ✅
- **Step 5 — Encoder filter.** `encode(...)` and `Pipeline::build(...)`
  gain `damage_filter: Option<Rect>`. Per-node, skips
  `DrawRect`/`DrawText` emission when the node's rect doesn't
  intersect the filter. Always emits `Push/PopClip` and
  `Push/PopTransform` for group/transform coherence. ✅
- **Step 6a — Persistent backbuffer.** `WgpuBackend.backbuffer:
  Option<Backbuffer>`, lazily (re)created on submit when
  `surface_tex.size()` or `.format()` changes. `submit()` renders to
  backbuffer, then `copy_texture_to_texture` onto the swapchain.
  Examples: `usage |= COPY_DST`, pass `&frame.texture`. ✅
- **Step 6b — Damage scissor + `LoadOp::Load`.** `submit()` now
  takes `damage: Option<Rect>` (logical px). On `Some`, sets
  `LoadOp::Load`, converts to physical-px scissor with
  `DAMAGE_AA_PADDING = 2`, and intersects with every group's
  scissor (skipping groups that fall outside damage entirely).
  Forced to `None` on the frame after `ensure_backbuffer` recreates
  (undefined contents). ✅

What's left:

- **Step 6c — Wire `ui.damage` end-to-end in examples.** Both
  helloworld and showcase currently pass `None` to both
  `Pipeline::build` (filter) and `backend.submit` (damage). The
  flip:
  ```rust
  let damage_logical = if ui.damage.full_repaint {
      None
  } else {
      ui.damage.rect
  };
  let buffer = pipeline.build(..., damage_logical, &compose_params);
  backend.submit(&frame.texture, clear, buffer, damage_logical);
  ```
  Both `damage_filter` and `damage` need the *same* rect: the
  encoder culls work, the backend scissors pixels. Disagreement
  paints outside the scissor (wasted) or skips inside it (gaps).
  After this lands, hover-over-button on showcase should partial-
  repaint just the button. ~10 LOC across two examples.
  Manual visual verification required.
- **Step 7 — Conservative cascade for transforms.** When a node's
  `ElementExtras.transform` differs from prev, mark its entire
  subtree dirty (their screen-space rects move under the new
  transform even though their authoring hashes are unchanged).
  Simplest first cut: detect transform diff at the top of
  `Damage::compute`'s per-node loop; if found, walk
  `subtree_end[i]` and mark every descendant. Or escalate to
  `full_repaint = true` for an MVP. ~30 LOC.

Decisions made along the way (preserved here so they survive a
session reboot):

- **Damage lives on `Ui`, not `LayoutEngine`.** Layout's job is
  "constraints → rects"; damage is "diff against history." Same
  lifecycle as `Cascades` (rebuilt at end_frame), same readers
  (encoder), no cross-frame state in layout.
- **Hash is computed in a single batch pass** (`Tree::compute_hashes`)
  rather than incrementally during `push_node`/`add_shape`. Cleaner,
  decoupled from recorder hot path; ~10 µs for 100 nodes is dwarfed
  by layout/encode/submit.
- **`needs_full_repaint` is a separate function**, called by
  `Damage::compute` to set the `full_repaint` bool. Both the helper
  and the field are reachable from production.
- **`Ui.surface: Rect`** is stored from the last `layout()` call.
  Used by damage heuristic; future Step 6 backbuffer-resize
  detection could also read it (currently it reads from the wgpu
  texture instead).
- **No runtime "damage on/off" knob** in the library. The host
  passes `None` if it wants to disable filtering. Backbuffer cost
  is structural — accepted (~8 MB per surface).
- **`copy_texture_to_texture`, not a blit pipeline.** Backbuffer
  matches surface format and size, so direct GPU memory copy works.
  Adds ~0.2 ms per frame on desktop GPUs.
- **`DAMAGE_AA_PADDING = 2`** physical px. SDF AA on rounded rects
  bleeds ~1 px outside the rect; glyph descenders/italics bleed a
  few px. 2 px is conservative without being wasteful.

Acceptance bar for shipping the whole thing (Step 6c done +
Step 7 done):

- Idle showcase: zero frames per second (Stage 1 already covers
  this; Stage 3 must not regress).
- Hover-over-button on showcase: damage rect ≈ button rect, NOT
  full surface. Visually: surrounding area unchanged across
  hover-edge frames (no flicker).
- Click-tab transition: damage = top-toolbar union + center-panel
  swap. Two regions unioned; partial repaint or full-repaint
  fallback both acceptable depending on size.
- Window resize: forced full repaint (backbuffer recreated), no
  artifacts.
- Animated transform on a parent: every frame is full-repaint OR
  every descendant in damage (Step 7's call).

## Implementation steps

Each step is independently shippable and testable:

1. **Hash column on Tree.** Add `Vec<u64>` filled during
   `push_node` and `add_shape`. Verify hashes differ when authoring
   differs; same when authoring identical. ~150 LOC + tests.

2. **Persistent prev-map on Ui.** `HashMap<WidgetId, (Rect, u64)>`,
   updated at end of `end_frame`. ~50 LOC + tests.

3. **Dirty-set + damage-rect computation.** Pure function over
   (prev, curr_hashes, curr_rects, curr_widget_ids). ~80 LOC +
   tests for added/removed/changed/reflowed cases.

4. **Heuristic fallback.** If damage-area / surface-area > 0.5,
   return "full repaint." ~20 LOC + 1 test.

5. **Encoder filter.** Skip DrawRect/DrawText emission for
   non-intersecting nodes. Always emit Push/PopClip /
   Push/PopTransform. ~50 LOC + tests verifying filtered cmd
   stream.

6. **Backend persistent backbuffer + scissor.** Owned
   `wgpu::Texture` of surface size. Render pass uses it with
   `LoadOp::Load`. Copy to swapchain at present time. Recreate on
   resize. ~150 LOC + manual showcase verification.

7. **Conservative cascade for transforms.** If a node's transform
   changed, mark subtree dirty (or fall back to full repaint).
   ~30 LOC.

Total estimate: ~500–700 LOC across `Tree`, `Ui`, encoder, backend.
~10–15 new tests. Largest single change: step 6 (backend
restructuring).

## Tests we'd want (acceptance bar)

A Stage 3 implementation isn't trustworthy without these:

**Unit / non-GPU:**

- `hash_differs_when_fill_color_changes` — change one shape's fill,
  hash differs, that node enters dirty set.
- `hash_same_when_authoring_unchanged` — re-record identical UI, no
  nodes are dirty.
- `dirty_set_after_sibling_reflow` — change a Fixed-width sibling,
  same-row neighbors detect rect change → dirty.
- `removed_widget_contributes_prev_rect_to_damage`.
- `added_widget_contributes_curr_rect_to_damage`.
- `damage_threshold_falls_back_to_full_repaint` — synthesize a
  >50%-area damage, verify fallback path is selected.
- `transform_change_triggers_subtree_dirty` — animate a transform,
  every descendant ends up in damage (or full-repaint fallback
  fires).
- `encoder_skips_drawrect_for_non_intersecting_node`.
- `encoder_emits_clip_pushpop_even_when_subtree_is_clean` —
  preserves group boundaries.

**Integration / GPU (manual or capture-based):**

- Showcase running in idle state has zero per-frame work (Stage 1
  already covers this; Stage 3 must not regress).
- Cursor hover over a button: damage rect ≈ button's rect, NOT
  full surface.
- Click a tab: damage rect = top toolbar (button colors flip) +
  central panel (content swap). Two regions unioned.
- Window resize: full repaint, no incremental path attempted.

## Out of scope for now

- **Multi-rect damage.** Today we compute one rect = union of all
  dirty rects. If two unrelated things change in opposite corners,
  the union is the whole screen — fallback fires. CSS / browser
  engines can submit multiple damage rects to the compositor;
  wgpu can't anyway. Skip.
- **Layer caches** (Flutter-style per-subtree offscreen RTs).
  Months of work, only worth it for animation-heavy scenarios.
- **`will-change`-style author hints.** Useful for layer caching;
  not for our damage-rect approach.
- **Cross-frame text-shape diff.** Cosmic already caches; if a
  text run's hash matches last frame's, the shape is the same.
  No special handling needed.

## When to ship

Don't ship until:

1. Stage 1 has been live and we've confirmed idle frames cost zero.
2. A real motivating workload: animation, frequently-changing
   counter, hover-heavy UI. Without one, Stage 3's cost-benefit
   evaluation is speculative.
3. We've accepted the persistent-backbuffer memory cost (one extra
   surface-sized texture per `Ui`).

If steps 1–5 are easy to ship as a pure-CPU experiment (no
backend changes — just compute the damage and don't use it for
anything), it might be worth doing as a **measurement** to verify
the dirty-set numbers match expectations on real workloads, and
that hashing overhead doesn't dominate. If they do, Stage 3 is
moot regardless. If they don't, step 6 becomes the actual lift.
