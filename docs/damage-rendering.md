# Damage-region rendering

Stage 3 from `incremental-rendering.md`. Stage 1 (frame skipping)
answers *should we render this frame at all?*. Stage 3 answers *how
much of the surface should we touch when we do?* — and is now
implemented end-to-end. This doc describes the shipped system.

## TL;DR

Per frame, after layout:

1. Hash every node's authoring inputs (`Tree.hashes`).
2. Compute screen-space rect for every node (`cascade_row.transform.apply_rect(layout.rect)`).
3. Diff `(rect, hash)` against last frame's snapshot keyed by `WidgetId`.
4. Union dirty contributions into one damage rect (or escalate to full repaint if >50% surface).
5. Encoder skips paint commands for nodes whose screen rect doesn't intersect damage.
6. Backend `LoadOp::Load`s the persistent backbuffer, scissors to damage rect, paints, then `copy_texture_to_texture` onto the swapchain.

Idle frames: zero CPU/GPU (Stage 1 gate). Hover-edge frames: a few
hundred bytes of dirty-set bookkeeping plus a scissored partial paint.
Large changes: 50%-area heuristic falls back to full repaint.

## What "damaged" means

A node is **dirty this frame** if any of:

1. Its **authoring data changed** — what the user wrote into widget
   builders differs from last frame. Detected by hashing.
2. Its **screen-space rect changed** — same authoring, but a sibling
   or ancestor reflowed/transformed us. Detected by rect comparison.
3. It **appeared** this frame (no entry in last frame's snapshot).
4. It **disappeared** this frame (entry in prev, not in curr).

The damage rect = union of all dirty nodes' prev+curr screen rects,
plus removed widgets' prev rects. Empty = nothing to do.

**Screen space, not layout space.** Damage is tracked in screen
coordinates (post-ancestor-transform, pre-DPR-scale). This is the
key correctness property: when a parent's transform changes, child
*layout* rects don't move, but child *screen* rects do — and that's
where the GPU paints. Comparing screen rects catches it; comparing
layout rects misses it.

## Where each piece lives

| Concern | Code |
|---|---|
| Authoring hash | `src/tree/hash.rs`, `Tree.hashes` (typed `NodeHash`) |
| Per-node cascade transform | `src/cascade.rs`, `Cascade.transform` |
| Prev-frame snapshot | `Damage.prev: FxHashMap<WidgetId, NodeSnapshot>` |
| Damage diff + filter | `src/ui/damage/`, `Damage::compute` (returns `Option<Rect>`) |
| Removed-widget diff | `src/ui/seen_ids.rs`, `SeenIds::removed() -> &[WidgetId]` |
| Encoder filter | `src/renderer/frontend/encoder/mod.rs`, `damage_filter` param |
| Backbuffer + scissor | `src/renderer/backend/mod.rs`, `Backbuffer` + `submit(damage)` |
| Public output | `FrameOutput.damage` returned from `Ui::end_frame` |

## Authoring hash — what's in, what's out

Hash captures inputs that affect **this node's own paint, given a
fixed rect**. Layout-affecting inputs are caught downstream by rect
comparison.

In:
- `LayoutCore` (mode, size, padding, margin, align, visibility) — yes,
  even though most affect layout. Cheap to include and pins layout
  drifts that produce identical rects (e.g., padding flip with no
  size change).
- `PaintCore.attrs` (sense, disabled, clip — packed byte).
- `ElementExtras` minus `transform`: position, grid cell, min/max
  size, gap, line gap, justify, child align.
- All shapes (RoundedRect fill/stroke/radius, Text content/color/
  size/wrap/align, Line endpoints/width/color).
- `GridDef` for grid panels.

Out:
- **`transform`** — only affects descendants' positions, never self's
  paint. Including it would over-dirty the parent on every animation
  frame; descendants are already caught by their screen-rect compare
  via `Cascades`.
- `WidgetId` — that's the *key*, not a value.
- `NodeId` — not stable across frames.
- Layout output (rect, desired) — captured separately as the rect
  signal.
- Children — each child has its own hash; the dirty set is per-node,
  not subtree-aggregated.

Hash is `FxHash` (64 bits). Collision probability across a 100-node
tree is ~2⁻²⁵ per frame — effectively never. Computed once per frame
in a single batch pass after recording (`Tree::compute_hashes`),
~10 µs for a 100-node tree.

## Snapshot map

```rust
struct NodeSnapshot { rect: Rect, hash: NodeHash }  // rect in screen space
Damage.prev: FxHashMap<WidgetId, NodeSnapshot>
```

Rolled forward in-place by `Damage::compute`: the diff loop reads
each `WidgetId`'s old entry via `self.prev.insert(wid, curr_snap)`
(which returns the previous value, if any) and writes the new one
in the same step. Removed widgets are evicted in a follow-up pass
that iterates the precomputed removed list. Capacity retained;
steady-state frames don't allocate.

## Damage compute

`Damage::compute(&mut self, tree, cascades, removed: &[WidgetId],
surface) -> Option<Rect>` rolls `self.prev` forward to this frame's
snapshot and returns the filtered damage rect in one call. The diff
reads each `WidgetId`'s old entry and writes the new one via
`self.prev.insert(wid, curr)`:

```text
screen_rect = cascade_rows[i].screen_rect   // cached on Cascade
curr_snap   = NodeSnapshot { rect: screen_rect, hash: tree.hashes[i] }
match prev.insert(wid, curr_snap):
    None                                     → added: damage |= screen_rect
    Some(snap) if snap matches               → clean
    Some(snap)                               → dirty: damage |= snap.rect ∪ screen_rect
```

Then walk the supplied `removed` slice; each disappeared widget's
last-known rect contributes to damage and its entry is dropped from
`prev`:

```text
for wid in removed:
    if let Some(snap) = prev.remove(wid):
        damage |= snap.rect
```

`removed` is precomputed by [`SeenIds::end_frame`](#sweepers-share-the-seenids-diff)
and shared with `TextMeasurer::sweep_removed` so neither consumer
walks `seen_ids` independently.

After the loop, the same call applies the 50% surface-area heuristic
and returns `Some(rect)` for partial repaint or `None` for full.
That used to live on a separate `Damage::filter(surface)` method;
the function is still present (`pub(crate)`) for tests but the
production call sequence is `compute → returned damage → consumed by
frontend + backend`. The call sits at the natural submit boundary
inside `Ui::end_frame`, so it's effectively still the lazy decision —
just no longer split across two methods.

`Cascade.screen_rect` is the layout rect projected through ancestor
transforms; it's filled in `Cascades::rebuild` and shared by encoder,
hit-index, and damage so the four passes can't disagree about where a
node lives in screen space.

### Sweepers share the SeenIds diff

`SeenIds` (`src/ui/seen_ids.rs`) is the per-frame `WidgetId` tracker
that owns:

- collision detection (`record(id) -> bool` for `Ui::node`),
- frame rollover (`begin_frame` swaps `curr ↔ prev` and clears
  `curr` — no clone),
- the removed-widget diff produced at `end_frame` and read via
  `removed() -> &[WidgetId]`.

Both `Damage::compute` and `TextMeasurer::sweep_removed` (Layer A of
the [text-reshape-skip](text-reshape-skip.md) work) consume the same
slice. Without this unification, each consumer would walk
`seen_ids` against its own map.

## Encoder filter

`encode(...)` and `Frontend::build(...)` accept `damage_filter: Option<Rect>`.
`None` = paint everything; `Some(rect)` = filter.

Per node:

```rust
let screen_rect = cascades.rows()[id].transform.apply_rect(layout.rect(id));
let paints = damage_filter.is_none_or(|d| screen_rect.intersects(d));
if paints {
    // emit DrawRect / DrawText for shapes
}
// always emit PushClip/PopClip and PushTransform/PopTransform regardless
```

Why always emit clip/transform pairs even when filtering: composer
groups and child transforms depend on them. Skipping a clipped
parent's `PushClip` would corrupt the scissor stack for unfiltered
descendants.

## Backend backbuffer + scissor

`WgpuBackend.backbuffer: Option<Backbuffer>` is lazily (re)created
when the surface texture's size or format changes. Each frame:

1. Ensure backbuffer matches surface (recreated this frame? remember it).
2. If `damage` is `Some` and not just-recreated → `LoadOp::Load`,
   convert logical damage to physical-px scissor (with
   `DAMAGE_AA_PADDING = 2` px on each side, clamped to surface).
3. If `damage` is `None` (or backbuffer recreated) → `LoadOp::Clear`.
4. Render to backbuffer with per-group scissor = `group.scissor.clamp_to(damage_scissor)`.
5. After the pass, `copy_texture_to_texture(backbuffer → surface_tex)`.
6. Submit. Host calls `frame.present()`.

The persistent backbuffer is what makes `LoadOp::Load` reliable —
the swapchain's preserve-contents behaviour varies by platform/
present-mode and can't be trusted for cross-frame state. We pay
~8 MB VRAM (1080p×4) plus ~0.1–0.3 ms/frame for the copy in exchange
for the ability to skip the bulk of paint work on partial-repaint
frames.

`copy_texture_to_texture` requires `COPY_DST` on the surface usage —
hosts must include it in `wgpu::SurfaceConfiguration`.

## Host integration

`Ui::end_frame` runs the entire CPU pipeline (layout, cascades,
input, damage, encode, compose) and returns a `FrameOutput` with the
composed buffer plus the filtered damage rect:

```rust
let frame_out = ui.end_frame();          // FrameOutput<'_>
backend.submit(
    &frame.texture,
    clear_color,
    frame_out.buffer,
    frame_out.damage,                    // backend scissor + LoadOp
);
```

The encoder reads the same `damage` value internally during
`Frontend::build`. Hosts can't desynchronize the encoder filter from
the backend scissor because there's only one carrier
(`FrameOutput.damage`).

## Edge cases (and how they're handled)

| Case | Handling |
|---|---|
| First frame | `Damage.prev` empty → all nodes "added" → damage = full surface → heuristic fires → full repaint. |
| Backbuffer (re)created | `submit` forces `damage = None` that frame (undefined contents). |
| Window resize | Surface size changes → backbuffer recreated → forced full repaint. |
| Hover/press fill change | Button shape's fill differs → hash differs → button dirty → damage = button rect. |
| Sibling reflow | Authoring unchanged on neighbour, but its screen rect shifts → caught by rect compare. |
| Animated parent transform | Parent's hash unchanged (transform omitted from hash), parent's screen rect unchanged. Children's screen rects DO change (cascade.transform composed) → children dirty by rect compare. Damage unions prev+curr child positions. |
| Empty UI | `Ui::layout` no-ops on empty tree; `Damage::compute` walks 0 nodes; `Damage.prev` stays empty; nothing to repaint. |
| Cosmic atlas growth | Glyphs are inserted, not relocated; existing UV coords stay valid. No interaction with damage. |
| Hash collision | 2⁻³² probability per pair → effectively never. Acceptable. |

## What's deliberately NOT done

- **Pre-clipping in widget code.** Widgets stay oblivious to damage.
  Filtering happens in encoder + backend. Keeping widgets pure is
  load-bearing.
- **Predicting which input flipped which widget.** Stage 1 runs the
  full record on input; the hash diff is the source of truth.
- **Unifying Stage 1 and Stage 3 gates.** Stage 1's `should_repaint`
  is checked *before* recording; Stage 3's dirty set is computed
  *after* recording. Different decision points.
- **Multi-rect damage.** One union rect; large unions trip the
  heuristic. Browsers can submit multiple rects to the compositor;
  wgpu can't anyway.
- **Layer caches** (Flutter-style per-subtree offscreen RTs).
  Different beast; only worth it for animation-heavy scenarios.
- **Runtime "damage on/off" knob in the library.** Hosts pass `None`
  to disable filtering. The backbuffer cost is structural; accepted.

## Decisions log

Preserved here so they survive context loss:

- **Damage lives on `Ui`, not `LayoutEngine`.** Layout's job is
  "constraints → rects"; damage is "diff against history." Same
  lifecycle as `Cascades` (rebuilt at end_frame), same readers.
- **Hash computed in a single batch pass** rather than incrementally
  during `push_node`/`add_shape`. Cleaner, decoupled from recorder
  hot path.
- **Heuristic merged into `Damage::compute(... surface)`.** Earlier
  the threshold was computed during `Damage::compute` and cached as
  `full_repaint: bool`; later it became a separate
  `Damage::filter(surface)` callable lazily at submit time; the
  current shape passes `surface` straight into `compute`, which
  returns the filtered `Option<Rect>`. The decision still happens at
  submit-time semantically (compute is the last `end_frame` step
  before encode/compose), just without the two-method indirection.
  `filter` is still present as a `pub(crate)` helper for tests.
- **Surface is `Ui`-owned via `Display`.** `Ui::begin_frame(display)`
  installs the surface; `Ui::end_frame` reads `self.display
  .logical_rect()` and threads it into `Damage::compute`. The host
  doesn't carry the surface through the rendering API anymore — it
  hands it to `begin_frame` and reads `FrameOutput` back.
- **No runtime damage toggle in the library.** Hosts pass `None` to
  disable filtering; backbuffer cost is structural.
- **`copy_texture_to_texture`, not a blit pipeline.** Backbuffer
  matches surface format and size, so direct GPU memory copy works.
- **`DAMAGE_AA_PADDING = 2`** physical px. SDF AA on rounded rects
  bleeds ~1 px outside the rect; glyph descenders/italics bleed a
  few px. 2 is conservative without being wasteful.
- **Damage is screen-space, not layout-space.** Catches transformed
  subtree movement; matches where the GPU actually paints.
- **`transform` omitted from authoring hash.** It only affects
  descendants; including it would over-dirty the parent. Descendants
  are caught by their screen-rect comparison.

## Acceptance bar (what shipped meets)

- ✅ Idle showcase: zero per-frame work (Stage 1 gate; Stage 3 doesn't
  regress).
- ✅ Hover-over-button: damage rect = button rect, partial repaint.
- ✅ First frame, resize, format change: forced full repaint, no
  artifacts.
- ✅ Animated parent transform: descendants dirty by rect compare,
  damage unions old+new positions.
- ✅ Full test suite passing (213+ tests at last count) including
  hover/un-hover, transformed-child, animated-transform, empty-UI,
  intersect/union, scissor.

## Future work

Out of scope for the current shipped Stage 3, but plausible next
steps if a workload demands them.

### Wanted (identity-based reuse)

These all need the per-node dirty *set* (`Damage.dirty: Vec<NodeId>`),
not just the union rect. The current rect filter does spatial culling
("is this node inside the dirty region?"); identity gives "did *this
specific node* change?" — strictly more information.

- **Per-node `RenderCmd` cache.** The encoder re-walks every visible
  node's shapes every frame: `align_text_in`, `dim_rgb`, stroke
  composition, etc. — even on clean nodes. Cache the emitted commands
  per `NodeId`; on a clean node whose ancestor cascade (transform/clip/
  disabled/invisible) is also unchanged, replay the cached slice
  instead of re-encoding. Invalidation key: `(authoring hash, cascade
  row)`. Saves CPU on every partial-repaint frame, on top of what the
  rect filter already saves on the GPU.
- **~~Skip cosmic-text reshape for clean Text nodes.~~ Shipped (Layer A).**
  Per-`WidgetId` reuse cache lives on `TextMeasurer.reuse`, keyed by
  identity, validity-checked by `NodeHash`. Sweep tied to
  `SeenIds.removed()`. See [`text-reshape-skip.md`](text-reshape-skip.md)
  for the final design. Layer B (eviction of the underlying
  `CosmicMeasure.cache` shaped-buffer table) is still pending; the
  reshape-skip doc tracks it.
- **Multi-rect damage.** Two unrelated regions changing (top-left +
  bottom-right) currently unions to ~the whole screen and trips the
  50% heuristic → full repaint. Cluster the per-node dirty rects into
  N disjoint regions; encoder filter accepts a slice of rects, backend
  sets multiple scissors (or splits composer groups). Complexity high,
  payoff workload-dependent — defer until the heuristic visibly fires
  on something users care about.
- **Incremental hit-index rebuild.** `HitIndex::rebuild` walks every
  node every frame. With the dirty set, only update entries for dirty
  nodes (and any whose cascade row changed). Modest CPU win; mostly
  matters once node counts get into the thousands.
- **Debug overlay.** Toggleable mode that flashes dirty nodes in
  red and draws the damage rect outline. Trivial to add once the
  per-node list is consumed; very useful for tuning the other items
  on this list.

### Other deferred work

- **Layer caches** (per-subtree offscreen RTs for animation-heavy
  scenarios). Months of work, separate from this stage. Strictly more
  powerful than the per-node command cache above but also vastly more
  invasive — only worth it if a real animation workload pushes the
  command cache past its limits.
- **Tighter damage on parent-transform animation.** Currently the
  damage rect is the union of every descendant's prev+curr screen
  rects, which can be large for deep transformed subtrees. A
  dedicated transform-cascade pass could collapse to a tight bounds
  if profiling ever shows it matters.
- **Fuse `compute_hashes` into `Cascades::rebuild`.** Both walk every
  node once; combining saves one SoA pass. ~10 µs savings on a
  100-node tree, only worth it if the trace ever shows it's hot.
