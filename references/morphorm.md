# Morphorm — Reference Notes for Palantir

Morphorm (geom3trik, used by Vizia) is a single-pass, parent-recursive layout engine in Rust. ~2.4k LOC across `tmp/morphorm/src/{types,node,cache,layout}.rs`. Marketed as "flexbox-like with fewer concepts you have to learn"; the README pitch is `Pixels | Percentage | Stretch | Auto` — four units, applied uniformly to width/height *and* to the four edge spacings (`left/right/top/bottom`) and to gaps. That symmetry is the whole design idea.

## 1. The model: parent picks axis, children pick units

`LayoutType` (`types.rs:3`) on the parent: `Row | Column | Overlay | Grid`. The parent owns one bit of policy — main-axis direction. Defaults to `Column`. There is no `flex-direction`, no `justify-content`, no `align-items` per-axis matrix; instead `Alignment` (`types.rs:202`) is a single 9-cell `TopLeft..BottomRight` enum on the parent that resolves to `(align_x, align_y)` fractions in `[0,1]` (`layout.rs:79`).

`Units` (`types.rs:108`) on the child, used identically for `width`, `height`, the four edge spacings (`left/right/top/bottom`), gap, min/max:
- `Pixels(f32)` — fixed.
- `Percentage(f32)` — of parent's axis dimension. Resolved via `to_px(parent_value, default)` (`types.rs:137`).
- `Stretch(f32)` — factor over remaining free space (post-fixed-children).
- `Auto` (default) — for sizes, hug-children-or-content; for spacing, "let the parent's padding decide on this side."

`PositionType` (`types.rs:70`): `Relative` (in-stack, default) or `Absolute` (out-of-flow, edge-anchored within parent's padded box). Absolute children don't contribute to a parent's auto size.

## 2. `Stretch` ≠ `flex-grow`

This is morphorm's key divergence from flexbox and the source of its blog-post claim "subview-based stretching." Two differences:

(a) **`Stretch` is a sizing unit, not a modifier on a base size.** In flexbox a child has `flex-basis` (a length) plus `flex-grow` (a factor that distributes leftover). In morphorm, choosing `Stretch(2.0)` *replaces* the size — there is no basis. Free space on the main axis = `parent_main - sum(non-stretch sizes) - sum(non-stretch gaps)`, distributed proportionally across `Stretch` items by factor (`layout.rs:483-513`). With min/max clamping handled via the standard "freeze violations, redistribute" loop (`layout.rs:497-512`) — the same technique flexbox uses for `resolve_flexible_lengths`.

(b) **The four edge spacings (`left`, `right`, `top`, `bottom`) and gaps can themselves be `Stretch`.** A child with `left: Stretch(1.0), right: Stretch(1.0)` is centered. `left: Auto, right: Stretch(1.0)` pushes it to the trailing edge. This collapses `justify-content` and `align-self` into a single uniform mechanism: stretch-spacing competes with stretch-sizes in the *same* free-space distribution loop. `StretchItem` (`layout.rs:19-44`) carries `item_type: ItemType::{Size, After}` — both kinds go into the same `main_axis: SmallVec<StretchItem>` list and are resolved together (`layout.rs:1251`, `1294-1320`). One pass, all stretch participants ranked by factor.

In flexbox you get centering by setting parent `justify-content: center`. In morphorm you get it by setting *the child's own* leading and trailing space to `Stretch(1.0)`. Locality of authority flips from parent to child.

## 3. Self vs child layout — child-position vs self-position units

Morphorm separates "what spacing do my children get?" (parent's `padding_*`, set on the parent via `padding_left/right/top/bottom`, `node.rs:123-132`) from "what spacing do I want around myself?" (child's own `left/right/top/bottom`, `node.rs:101-110`).

The interaction rule: a child's `Auto` edge spacing defers to the parent's `padding_*` on that side. So a child says `left: Auto` and the parent's `padding_left: Pixels(8)` wins; if the child says `left: Pixels(4)`, it overrides. This is implemented through `padding_main_before/after` and `cross_before/after` direction-agnostic accessors (`node.rs:277-305`) plus the `select_unwrap_default(..., Auto)` defaults on edges (`node.rs:251-275`).

Result: the parent specifies a default gutter; any child can opt out per-side without the parent caring. No `margin: auto` collisions because morphorm has no margin — only the symmetric `left/right/top/bottom` × `Pixels|Percentage|Stretch|Auto` matrix.

## 4. The single-pass algorithm

Entry point: `Node::layout(...)` (`node.rs:42`) on the root node; recurses depth-first via the dispatch fn `layout()` (`layout.rs:1119`). Per non-leaf, non-grid, non-overlay node:

1. Resolve `main`/`cross` from `parent_main`/`parent_cross` for non-stretch sizes (`Pixels` → val, `Percentage` → ratio, `Stretch` → full parent extent, `Auto` → 0 then later content-size) (`layout.rs:1157-1170`).
2. If `Auto` on a leaf, call `content_sizing(parent_w, parent_h)` (`node.rs:114-120`) — the host's text-measure hook — and use its return for `min_main`/`min_cross` (`layout.rs:1198-1218`). Same hook is also where text-wrap-width-dependence is expressed (parent passes `Some(width)` so text can wrap to it).
3. Subtract padding + border to get content box (`layout.rs:1262-1263`).
4. **First child walk** (`layout.rs:1278+`): for each *non-stretch* child, call `layout(child, ...)` recursively to determine its size; for *stretch* children, append a `StretchItem::Size` to `main_axis` and skip recursion. Same for stretch gaps — `StretchItem::After`. Also accumulate `main_used` (sum of fixed sizes + fixed gaps).
5. **Stretch resolution loop**: free space = `parent_main - main_used`, divided by `main_flex_sum`. Each item gets `factor * free / sum`, clamped to its min/max. Violations refreeze and re-divide until no violations remain (the flexbox `resolve_flexible_lengths` pattern).
6. **Second child walk** for the now-resolved-size stretch children: recurse into `layout(child, ...)` with the resolved main extent, so they can lay out their own subtrees with a known size.
7. Position pass: walk children in order, accumulating positions main-axis-wise, computing cross-axis offset from `Alignment` × `(parent_cross - child.cross)` plus child's own cross-edge spacing (`layout.rs:79`, `:284-305` for the overlay variant). Write rects via `cache.set_rect(...)`.

Steps 4-6 mean stretch children are *visited twice*: once to be deferred, once to be sized. Non-stretch children are visited once. This is "single pass" in the WPF/CSS sense — there is no separate measure phase that runs over the whole subtree before arrange. But within one node's children, stretch siblings cause a re-recursion. For deep stretch chains (`Stretch` ⇒ `Stretch` ⇒ `Stretch`), the second recursion does the real work and the first is essentially a typecheck.

`Overlay` (`layout.rs:117`) is different: it does a two-pass *stabilization* loop (`layout.rs:180`, `for _ in 0..2`) because auto-sizing in overlay needs to know `max(child_size)` first, which requires laying out children, which may depend on the container size. Two iterations are sufficient because it's a fixpoint over `min`/`max` clamping, not a free-space problem.

`LayoutWrap::Wrap` (`types.rs:237`, `layout.rs:635`) is a separate path that does line-breaking on the main axis. Stretch children inside a wrap container contribute their `min_main` to the break decision (`layout.rs:760`) and get sized per-line in phase 3 (`layout.rs:779+`). This is morphorm's approximation of `flex-wrap: wrap`.

## 5. Caching

`Cache` (`cache.rs:9`) is a five-method trait the host implements: `width`, `height`, `posx`, `posy`, `set_bounds(node, x, y, w, h)`. That's it — no input-keyed memoization à la Taffy's 9-slot `compute_cache_slot`. Morphorm doesn't cache layout *inputs*; it just stores the *output* rect. Re-running layout always re-recurses, even if nothing changed.

The `Node::CacheKey` associated type (`node.rs:24`) lets the host use a slotmap key or arena index instead of `&Self` to identify nodes in the cache (the README ECS example uses `Entity`).

This is one of morphorm's simplifications relative to Taffy: no input-keyed memoization, no dirty bits, no skipping on idle. Vizia compensates at the styling layer (track which nodes' styles changed, only re-layout affected subtrees), but morphorm itself runs every time.

## 6. What it does well vs what it can't express

**Well:**
- Centering, spacing, edge-anchoring all expressed by giving stretch units to space *and* size in the same factor pool — uniform and learnable.
- `Auto` cascading from parent to child for spacing means sane defaults without ceremony.
- `content_size` hook is a clean place for text and aspect-ratio constraints.
- Grid is real: explicit `grid_columns`/`grid_rows: Vec<Units>`, `column_start/span`, `row_start/span` (`node.rs:182-192`), with stretch tracks resolved by the same violation-freeze loop (`layout.rs:475-555`).
- Single layout type per parent; no `flex-direction` matrix to mentally track.

**Can't express (or expresses awkwardly):**
- **Baselines** — no baseline alignment. `Alignment` is geometric only.
- **Inline / mixed content / text flow** — no shaping, no inline boxes. Text is one leaf with a `content_size`.
- **Cross-axis stretch with intrinsic min** without min-cross plumbing — flexbox's `align-items: stretch` with `min-cross-content` falls through `min_cross: Auto + content_sizing`, which only works for childless leaves.
- **Wrap with stretch sizing** is approximate (uses `min_main` for break decisions, `layout.rs:760`); a child that *would* be small after stretch resolution may force a break it shouldn't.
- **Margin collapse** — there is no margin, just `left/right/top/bottom` spacing, which never collapses. (Probably correct: collapsing is a CSS quirk you don't want.)
- **Min-content / max-content sizing** — no equivalent of CSS `min-content`. `min_width: Auto` means "sum/max of children" (`README.md:121`), which is closer to `max-content` for a single-line container but with no way to query "what's the longest unbreakable word."
- **Float, table, multi-column, position: fixed** — out of scope.

## 7. Lessons for Palantir

Morphorm's units are the closest external analogue to our `Sizing::{Fixed, Hug, Fill}`:
- `Pixels(n)` ≈ `Fixed(n)` — exact match.
- `Auto` ≈ `Hug` — exact match (both mean "size to content/children").
- `Stretch(f)` ≈ `Fill` — close but the factor matters; we currently distribute equally across `Fill` siblings, morphorm by ratio.
- `Percentage(f)` — we don't have it.

**Should we add `Percentage`?** Probably yes, eventually. Our `Sizing` enum is closed (`geom.rs`), so adding `Percentage(f32)` is one variant + one branch in `resolve_axis`. It composes cleanly: `Percentage` is concrete-sizing (like `Fixed`) once the parent extent is known, so it slots in before `Fill` distribution. The lib tests in `lib.rs` would gain a few cases but the contract is clean. Hold off until a real need appears (probably when we get to dialogs, splitters, or scrollable panes where "30% of viewport" is the natural spec) — the prototype hasn't asked for it yet.

**Should we adopt morphorm's `Stretch` factor?** Mild yes. Our `Sizing::Fill` distributes equally; morphorm's `Stretch(f)` allows weighted distribution (1:2:1 ratios). Cheap to add — `Fill(f32)` with default `1.0`, change `resolve_axis` to divide by sum-of-factors instead of count. Doesn't change anything about callers that pass plain `Fill`. Worth doing pre-emptively because retrofitting changes API.

**Don't adopt the stretch-on-spacing trick.** Morphorm's "child sets `left: Stretch, right: Stretch` to center itself" is elegant in isolation but conflates sizing and alignment. WPF's split (`HorizontalAlignment` on the child, slot-shrink in `ArrangeCore`) is what we already model. Mixing stretch-spacing into our `Spacing` field would mean `Spacing` becomes `Sizing`-flavored, which doubles the resolution algorithm. Keep `HorizontalAlignment`-style alignment in the parent's arrange logic; users express centering via alignment, not via stretchy paddings on the child.

**Copy: `Auto` on edge spacing inheriting parent padding.** Morphorm's rule "child's `left: Auto` defers to parent's `padding_left`" is a small win we don't currently have — our `Spacing` is just margin-like, doesn't fall back to parent padding. Would need a `Spacing::Auto` variant. Probably not worth it before we have multi-level container nesting in real use.

**Avoid: the single-pass + recurse-twice-on-stretch shape.** Morphorm pays for "single pass" by visiting stretch subtrees twice during their parent's layout. We already do clean two-pass measure→arrange and stretch siblings cost one extra cross-axis assign in arrange. Don't fold our two passes into one to chase the morphorm model — WPF's measure/arrange is structurally simpler once you accept the two-walk shape, and easier to extend (Grid, MinMax, Margin all live in the wrapper without breaking the contract).

**Avoid: not caching inputs.** Morphorm's `Cache` only stores outputs. Fine for a system with external dirty tracking (Vizia diffs styles); a poor fit if we ever add idle-frame skipping. Our current "rebuild every frame" matches morphorm's eager recompute, but we should leave room for a Taffy-style input-keyed cache later.

**Single biggest takeaway:** morphorm validates that `Pixels | Percentage | Stretch | Auto` is a reasonable user-facing alphabet for both sizes and gaps — closer to our `Sizing` than CSS is. Adding `Percentage` (and weighting `Fill`) brings us to feature-parity on the units side without taking on the unification-of-sizing-and-spacing that gives morphorm its odd corners. The bits to skip are the unification gimmick and the input-cacheless model.
