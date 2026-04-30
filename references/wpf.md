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

## 8. Online retrospectives & lessons from the field

The source tree shows *what* WPF does; this section catalogues what 15 years of shipping it taught the people who wrote it and used it. Useful when deciding what *not* to copy.

**Why momentum stalled.** WPF was retained-mode-on-DX9 with a single STA dispatcher; by the time Win10 shipped, the platform had moved to Composition (DComp / Visual Layer / DX12). WinUI 3 explicitly replaces the rendering pipeline: "WPF relies on DirectX 9, while WinUI 3 leverages the Visual Composition Layer powered by DirectX 12 … compositor-backed rendering keeps it pegged near 60fps in scenarios where WPF drops frames" ([CTCO comparison](https://www.ctco.blog/posts/winui-vs-wpf-2026-practical-comparison/), [Heise GUI frameworks](https://www.heise.de/en/background/GUI-frameworks-for-NET-Part-2-WPF-and-WinUI-3-10372839.html)). The dispatcher itself is a known liability: every `Dispatcher.CurrentDispatcher` access on a thread "will unconditionally create a Dispatcher … [and] creates an HWND and other resources that will never get freed" ([dotnet/wpf#3384](https://github.com/dotnet/wpf/issues/3384), [dotnet/wpf#3412](https://github.com/dotnet/wpf/issues/3412)). Lesson for Palantir: not having a dispatcher is a feature; don't reintroduce one for "thread safety" without a concrete need.

**The DependencyProperty leak family.** The dominant WPF leak shape is: long-lived source raises `PropertyChanged` → short-lived listener subscribed via strong delegate → listener pinned forever. JetBrains' dotMemory writeup names this the #1 WPF leak pattern ([Fighting Common WPF Memory Leaks](https://blog.jetbrains.com/dotnet/2014/09/04/fighting-common-wpf-memory-leaks-with-dotmemory/), [Memory Leaks and Dependency Properties](http://www.sharpfellows.com/post/Memory-Leaks-and-Dependency-Properties)). The worst variant is `DependencyPropertyDescriptor.AddValueChanged`: "GC will not collect objects subscribed … until they're unsubscribed using `RemoveValueChanged`" ([eidias blog](https://www.eidias.com/blog/2014/2/13/memory-leak-using-propertydescriptor-addvaluechanged)). MS's own answer was the `WeakEventManager`/`IWeakEventListener` pattern ([Weak event patterns](https://learn.microsoft.com/en-us/dotnet/desktop/wpf/events/weak-event-patterns), [jgoldb on memory leaks](https://learn.microsoft.com/en-us/archive/blogs/jgoldb/finding-memory-leaks-in-wpf-based-applications)) — which says it all: the framework needed an entire parallel event system to undo its default. Palantir sidesteps this entirely by not having a property system; widget state lives in an `Id → Any` map keyed by `WidgetId`, and the tree is rebuilt each frame.

**MeasureOverride footguns.** Three recurring patterns from the field:
- Calling `InvalidateMeasure()`/`InvalidateArrange()` from inside `MeasureOverride`/`ArrangeOverride` produces an unbounded Render→Layout→Render loop ([codestudy.net infinite loop](https://www.codestudy.net/blog/infinite-loop-invalidating-the-timemanager/), [SLaks: don't modify other controls during layout](https://blog.slaks.net/2011/07/dont-modify-other-controls-during-wpf.html)). MS docs explicitly call this out on `UIElement.InvalidateMeasure`.
- Not honoring `+∞` available size: parents pass `PositiveInfinity` to ask "what's your intrinsic?"; panels that clamp it to `0` or to their own `RenderSize` produce wrong sizes and trigger re-measure feedback ([Actipro: ZoomContentControl infinite measure](https://www.actiprosoftware.com/community/thread/23283/zoomcontentcontrol-infinitely-calling-measure), [Microsoft Q&A on UIElement.Measure perf](https://learn.microsoft.com/en-us/answers/questions/903575/performance-on-wpf-uielement-measure)).
- Returning a `desired` size that depends on the *available* size in a way that's non-monotonic; this is the cyclic-measure bait — Telerik and DevExpress both ship workarounds for it in their grid controls ([Telerik MeasureOverride performance](https://www.telerik.com/forums/measureoverride-performance), [DevExpress Q480745](https://supportcenter.devexpress.com/ticket/details/q480745/performance-issue-with-onlayoutupdated-measureoverride-and-arrangeoverride-events)).

**The Grid cyclic pathology in the wild.** The `c_layoutLoopMaxCount` loop in `Grid.MeasureOverride` is hit any time a `*` column contains an `Auto` row whose content depends on the column's resolved width (or vice versa). MS's own VS2010 perf retro highlights Grid as a top offender ([VS2010 WPF perf tuning](https://devblogs.microsoft.com/visualstudio/wpf-in-visual-studio-2010-part-2-performance-tuning/)); Dr. WPF's Layout series walks through reproductions ([Dr. WPF: Layout](http://drwpf.com/blog/category/layout/), [Dr. WPF: Panels](http://drwpf.com/blog/category/panels/)). Avalonia's recommendation in their migration docs is explicit: "prefer `Panel` for overlapping content since it avoids the overhead of the Grid layout engine" ([Avalonia layout migration](https://docs.avaloniaui.net/docs/migration/wpf/layout)). Lesson: keep Grid out of the prototype; if added later, forbid `Auto`↔`*` cross-axis dependencies.

**Virtualization is fragile by design.** The CodeMag "XAML Anti-Patterns: Virtualization" piece is the canonical catalogue ([CodeMag](https://codemag.com/Article/1407081/XAML-Anti-Patterns-Virtualization)): wrapping an `ItemsControl` in a `ScrollViewer` silently disables virtualization; restyling the template without a `ScrollViewer` does too; nested `ItemsControl` blows past 1.3GB and OOMs without explicit `VirtualizingStackPanel.IsVirtualizing="True"` on every level. `Auto` heights kill it. Container recycling is opt-in. MS's own perf doc admits most of this ([Optimize control performance](https://learn.microsoft.com/en-us/dotnet/desktop/wpf/advanced/optimizing-performance-controls)). Vincent Sibal's archived blog has the DataGrid-flavoured deep dives ([Vincent Sibal's archive](https://learn.microsoft.com/en-us/archive/blogs/vinsibal/), [DataGrid visual layout](https://learn.microsoft.com/en-us/archive/blogs/vinsibal/wpf-datagrid-dissecting-the-visual-layout)). Palantir punt: don't ship a virtualizer until there's a concrete list-of-10k case; when you do, design it as a different *recorder* (skip `show()` for offscreen items) rather than a layout-time concept.

**Airspace and the visual-tree dead end.** Dwayne Need's `AirspaceDecorator` work ([dwayneneed.github.io: WPF](https://dwayneneed.github.io/category/WPF), [MahApps mirror](https://github.com/MahApps/Microsoft.DwayneNeed)) documents the unfixable seam between WPF's software-composited tree and any HWND/D3D content (browser, video, native control). The root cause is that WPF composites a *retained* visual tree internally and hands a single bitmap to DWM — anything not inside that tree lives in a different airspace. WinUI 3's switch to the Visual Layer (DComp) is precisely the fix: every `Visual` becomes a system composition node, so HWND interop and acrylic/mica "just work" ([WPF→WinUI3 migration patterns](https://learn.microsoft.com/en-us/windows/apps/windows-app-sdk/migrate-to-windows-app-sdk/wpf-patterns-winui3)). For Palantir on wgpu: there's no airspace problem because there's no internal compositor — we paint to a surface the host owns.

**What Avalonia and Uno changed.** Both keep the two-pass measure/arrange contract verbatim — the algorithm is sound. What they dropped or altered:
- Avalonia: render-transform pivot defaults to centre (50%,50%) not top-left; `StackPanel.Spacing` is first-class; `Panel` (z-stack) is preferred over `Grid` for overlay; `LayoutTransform` is gone in favour of just `RenderTransform` plus opt-in layout-aware variants ([Avalonia: Render vs layout transforms](https://docs.avaloniaui.net/docs/graphics-animation/render-vs-layout-transforms), [DMC: noteworthy differences](https://www.dmcinfo.com/blog/15571/avalonia-ui-noteworthy-differences-from-wpf/), [5 Avalonia features WPF devs envy](https://avaloniaui.net/blog/5-avalonia-features-that-make-wpf-devs-jealous)).
- Uno: alignment defaults to top-left rather than `Stretch`, matching WinUI not WPF; the rest of the layout pipeline is a faithful WinUI port retargeted to native renderers per platform ([Uno: WPF migration](https://github.com/unoplatform/uno/blob/master/doc/articles/wpf-migration.md/), [Uno UI layer: WinUI to native](https://platform.uno/articles/how-uno-platforms-ui-layer-works-winui-to-native/)).

Cross-cutting signal: every reimplementation kept measure/arrange and threw away `LayoutTransform`, the dispatcher coupling, and (implicitly) the DependencyProperty leak surface. That's the same shortlist as §7 — independent confirmation we're cutting in the right places.
