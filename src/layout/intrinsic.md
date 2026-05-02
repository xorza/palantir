# Intrinsics

On-demand `intrinsic(node, axis, req: LenReq) -> f32` queries with an
intra-frame cache, used by `Grid` and `Stack` drivers when a parent
needs to know how large a child *would* be on one axis without committing
to a measure pass at a specific available width.

Two patterns motivate the API:

- **Property-grid** — `Grid` with `Track::hug()` for the label column
  and `Track::fill()` for the value column, where the value column
  contains wrapping text. The Hug column hugs to the longest label; the
  Fill column gets the rest of the width and wraps text inside.
- **Chat message** — `HStack { Avatar (Fixed), Message (Fill, wrap) }`.
  Avatar takes its fixed width; the message claims the leftover and the
  wrapped text reshapes at that width, not at the natural unbroken
  width.

Both are pinned by tests in `src/widgets/tests.rs` and demonstrated in
the `text layouts` tab of `examples/showcase`.

## LenReq

```rust
pub enum LenReq {
    /// Smallest size the node can occupy without breaking. Text:
    /// longest unbreakable run.
    MinContent,
    /// Size the node "wants" with unlimited room. Text: natural
    /// unbroken width.
    MaxContent,
}
```

Each query is **per-axis**. Callers ask for one specific
`(axis, req)`; the orthogonal axis isn't computed.

For a leaf text shape:

- `intrinsic(text, X, MaxContent)` = natural unbroken width.
- `intrinsic(text, X, MinContent)` = longest-word width.
- `intrinsic(text, Y, _)`         = single-line height.

Height-given-width is intentionally absent — neither motivating pattern
needs it. See "Deferred" below.

## API

```rust
impl LayoutEngine {
    pub fn intrinsic(
        &mut self,
        tree: &Tree,
        node: NodeId,
        axis: Axis,
        req: LenReq,
        text: &mut TextMeasurer,
    ) -> f32;
}
```

Same shape as `measure`, dispatched per-driver. `compute()` in
`intrinsic.rs` is the cache-miss path: it applies the node's `Sizing`
override, then adds padding/margin and clamps to `min_size`/`max_size`
before delegating content sizing to the driver.

`Sizing` semantics in intrinsic context:

- `Fixed(v)` → `v` (content not queried).
- `Hug` → content's intrinsic.
- `Fill(_)` → content's intrinsic. **The Fill weight is ignored at
  query time** — it only matters when distributing leftover space at
  resolution time. Matches CSS Grid `1fr` semantics: a `1fr` track
  contributes its content's `min-content` to the base size and
  `max-content` to growth, not infinity and not zero.

## Per-driver behavior

- **Leaf.** `intrinsic.rs::leaf` walks the node's shapes. `Shape::Text`
  contributes via `TextMeasurer::measure(src, size, max_w = None)` —
  cosmic returns both `intrinsic_min` and natural width from one
  unbounded shape, cached on the cosmic side. Other shapes contribute
  zero (they paint relative to the owner's arranged rect, they don't
  drive size).
- **HStack / VStack on main axis.** Sum of children's intrinsic on that
  axis + `(n-1) * gap`.
- **HStack / VStack on cross axis.** Max of children's intrinsic on
  that axis.
- **ZStack / Canvas.** Max of children on the queried axis (Canvas
  also adds child positions, like its `measure`).
- **Grid.** Sum of resolved track sizes + gaps, where each track's range
  comes from `MinContent`/`MaxContent` queries to span-1 children
  (see "Grid Auto under constraint" below).

## Cache

`Vec<[f32; 4]>` on `LayoutEngine`, indexed by `node.index()` with one
slot per `(axis, req)` pair. NaN means "not yet computed". Resized to
`node_count` at the top of `run` (capacity retained, same pattern as
`desired`).

`engine.intrinsic()` checks the cache first, recurses on miss, stores
the result. The answer is a pure function of the subtree — it doesn't
depend on the parent's available width or the arranged rect — so
intra-frame caching is sound.

Cross-frame caching is **deferred**. Cosmic already caches text shapes
across frames keyed on content hash, which covers the expensive part of
leaf intrinsics for free. Container intrinsics are cheap arithmetic;
re-running them per frame is fine until profiles say otherwise. When the
persistent `Id → Any` state map lands (CLAUDE.md §Status), revisit
keying on `WidgetId` plus a content/topology hash, which is the model
Yoga/Taffy use.

## Grid `Auto` track sizing under constraint

Implemented in `grid::measure`. Two-phase resolution:

1. **Per-track range** — for each `Hug` track, query span-1 cells:
   - `track.min = max(t.min, max over cells of intrinsic(MinContent))`
   - `track.max = min(t.max, max over cells of intrinsic(MaxContent))`
   - `Fixed(v)` and `Fill(_)` track ranges come from their `Sizing`
     directly; `t.min`/`t.max` clamps still apply.
2. **Distribute available space.**
   - `available >= sum(track.max) + gaps`: each Hug track gets
     `track.max`. Leftover goes to Fill tracks proportionally to weight.
   - `available <= sum(track.min) + gaps`: each track gets `track.min`.
     Grid overflows the slot.
   - Otherwise: each track starts at `track.min` and grows toward
     `track.max` proportionally to its `(track.max - track.min)` slack
     until the sum equals available.

Then cells are measured with their resolved column widths so wrap text
shapes correctly. Row heights resolve from the resulting cell desired
heights.

**Span > 1.** Span-1 cells contribute to track sizing; span > 1 cells
consume whatever the spanned tracks already resolved to. Avoids the
WPF cyclic-iteration trap. User-visible gotcha: a span-N cell with no
span-1 cells in any of its tracks resolves to zero width on those
tracks. Workaround: add explicit `Track.min` or include a span-1 cell.

**Hug-grid + Fill-column gotcha.** A grid that itself sizes as
`Sizing::Hug` on an axis has no available width to distribute on that
axis, so Fill columns collapse to their min. Same rule CSS Grid
follows for `display: grid; width: auto; grid-template-columns: 1fr`.
To get a Fill column to actually fill, the grid must be `Fixed` or
`Fill` on that axis. Pinned by
`hug_grid_fill_col_does_not_grow_row_height_on_horizontal_resize`.

## Stack `Fill` resolved during measure

Implemented in `stack::measure`. Two-pass:

1. **First pass** — measure every child with `available.main = INFINITY`
   (the WPF intrinsic trick). Children report their natural main size.
2. **Second pass** — only if the stack itself has a finite main-axis
   size *and* there are Fill children: re-measure each Fill child at
   its resolved Fill share, clamped to
   `[intrinsic(MinContent), max_size]`.

Wrap text in Fill children reshapes via the existing `shape_text`
reshape branch because the second-pass available width is smaller than
the natural width.

Hug stacks (main-axis available is `INF`) skip the second pass — Fill
children stay at natural width as before, matching the existing
"Hug stack hugs to children's natural widths" rule.

The Fill distribution itself stops at:

- **Min floor** — clamped to `MinContent` per child so wrap text never
  breaks inside a word (it overflows the slot instead).
- **Weight** — `Sizing::Fill(w)` weights split leftover.
- **Max-size clamp** — `Element.max_size` honored.

Anything richer (`flex-basis`, `flex-shrink` distinct from `flex-grow`,
`align-items: baseline`, `flex-wrap`, `align-content`) is **out of
scope** — see "Future direction".

## Why on-demand instead of a pre-pass

Yoga and Taffy use on-demand intrinsic queries with caching; Flutter
uses a separate intrinsic API that is widely cited as its O(n²) slow
path. We picked the on-demand model because:

- It only computes intrinsics for the subtrees that drivers actually
  query. A frame with no Hug grids or Fill stacks pays zero cost.
- The cache key is just `(NodeId, Axis, LenReq)` — no need for a
  `(known_dim, available)` tuple, because the answer is independent of
  parent context.
- Cosmic's existing cross-frame text-shape cache already covers the
  expensive leaf side.

The algorithm is **forward**: drivers either query intrinsic and
then measure at the resolved size (Grid Phase-1 col resolution,
Stack pass-2 Fill), or measure at `INF` on the queried axis to
get the child's natural answer at the committed cross (Stack
pass-1 main, ZStack/Canvas Hug axes — see "Height-given-width"
below). Either way, no iterative re-measure, no WPF-style
`c_layoutLoopMaxCount`. One-shot decision, accept the result.

## Future direction: native vs Taffy

The committed layout vocabulary is **`HStack`, `VStack`, `ZStack`,
`Canvas`, `Grid`**. Step C extends Stack Fill to be intrinsic-aware
(chat message); the Grid algorithm above extends Auto track sizing the
same way (property grid). Beyond this, the native panel set is "done"
for the foreseeable future.

If demand for richer flex/grid features (percentage flex-basis, wrap,
align-content, CSS Grid `minmax`/`repeat`/named areas) ever arrives,
the cheapest path is opt-in Taffy alongside the native panels —
`references/taffy.md` §7 has the integration sketch. We'll pick a
direction when the first user demand arrives.

## Height-given-width

There is no separate `intrinsic_main_given_cross` query, but
height-given-width is solved in-tree by a different mechanism:
**measure-at-INF on the queried axis with the committed cross**.

Concretely, in `stack::measure` pass-1 a non-Fill child is
measured with `axis.compose_size(INF, cross_avail)`. The child
runs its full layout under the finite cross — wrap text shapes
at the constrained width, nested grids resolve cols at the
constrained width — and reports the resulting main-axis size.
That answer is height-given-width by definition. ZStack and
Canvas use the same pattern on their Hug axes via
`child_avail_per_axis_hug`.

This is **not** equivalent to `intrinsic(child, main, MaxContent)`,
which would not see the cross. For wrap text the intrinsic
returns single-line height (unbounded shape); for a Grid with
wrapping cells it returns sum of single-line row heights.
Replacing INF-measure with intrinsic causes the parent to commit
a too-small main slot and inner contents collapse.

Pinned by:
- `vstack_section_with_hug_grid_and_fill_col_wrap_does_not_collapse`
- `hug_zstack_with_nested_grid_wrap_does_not_collapse`

A standalone `intrinsic_main_given_cross` would be a recursive
intrinsic that propagates width down — i.e., a measure pass with
a different name. The current "intrinsic for unbounded queries,
measure-at-INF for cross-dependent queries" split is the right
shape.

## Deferred

- **Baseline alignment.** Not part of intrinsics; would attach to
  `LayoutResult` if needed.
- **Aspect-ratio constraints.** Separate concern.
- **Cross-frame caching.** Cosmic's text-shape cache covers the
  expensive part. Re-add if a profile shows container intrinsic
  recomputation dominating.
