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
| Authoring hash | `src/tree/hash.rs`, `Tree.hashes` |
| Per-node cascade transform | `src/cascade.rs`, `Cascade.transform` |
| Prev-frame snapshot | `Ui.prev_frame: FxHashMap<WidgetId, NodeSnapshot>` |
| Damage diff | `src/ui/damage/`, `Damage::compute` |
| Heuristic | `damage::needs_full_repaint` |
| Public host accessor | `Ui::damage_filter() -> Option<Rect>` |
| Encoder filter | `src/renderer/encoder/mod.rs`, `damage_filter` param |
| Backbuffer + scissor | `src/renderer/backend/mod.rs`, `Backbuffer` + `submit(damage)` |
| Surface size tracking | `Ui.surface: Rect` |

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
struct NodeSnapshot { rect: Rect, hash: u64 }  // rect in screen space
Ui.prev_frame: FxHashMap<WidgetId, NodeSnapshot>
```

Rebuilt at the tail of every `Ui::end_frame()` after the damage diff
runs (so `Damage::compute` sees the *previous* frame's snapshot).
Capacity retained; steady-state frames don't allocate.

## Damage compute

Pure function over `(tree, layout_result, cascades, prev_frame, curr_widget_ids, surface)`.
For each curr node:

```text
screen_rect = cascade_rows[i].transform.apply_rect(layout.rect(id))
match prev[wid]:
    None                                     → added: damage |= screen_rect
    Some(snap) if snap matches               → clean
    Some(snap)                               → dirty: damage |= snap.rect ∪ screen_rect
```

Then for each prev entry whose `wid` isn't in `curr_widget_ids`:

```text
removed: damage |= snap.rect
```

`curr_widget_ids` is reused from `Ui.seen_ids` (the per-frame
uniqueness set) — no extra hash set built.

Finally `full_repaint = needs_full_repaint(self, surface)` —
`damage.area() / surface.area() > 0.5`.

## Encoder filter

`encode(...)` and `Pipeline::build(...)` accept `damage_filter: Option<Rect>`.
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

```rust
let damage = ui.damage_filter();        // Some(rect) or None
let buffer = pipeline.build(
    ui.tree(),
    ui.layout_result(),
    ui.cascades(),
    ui.theme.disabled_dim,
    damage,                             // encoder filter
    &compose_params,
);
backend.submit(
    &frame.texture,
    clear_color,
    buffer,
    damage,                             // backend scissor + LoadOp
);
```

Same `damage` to both. Disagreement (filtering one, not the other)
either skips work that the other path expects (gaps) or paints
outside the scissor (wasted). One accessor (`Ui::damage_filter`)
keeps them in lockstep.

## Edge cases (and how they're handled)

| Case | Handling |
|---|---|
| First frame | `prev_frame` empty → all nodes "added" → damage = full surface → heuristic fires → full repaint. |
| Backbuffer (re)created | `submit` forces `damage = None` that frame (undefined contents). |
| Window resize | Surface size changes → backbuffer recreated → forced full repaint. |
| Hover/press fill change | Button shape's fill differs → hash differs → button dirty → damage = button rect. |
| Sibling reflow | Authoring unchanged on neighbour, but its screen rect shifts → caught by rect compare. |
| Animated parent transform | Parent's hash unchanged (transform omitted from hash), parent's screen rect unchanged. Children's screen rects DO change (cascade.transform composed) → children dirty by rect compare. Damage unions prev+curr child positions. |
| Empty UI | `Ui::layout` no-ops on empty tree; `Damage::compute` walks 0 nodes; `prev_frame` stays empty; nothing to repaint. |
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
- **`needs_full_repaint` is a separate function** called by
  `Damage::compute` to set the `full_repaint` bool. Both reachable
  from production.
- **`Ui.surface: Rect`** is stored from the last `layout()` call so
  damage and future backbuffer-resize logic share one source of
  truth.
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
- ✅ 206 tests passing including hover/un-hover, transformed-child,
  animated-transform, empty-UI, intersect/union, scissor.

## Future work

Out of scope for the current shipped Stage 3, but plausible next
steps if a workload demands them:

- **Multi-rect damage** (avoid full-screen heuristic when two
  unrelated regions change). Cost: rework the encoder filter to
  accept multiple rects, backend to set multiple scissors per group
  (or split groups). Complexity high, payoff workload-dependent.
- **Layer caches** (per-subtree offscreen RTs for animation-heavy
  scenarios). Months of work, separate from this stage.
- **Tighter damage on parent-transform animation.** Currently the
  damage rect is the union of every descendant's prev+curr screen
  rects, which can be large for deep transformed subtrees. A
  dedicated transform-cascade pass could collapse to a tight bounds
  if profiling ever shows it matters.
- **Fuse `compute_hashes` into `Cascades::rebuild`.** Both walk every
  node once; combining saves one SoA pass. ~10 µs savings on a
  100-node tree, only worth it if the trace ever shows it's hot.
