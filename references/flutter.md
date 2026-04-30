# Flutter — reference notes for Palantir

Flutter is the canonical "constraints go down, sizes go up" layout engine. Authoring is reactive (rebuild a widget tree on state change) but the *layout protocol* underneath is the most-cited contemporary alternative to WPF-style two-pass measure/arrange. Worth understanding even though our authoring model is closer to egui's than Flutter's. No source in `tmp/` — citations are to the public docs and `inside-flutter`.

## 1. The three trees

Flutter splits what WPF fuses into one `UIElement` and what egui erases entirely:

- **Widget**: immutable, cheap, disposable configuration. `Container(color: blue, child: ...)` allocates a fresh widget every build. Has no identity beyond `key` + `runtimeType`.
- **Element**: persistent instance. One per widget *position* in the tree. Holds the link to the live `State` (for `StatefulWidget`), the `BuildContext`, and the bridge to the render tree. Survives across builds when widget type matches at the same slot.
- **RenderObject**: the actual layout/paint participant. Owns `constraints`, `size`, `parentData`, `markNeedsLayout`, `markNeedsPaint`. Subclasses: `RenderBox` (Cartesian), `RenderSliver` (scroll), `RenderView` (root).

`RenderObjectElement.mount` creates the render object; `update` mutates it in place when the new widget's config differs. Hot reload survives because the *element* tree (and its `State` objects) outlives any individual widget instance — code reload swaps widget classes, the element tree re-runs `build`, reconciliation patches render objects.

For Palantir this maps to: "`Widget` ≈ recording-time `Node`/`Shape` calls, `Element` ≈ persistent state map (`Id → Any`), `RenderObject` ≈ the arena `Tree`." The split is load-bearing for hot-reload-style retained UIs; it is *unnecessary* for an immediate-mode arena that rebuilds every frame.

## 2. Layout protocol — `performLayout` + `BoxConstraints`

Source: `api.flutter.dev/flutter/rendering/RenderBox-class.html`, `RenderObject/performLayout.html`, `docs.flutter.dev/ui/layout/constraints`.

The contract, verbatim from the docs: **"Constraints go down. Sizes go up. Parent sets position."**

`BoxConstraints { minWidth, maxWidth, minHeight, maxHeight }` flows top-down. Each `RenderBox.performLayout()`:

1. Reads `this.constraints` (set by parent's `child.layout(constraints, parentUsesSize: ...)`).
2. For each child: `child.layout(childConstraints, parentUsesSize: true)` — a single call that does *both* what WPF splits into `Measure` and the size-determining part of `Arrange`.
3. Sets `this.size` (or, if `sizedByParent == true`, sets it in `performResize` from constraints alone).
4. Walks children again to assign `child.parentData.offset` (positions). This is the "parent sets position" half.

Critical invariant: a child cannot read its own position, and a parent cannot read `child.size` unless it passed `parentUsesSize: true`. The flag exists purely for the relayout-boundary optimization (§4) — if the parent didn't use the size, dirtying the child can't dirty the parent.

So Flutter is "one pass" only in the sense that constraints + size flow happen together per node. Position assignment is still a second walk over children — it just happens inside the same `performLayout`. The model is a single recursion with a constraint argument, not WPF's two top-level passes.

## 3. Why one-pass-with-constraints, not two-pass measure/arrange

Stated tradeoff (`docs.flutter.dev/ui/layout/constraints`, "Limitations"):

- "A widget can decide its own size only within the constraints given to it by its parent."
- "A widget can't know and doesn't decide its own position."
- "It's impossible to precisely define the size and position of any widget without taking into consideration the tree as a whole."

The win: each node visited at most twice (down with constraints, up with size). `inside-flutter` calls this **sublinear layout** because skip-clean-subtree caching turns the steady state into O(dirty-set), not O(tree). The loss: layouts that genuinely need a child's intrinsic size *before* deciding the child's constraints (e.g. "make all columns as wide as the widest cell") cannot fit the protocol — they fall back to `IntrinsicWidth`/`IntrinsicHeight`, which is the slow path (§5).

WPF's two-pass split makes the same problem cheap: `MeasureOverride` returns desired with no committed slot; `ArrangeOverride` then partitions space using those desireds. Flutter chose to forbid the dependency outright rather than pay for a generalized Measure pass. Palantir already pays for two passes, so we get table-style "max width across rows" for free where Flutter does not.

## 4. Relayout boundaries and dirty propagation

`markNeedsLayout()` walks up until it hits a render object that is a *relayout boundary*, then schedules only that subtree. A node is a relayout boundary iff one of:

- `parentUsesSize == false` (parent doesn't read our size, so resizing us can't resize them).
- `constraints.isTight` (min == max on both axes, so our constraints can't change without our parent re-laying out anyway).
- `sizedByParent == true` (size is a pure function of constraints).
- Explicit: the node is a `RenderConstrainedBox` with tight constraints from above.

This is why `SizedBox(width: 100, height: 100, child: ...)` is a perf idiom — it injects a tight-constraint relayout boundary. It's the same insight as WPF's `IsMeasureValid` cache (`UIElement.cs:619`), reframed: instead of "skip if availableSize unchanged," Flutter says "skip the parent walk entirely if we can prove changes can't escape this subtree."

Repaint boundaries (`RepaintBoundary` widget) are the analogous concept for the paint phase — they get their own layer in the compositor, so `markNeedsPaint` stops there.

## 5. Intrinsic dimensions — the O(n²) folklore

`computeMinIntrinsicWidth(double height) → double` and three siblings let a parent ask "how wide do you want to be at this height?" *without* committing to a layout. They're used by `IntrinsicWidth`, `IntrinsicHeight`, `Table`, `IntrinsicColumnWidth`, `Wrap`-with-baseline, and a handful of custom layouts.

From `inside-flutter`: "Some layouts involve intrinsic dimensions or baseline measurements, which do involve an additional walk of the relevant subtree (aggressive caching is used to mitigate the potential for quadratic performance in the worst case). These cases, however, are surprisingly rare."

The quadratic case: a node with N children calls each child's intrinsic — each child recursively asks *its* children — and if intrinsic results are not cached at every level, the depth-d tree does O(d × n) work per query, and a parent that queries multiple intrinsics on each child (e.g. "min and max width at this height") multiplies further. Flutter's docs and `RenderIntrinsicHeight` both warn explicitly: prefer not to use intrinsics; they "add a speculative layout pass before the final layout phase." The `IntrinsicHeight` widget docs literally tell you "this class is relatively expensive. Avoid using it where possible."

Compare WPF: `Measure` *is* the intrinsic-width query — it's just always run, and its result is cached on the node. There's no separate "intrinsic" path. The cost Flutter calls quadratic is the cost WPF amortizes across every layout. Palantir, like WPF, gets intrinsic for free by virtue of running a real Measure pass.

## 6. Sliver protocol

`RenderSliver` replaces `RenderBox` for scrollable contents. Constraints become `SliverConstraints { axisDirection, scrollOffset, remainingPaintExtent, crossAxisExtent, viewportMainAxisExtent, ... }` and the output is `SliverGeometry { scrollExtent, paintExtent, maxPaintExtent, layoutExtent, ... }`.

The point: a sliver knows its own scroll position. A `SliverList` of 10 000 items asks each child to lay out *only when* its `scrollOffset` enters the visible window (`remainingPaintExtent > 0` after the prior siblings have consumed their share). Off-screen children are never laid out.

Box layout cannot do this — `BoxConstraints` has no scroll concept, so a `Column` of 10 000 children forces every one through layout. Slivers are a parallel protocol bolted on for the lazy-list case. Inside a viewport, `RenderViewport` translates between the two: it lays out box-children with viewport-derived constraints, and slivers via the sliver protocol.

For Palantir: when we hit virtualized lists, we'll need *something* — either Flutter-style protocol bifurcation, or a "virtual node" that emits children lazily in measure. The latter fits two-pass layout better; the sliver protocol is essentially a second layout protocol for one specific optimization, which feels like a ratchet we should avoid until we have to.

## 7. Build / layout / paint pipeline + Impeller

`BuildOwner` drives widget→element rebuild (dirty elements list, `buildScope`). `PipelineOwner` drives layout, compositing-bits-update, paint, semantics. Each phase has its own dirty set; phases run in order, each draining its set. This is structurally identical to WPF's `ContextLayoutManager` pumping Measure then Arrange queues — Flutter just adds a build phase upstream and a paint-layer phase downstream.

Rendering: was Skia-on-everything; Flutter 3.24 (Feb 2024) made **Impeller** default on iOS, then Android API 29+. Impeller AOT-compiles shaders (Skia compiled JIT, causing "shader jank" — the visible stutter the first time a new effect rendered). Targets Metal/Vulkan directly. Architecturally it's a tessellator + a small set of pre-built pipelines, very similar in spirit to what Palantir's renderer plan calls for — typed batches over a fixed pipeline set, no general 2D canvas API exposed to user code.

## 8. Lessons for Palantir

**Copy.**
- `BoxConstraints { min, max }` per axis as the canonical layout input. Cleaner than WPF's "available + min/max as DPs" because min/max travel inseparably from the constraint. We already do something close in `geom::Sizing`; consider making the *constraint* an explicit `(Range<f32>, Range<f32>)` passed into `measure` rather than a single `Size available` plus implicit clamps.
- Tight-constraint = relayout boundary. If we ever add incremental relayout, `Sizing::Fixed(n)` is the natural boundary marker — same logic as Flutter's `isTight`.
- Repaint boundaries as an explicit user-facing widget when we have layered compositing. The opt-in model beats heuristics.
- Impeller's "AOT-compile a small fixed pipeline set, no general canvas API" — this is exactly the renderer shape `DESIGN.md` already commits to. Concrete validation that the trade is worth it.

**Avoid.**
- Three-tree split. We have no hot-reload requirement, no retained widget instances, no `setState`-driven rebuild. Widget/Element/RenderObject collapses to `node arena + state map` for us. Don't reinvent it.
- The intrinsic-dimensions API as a separate code path. Flutter has it because their main protocol forbids reading child size before constraining it; we don't have that constraint, so "intrinsic" *is* `Measure(Constraint::unbounded)`. One path, no caching layer needed.
- Sliver protocol bifurcation for lazy lists. When virtualization comes, prefer a "virtual children" hook on a single node that yields measured children on demand within the visible window — handled by the existing measure pass, not a parallel protocol.
- "One-pass with constraints" as the layout philosophy. The Flutter docs admit the limitation upfront ("can't precisely define size without considering the tree as a whole", `IntrinsicHeight` "relatively expensive"). WPF-style two-pass costs us one extra tree walk and buys back the entire class of layouts Flutter pushes onto the slow intrinsic path. We've already paid; collect the dividend.

**Simplify.**
- Flutter passes constraints *and* a `parentUsesSize` flag to enable the relayout-boundary check. We don't have incremental relayout, so drop the flag. If/when we add it, prefer "is the node a `Sizing::Fixed` or root" over per-call flags — same information, less plumbing.
- `sizedByParent` is a fast-path optimization for nodes whose size depends only on constraints (not children). Equivalent in Palantir is a node with `Sizing::{Fixed, Fill}` on both axes — no measure of children needed for own size. The optimization is real (skips a child measure traversal during `markNeedsLayout` upward walk); not worth implementing until profiles show it.

**Single biggest takeaway.** Flutter's layout shape is a direct consequence of choosing reactive-rebuild + retained render tree + incremental relayout. The "one-pass" rhetoric is really "one *recursion* per relayout subtree, with constraints in and size out." Once you have that, intrinsic queries become a separate, expensive, opt-in API because the main protocol can't express them. Palantir's rebuild-every-frame + two-pass model lets us put intrinsic queries on the main path instead — and that, not the immediate-mode authoring API, is where we actually diverge from Flutter.
