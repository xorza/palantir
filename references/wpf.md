# WPF Layout — Reference for Palantir

Grounded in `tmp/wpf/src/Microsoft.DotNet.Wpf/src/{PresentationCore,PresentationFramework}`. Two classes carry it: `UIElement` (defines `Measure`/`Arrange` and dirty-flag machinery) and `FrameworkElement` (which `sealed`-overrides `MeasureCore`/`ArrangeCore` to apply `Margin`/`Width`/`MinMax`/`Alignment` policy, then forwards to user-overridable `MeasureOverride`/`ArrangeOverride`).

## 1. Measure → Arrange contract

`UIElement.Measure(Size availableSize)` (UIElement.cs:552):

- `availableSize` is a *soft* constraint passed top-down. Either axis may be `double.PositiveInfinity` ("measure to content"). `NaN` throws.
- Short-circuits if `IsMeasureValid && !neverMeasured && DoubleUtil.AreClose(availableSize, _previousAvailableSize)` (line 619). This cache is what makes idle frames cheap.
- Otherwise calls `MeasureCore(availableSize)` inside try/finally guarded by `MeasureInProgress`, validates non-`Infinity`/non-`NaN`, stores `_desiredSize`, clears `MeasureDirty`, and — crucially — calls `InvalidateArrange()` *before* `MeasureCore` so any newly arranged subtree is queued.
- If `_desiredSize` changed and parent isn't measuring, calls `parent.OnChildDesiredSizeChanged(this)` (line 323), which by default re-invalidates the parent. This is how desired-size deltas bubble.

`Arrange(Rect finalRect)` is symmetric: parent picks a final rect (typically ≥ child's `DesiredSize`), child distributes via `ArrangeOverride`, stores `RenderSize`. If `finalRect.Size < unclippedDesiredSize`, `FrameworkElement.ArrangeCore` (line 4597) bumps `arrangeSize` back up to `unclippedDesiredSize` and sets `NeedsClipBounds = true` — child still lays out at desired, but is clipped at the slot.

Custom panel contract reduces to:
```text
MeasureOverride(available):  for each child: child.Measure(slot); return desired
ArrangeOverride(finalSize):  for each child: child.Arrange(rect);  return used
```

## 2. Width/Height/Min/Max, Margin — outer vs inner

`FrameworkElement.MeasureCore` (FrameworkElement.cs:4284) applies these; `MeasureOverride` never sees them directly. Sequence:

1. Subtract `Margin` from `availableSize` → `frameworkAvailableSize`. Margin is *outer*, outside `Width`/`Height`.
2. Build `MinMax` from `Width/Height/MinWidth/MinHeight/MaxWidth/MaxHeight`. If `Width` is set, `MinMax` collapses min and max to that value — `Width` is just a coercion of the range.
3. Clamp `frameworkAvailableSize` into `[min, max]` (line 4375).
4. Call `MeasureOverride(frameworkAvailableSize)` — panel sees the *inner* box.
5. Maximize result against `mm.min`, clip against `mm.max` (sets `clipped`), add margins back. Returned desired size is the *outer* box.

`Auto` ⇔ no `Width` set (`MinMax` leaves `[0, +∞]`, child returns content size). `*` is a `GridLength` only meaningful inside `Grid`. Padding is per-control (`Border.Padding`, `Control.Padding`), not framework-level. `MinWidth` always wins over `MaxWidth` if they conflict (line 4375 clamps min after max).

## 3. Invalidation and dirty flags

WPF does **not** re-layout the tree every frame. Layout runs only on dirty marks:

- `InvalidateMeasure()` (UIElement.cs:249): if `!MeasureDirty && !MeasureInProgress && !NeverMeasured`, push self onto `ContextLayoutManager.MeasureQueue`, set `MeasureDirty = true`. Else no-op.
- `InvalidateArrange()` (line 282): same shape, separate queue.
- `DependencyProperty` metadata flagged `AffectsMeasure`/`AffectsArrange` calls these on change.
- `OnChildDesiredSizeChanged` propagates desired-size deltas upward.

`ContextLayoutManager.UpdateLayout` drains Measure queue (deepest-first), then Arrange, then fires `LayoutUpdated`. Because `Measure` short-circuits on unchanged `availableSize`, only the dirty subtree actually re-runs.

## 4. Visual vs logical tree

**Logical tree** (`LogicalTreeHelper`) = author's content/items model, used for resource and inheritance lookup. **Visual tree** (`VisualTreeHelper`, rooted at `Visual`) = post-template, what layout, hit-test, and render walk. `UIElement : Visual` adds layout; `FrameworkElement : UIElement` adds resources/styles/logical-tree linkage. Layout uses *only* the visual tree. Palantir collapses both into a single arena since there's no template expansion.

## 5. Specific panels

**StackPanel** (`Stack.cs:543`, `StackMeasureHelper`). On the stacking axis, sets `layoutSlotSize.{Width|Height} = double.PositiveInfinity` before `child.Measure`. Each child returns intrinsic size on that axis; panel sums them, takes max on cross axis. Arrange walks once, assigning `childDesiredSize` on main axis, `arrangeSize` on cross. Non-`Stretch` cross-alignment causes `FrameworkElement.ArrangeCore` (line 4611) to shrink slot to desired. **The infinity trick is the single most important pattern: infinite available on main axis forces children to return intrinsic — exactly what `Hug` semantics need.**

**Grid** (`Grid.cs:399`). Multi-pass; see ASCII diagram around line 580 and dispatch at 618–680. Cells partition into four groups by row/column kind (`Pixel`/`Auto`/`*`):
- Group1: Auto/Auto and Pixel — measure first, fixes Auto sizes.
- Group2: `*` columns ∧ Auto rows.
- Group3: Auto columns ∧ `*` rows.
- Group4: `*`/`*` — measured last with resolved star sizes.

`ResolveStar` distributes remaining space proportionally across `*` definitions, respecting min/max. The cyclic case (Group2 ∧ Group3, no Group1 to break the tie) loops Group2/Group3 measure up to `c_layoutLoopMaxCount` until `hasDesiredSizeUChanged` settles (line 658). **This is WPF's most expensive corner** and where naive use blows up to many measure passes per child.

**DockPanel** (`DockPanel.cs:198`). Single-pass measure. Walks children in order; each gets `childConstraint = remaining_size_on_each_axis`, `child.Measure(childConstraint)`. `Dock.Left/Right` consumes width; `Dock.Top/Bottom` consumes height. Arrange (line 261) peels rectangles off the slot edges. `LastChildFill` gives the final child whatever remains. No iteration — DockPanel is the cheapest interesting panel.

## 6. Alignment in Arrange

When `ArrangeCore` gets a slot larger than `unclippedDesiredSize` and `HorizontalAlignment != Stretch`, line 4611 shrinks `arrangeSize` back to `unclippedDesiredSize`. The leftover slack feeds `ComputeAlignmentOffset` (line 4805): `Left → 0`, `Right → slack`, `Center/Stretch → slack/2`, with `Stretch → Left` fallback when not actually stretched. The element's `LayoutOffset` becomes `finalRect.TopLeft + alignmentOffset`. So **`Stretch` is the only mode that consumes the full slot**; every other mode renders at `DesiredSize` and pushes leftover into a translation. Panels' `ArrangeOverride` see the post-shrink rect — they never deal with extra slot space themselves.

## 7. Lessons for Palantir

**Copy verbatim.**
- Two-pass split: post-order Measure (`available` in, `desired` out), pre-order Arrange (`finalRect` in). Load-bearing.
- Infinity-on-main-axis for HStack/VStack — already in our `Hug` semantics; preserve it.
- Outer/inner box: subtract margin, clamp by min/max, run override, clamp again, add margin back. Treat `Width = N` as `min = max = N` to collapse cases.
- Alignment-in-Arrange: shrink slot to desired then translate. Don't make panels handle alignment.
- Port DockPanel almost as-is — algorithm is ~30 lines and correct.

**Simplify.**
- Drop `LayoutTransform` (FrameworkElement.cs:4368, 4623). `FindMaximalAreaLocalSpaceRect` is ~⅓ of `MeasureCore` and useless without arbitrary 2D transforms.
- Drop `UseLayoutRounding` at the layout level. Round once in the paint pass against actual surface scale.
- Drop `UnclippedDesiredSizeField` caching (line 4478). WPF caches it because re-measure is incremental; we re-measure every frame, so a local suffices.
- Drop `BypassLayoutPolicies`, `LayoutSuspended`, `Visibility.Collapsed`. Elide collapsed nodes at recording time.
- Drop the entire dispatcher (`ContextLayoutManager`, queues, `Dispatcher.DisableProcessing`). No retained tree → no queue → the rebuild *is* the invalidation. (Per `DESIGN.md` §7: defer dirty tracking until profiling demands it.)
- Drop `OnChildDesiredSizeChanged` cascading — only matters when partial re-measure is allowed.
- Fold `MeasureCore` (sealed framework policy) and `MeasureOverride` (panel logic) into a single `measure(node, available)` that runs margin/min-max wrapping inline. Saves a v-table dispatch and the two-method mental model. The non-transform, non-rounding path of `MeasureCore` is ~30 of its 220 lines — keep those.

**What WPF gets wrong / overcomplicates.**
- `Grid`'s cyclic-group iteration is bounded by `c_layoutLoopMaxCount` *because the algorithm doesn't always converge*. When/if you add Grid: restrict to fixed + Auto + Star without `*` ↔ `Auto` cross-axis cycles. The 95% case is one measure pass with two `ResolveStar` calls.
- `MinMax` is rebuilt for both Measure and Arrange; cache once per node.
- `FrameworkElement` splits policy across `MeasureCore` (sealed) and `MeasureOverride` (virtual); the boundary is leaky (panels still see `availableSize` post-margin/min-max but must respect their own `Stretch` semantics in Arrange via the slot-shrink). Folding both removes the surprise.

**Single biggest takeaway:** layout policy belongs to the framework wrapper, not the panel. Panels know only about children's `desired`/`final` rects. Margin, min/max, width/height, alignment — all happen in the wrapper around `MeasureOverride`/`ArrangeOverride`. Mirror this split in Rust (`fn measure(node)` wraps `fn measure_panel(node)`) and panels stay small.
