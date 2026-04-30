# Yoga — Reference for Palantir

Facebook's CSS Flexbox engine in C++ (now Meta), originally `css-layout` (2014, JS port of the spec), rewritten in C, then C++. Powers React Native, Litho, ComponentKit. Source in `tmp/yoga/yoga/`. The actual layout algorithm is one file: `algorithm/CalculateLayout.cpp` (2528 lines).

## 1. Spec subset

CSS Flexbox Level 1 minus a few corners, plus a couple of pragmatic extensions. What's in:

- `flex-direction` (row/col + reversed), `flex-wrap`, `flex-grow`/`flex-shrink`/`flex-basis`, `flex` shorthand.
- `justify-content`, `align-items`, `align-self`, `align-content`.
- `gap`/`row-gap`/`column-gap`.
- `position: absolute` with `inset` (`AbsoluteLayout.cpp`, 594 lines — its own sub-engine running after main flex layout).
- `aspect-ratio` (Yoga shipped this years before browsers).
- `display: none` and `display: contents`.
- `direction` (LTR/RTL).

What's missing or partial:

- No `min-content` keyword sizing — see `SizingMode.h:17` comment ("Yoga does not current support automatic minimum sizes"). `flex-basis: min-content` not supported.
- No baseline alignment across lines; intra-line baseline only (`Baseline.cpp`).
- No `position: sticky`, no floats, no inline flow.
- Box model is `box-sizing: border-box` always (padding/border included in the dimension, unlike CSS default `content-box`). Different from the web; matches React Native.
- Percentages resolve against parent's resolved size — fine when parent has a definite size, awkward in nested `Hug`/`MaxContent` contexts where the parent's size depends on the child (`CalculateLayout.cpp:1469-1502` has a "FixFlexBasisFitContent" experimental flag for exactly this case).

## 2. Node + measure callback

`yoga::Node` (`node/Node.h`) owns: `Style style_`, `LayoutResults layout_`, `vector<Node*> children_`, `Node* owner_`, `YGMeasureFunc measureFunc_`. Pointer-based tree, parent owns children, mutable.

The host integration point is the **measure function**: a C callback `YGSize(node, w, wMode, h, hMode)` set via `Node::setMeasureFunc` (`Node.cpp:100`). A node *with* a measure func is a leaf — Yoga asserts it has no children (`Node.cpp:108`). This is how text, images, and any host-measured content plug in. `Node::measure` (`Node.cpp:54`) just forwards to `measureFunc_(...)`. Yoga itself never inspects what's inside; the host returns a `(width, height)` and Yoga treats it as opaque.

`MeasureMode` (passed to the callback): `Exactly` (definite size given), `AtMost` (definite max), `Undefined` (size yourself). Internally Yoga renamed this to `SizingMode` with `StretchFit`/`FitContent`/`MaxContent` (`SizingMode.h:21`) — same three modes, CSS-spec-aligned names, with a translation back to the public `MeasureMode` for the callback.

This callback model is the part everyone copies (Taffy's `MeasureFunction` is the same shape; egui_taffy and Bevy UI use it for text). It's the single cleanest extension point in the API.

## 3. Layout cache

Two-level, per node, in `LayoutResults`:

- One "layout" slot: `cachedLayout` — the most recent `performLayout=true` call.
- A ring buffer of 8 "measurement" slots: `cachedMeasurements[MaxCachedMeasurements]` (`LayoutResults.h:25`) — measure-only calls (`performLayout=false`) populated round-robin.

Each entry is a `CachedMeasurement` (`node/CachedMeasurement.h`): `availableWidth`, `availableHeight`, two `SizingMode`s, `computedWidth`, `computedHeight`. Lookup is `canUseCachedMeasurement` (`algorithm/Cache.cpp:45`), which is more sophisticated than equality:

- Exact match on `(mode, available)` pair, OR
- `StretchFit` and previous output equals new input (we asked for exactly the size we got),
- Old result was `MaxContent` and new constraint is `FitContent` ≥ old result (max-content still fits),
- New `FitContent` is stricter than old `FitContent` and old result still fits the new constraint.

These rules let Yoga reuse a single intrinsic measurement across many parent contexts during one flex resolution pass, where a child gets re-measured under different `SizingMode`s as the parent figures out free-space distribution.

The cache is also keyed on a `generationCount` (`CalculateLayout.cpp:36`, atomic global) and a `configVersion`. `calculateLayoutInternal` (`CalculateLayout.cpp:2241`) bumps the generation each top-level `calculateLayout` call; cached entries from previous frames remain *valid* unless the node's `isDirty()` is set. So Yoga is *retained* across frames — re-running layout with no style changes is essentially free; only nodes with `isDirty_ = true` re-run.

## 4. Mutation tracking

`Node::setDirty` (`Node.cpp:174`), `Node::markDirtyAndPropagate` (`Node.cpp:421`):

```cpp
if (!isDirty_) {
  setDirty(true);
  setLayoutComputedFlexBasis(FloatOptional());
  if (owner_ != nullptr) owner_->markDirtyAndPropagate();
}
```

Walks up to the root. Every style setter funnels through this — `node/StyleProperties.cpp` setters all end with `markDirtyAndPropagate()` if the new value differs. `setMeasureFunc`, `setConfig`, `insertChild`, `removeChild` likewise. The early-out `if (!isDirty_)` makes propagation O(depth) the first time and O(1) thereafter until a layout pass clears the bits.

`calculateLayoutInternal` (`CalculateLayout.cpp:2259`) reads `isDirty()`, `lastOwnerDirection`, `configVersion` to decide whether to invalidate the cache. After `performLayout=true` it clears `setDirty(false)` (`CalculateLayout.cpp:2420`).

This is what React Native's reconciler relies on: diff your VDOM, mutate Yoga node styles, call `calculateLayout` once on the root — only changed subtrees recompute.

## 5. The "single-pass-ish" algorithm

`calculateLayoutImpl` (`CalculateLayout.cpp:1257`, ~1000 lines) runs in steps explicitly numbered in source comments:

1. **STEP 1**: Resolve flex/cross axis given `direction` (`:1413`).
2. **STEP 2**: Compute `availableInnerWidth/Height` by subtracting margin/padding/border (`:1441`).
3. **STEP 3**: `computeFlexBasisForChildren` — recursive call into each child to determine its flex basis (`:1508`). This is the **first sub-pass** and may itself recurse all the way down. It calls `calculateLayoutInternal` with `performLayout=false`.
4. **STEP 4**: `calculateFlexLine` partitions children into wrap-lines (`:1554`).
5. **STEP 5**: `resolveFlexibleLength` per line — distribute free space across `flex-grow`/`flex-shrink` (`:1648`). Two internal sub-passes (`distributeFreeSpaceFirstPass` then `distributeFreeSpaceSecondPass` at `:849, :650`) because frozen items violating min/max constraints feed back into the remaining free space.
6. **STEP 6**: `justifyMainAxis` (`:1679`) — final main-axis positions; computes cross-axis size for stretch.
7. **STEP 7**: Cross-axis stretch + alignment (re-measure children with `StretchFit` if needed).
8. **STEP 8**: Multi-line `align-content`.
9. **STEP 9** (after the big function): `layoutAbsoluteDescendants` runs `position: absolute` items against final container size.

The comment at `:1240` is honest: "multiple measurements may be required to resolve all of the flex dimensions". A child can be visited 2-4× in one pass (basis measurement, flex resolution remeasure, stretch remeasure, final layout). The 8-slot per-node cache is sized for this.

So Yoga is *not* a clean two-pass like WPF. It's more like "one logical pass that recursively re-enters itself with different `SizingMode`s, with a cache pinning down convergence." The cache isn't an optimization, it's load-bearing — without it, re-measure would be exponential in tree depth.

## 6. Known issues

- **`aspect-ratio`** (`CalculateLayout.cpp:202-246, 744-759, 1754-1759`): Yoga's implementation predates the CSS spec and disagrees with browsers in subtle ways around when aspect-ratio applies vs. when explicit dimensions win. Sprinkled throughout the algorithm rather than centralized.
- **Baselines**: `Baseline.cpp` only handles single-line `align-items: baseline`; cross-line baselines are wrong. Yoga ships with a baseline callback (`baselineFunc_`) but most hosts don't set it.
- **Percentages in indefinite contexts**: a `width: 50%` child of a `Hug` parent has no resolution — Yoga returns 0 or undefined, browsers do something more nuanced. The `FixFlexBasisFitContent` experimental flag (`:1474`) tries to compute a definite owner-size from a definite parent-of-parent, but it's opt-in.
- **Errata flags** (`enums/Errata.h`): Yoga maintains `StretchFlexBasis`, `AbsolutePositionWithoutInsetsExcludesPadding`, `AbsolutePercentAgainstInnerSize` as feature-flagged bug compatibility for downstream consumers (React Native classic). Says everything about how hard breaking changes are.
- **Pixel grid rounding** (`PixelGrid.cpp`): rounding done per-node accumulated 1px gaps in nested layouts at fractional scales; the well-known `aa5b296` fix in Taffy switched to rounding cumulative coordinates instead. Yoga still does per-node.
- **Performance flat**: extremely wide flat trees (1 parent, 100k children) iterate `flexLine` and `resolveFlexibleLength` linearly per child — Yoga still beats Taffy here per Taffy's own benchmarks, but it's the worst case.

## 7. Lessons for Palantir

**What flexbox gives you that WPF Hug/Fill/Fixed doesn't.**

- `flex-grow` with **non-equal weights**: WPF's `*` (Grid) supports `2*` vs `1*` weighting for proportional space; Palantir's `Sizing::Fill` distributes leftover *equally* across siblings. Real apps want "this column gets 2× the leftover, this one 1×" — a single `f32` weight per `Fill` child closes the gap (`Sizing::Fill { weight: f32 }`).
- `flex-shrink`: when content overflows, who shrinks first? WPF's answer is "everyone clips at slot boundary". Flexbox's answer is a per-item shrink factor. Cleaner for responsive UI; load-bearing for `react-native` style content.
- `flex-basis`: "compute me at this size first, *then* distribute leftover". Decouples intrinsic size from final size. WPF's `Hug` measures intrinsic then `Fill` distributes — same idea, less general (you can't say `flex-basis: 100px; flex-grow: 1`).
- `gap`: built-in inter-child spacing without margins-on-every-child gymnastics. Cheap to add to `HStack`/`VStack`.
- `align-self`: per-child cross-axis override of parent's `align-items`. WPF has `HorizontalAlignment`/`VerticalAlignment` already — same thing.

**What it costs.**

- The 8-slot per-node cache. A naive flexbox without caching is O(2^depth) on pathological trees. Either we accept retained `Cache`s in the state map (we already have `WidgetId → Any`) or we accept worse asymptotics on deep trees.
- Multi-pass children. `Button` containing a flex container would be measured 2-4× per frame, not once. Our `Shape`/`Node` split assumes paint reads each node's `Rect` once after measure+arrange — flex doesn't break that, but the measure recursion is more expensive.
- Algorithmic complexity. `CalculateLayout.cpp` is ~2500 lines for a reason — the corner cases (wrap + min/max + aspect-ratio + absolute children + RTL) genuinely interact.
- Style surface area. Yoga's style is ~30 fields; `taffy::Style` is ~50. `Sizing` + `Spacing` + `Style` is currently ~10 fields total. Going full flexbox 3-5× the style cost per node.

**Recommendation.**

*Do* extend `HStack`/`VStack` with:

1. **`Sizing::Fill { weight: f32 }`** (default `1.0`). Trivial change in `resolve_axis` — divide leftover proportionally instead of equally. Keeps the WPF mental model, gets us 80% of `flex-grow`. Add a lib test pinning weighted distribution.
2. **`gap: f32`** on `HStack`/`VStack`. ~5 lines in the arrange driver. No measure-pass implications because gap is part of the panel's own size policy.
3. **`align: Start | Center | End | Stretch`** cross-axis (per child override). Per-child translation in arrange, mirrors WPF `HorizontalAlignment`/`VerticalAlignment` and is cheap.

*Do not* port full flexbox into core. Reasons:

- `flex-shrink` requires the multi-pass measure cache. We rebuild the tree every frame with no cross-frame `Cache` — adopting Yoga semantics drags retained cache and dirty propagation along with it.
- `flex-basis` distinct from content size only matters once `flex-grow` ≠ 0 and `flex-shrink` ≠ 0 simultaneously. The 80% case is "stretch to fill" which we already have.
- `wrap` opens cross-axis line-collection complexity; not needed for desktop tooling UIs.
- The 2500-line algorithm is famously hard to debug. WPF's `Hug`/`Fill`/`Fixed` is ~120 lines in our `layout.rs` and pinned by tests.

If a user genuinely needs CSS-flex semantics (porting a web layout, complex wrapping rules), expose it via the Taffy feature gate — that's exactly what `references/taffy.md` §9 already argues. Yoga itself is C++ and Meta-internal; there's no Rust port worth depending on.

**Single biggest takeaway:** Yoga's caching is *the* algorithm, not an optimization. A Rust flexbox without retained per-node measurement caches is either incorrect or exponentially slow. If we're not willing to give a node a persistent `Cache` (we're not, that's the immediate-mode contract), we shouldn't ship full flexbox. Weighted `Fill` + `gap` + `align-self` gets us the user-visible wins without that bargain.
