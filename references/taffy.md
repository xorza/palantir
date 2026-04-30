# Taffy — Reference Notes for Palantir Integration

Taffy (DioxusLabs) is a pure-Rust layout engine implementing CSS Block, Flexbox, and Grid. It is the layout backend used by Dioxus, Bevy UI, Zed/Lapce-adjacent projects, and (via `egui_taffy`) egui. It is *not* a renderer and *does not own a tree* — it operates over any structure that implements its traits.

## 1. Tree abstraction (`src/tree/traits.rs`)

Taffy inverts ownership: the host owns the tree, Taffy is handed `&mut tree` for one layout pass. Three traits form a stack:

- `TraversePartialTree` — minimum: `child_ids(NodeId) -> ChildIter`, `child_count`, `get_child_id(parent, index)`. This is all the algorithms strictly require.
- `TraverseTree: TraversePartialTree` — marker promising recursion is safe (needed for `RoundTree` / `PrintTree`).
- `LayoutPartialTree: TraversePartialTree` — the load-bearing trait:
  ```rust
  fn get_core_container_style(&self, id: NodeId) -> Self::CoreContainerStyle<'_>;
  fn set_unrounded_layout(&mut self, id: NodeId, layout: &Layout);
  fn compute_child_layout(&mut self, id: NodeId, inputs: LayoutInput) -> LayoutOutput;
  ```
  Plus per-algorithm extension traits `LayoutFlexboxContainer`, `LayoutGridContainer`, `LayoutBlockContainer` that expose typed style getters (`get_flexbox_container_style(id)` etc.) so the host's style storage is opaque to Taffy.
- `CacheTree` — `cache_get` / `cache_store` / `cache_clear` per `NodeId`.

`NodeId` is `pub struct NodeId(u64)` (`src/tree/node.rs`). It is host-defined: an arena index, a slotmap key, or a raw pointer all work. The example `examples/custom_tree_vec.rs` uses `Vec<Node>` indices; `custom_tree_owned_unsafe.rs` uses pointers; the built-in `TaffyTree` uses `slotmap::SlotMap`.

The host's `compute_child_layout` is the dispatch site: it inspects the node's `Display` and calls one of `compute_flexbox_layout`, `compute_grid_layout`, `compute_block_layout`, `compute_leaf_layout`, or `compute_hidden_layout`, almost always wrapped in `compute_cached_layout(self, id, inputs, |_,_,_| { … })` so caching is consulted first.

## 2. Algorithms (`src/compute/`)

- **Flexbox** (`flexbox.rs`): full CSS Flexbox spec — `flex-direction`, `wrap`, `flex-grow/shrink/basis`, `gap`, `align-*`, `justify-*`. High fidelity to the web; passes WPT-derived fixtures in `test_fixtures/`.
- **CSS Grid** (`grid/`): tracks (`fr`, `auto`, `min-content`, `max-content`, `minmax`, `fit-content`), `repeat()`, named lines, named areas, `grid-auto-flow`, `gap`. Most complete spec-compliant Grid implementation in Rust.
- **Block** (`block.rs`, behind `block_layout` feature): basic CSS block flow + margin collapse. Floats are gated on `float_layout`.
- **Leaf** (`leaf.rs`): nodes without layout children — text, images, custom-measured content.

Notably **missing**: inline/flow text layout, tables, multi-column, positioned (`position: absolute`) layout is partial, no shaped text. Taffy is "boxes only" — text shaping is out of scope.

## 3. Intrinsic sizing — `MeasureFunc` and `available_space`

There is no `MeasureFunc` *trait* per se; the leaf algorithm `compute_leaf_layout` takes a closure:
```rust
MeasureFunction: FnOnce(Size<Option<f32>>, Size<AvailableSpace>) -> Size<f32>
```
Args: `known_dimensions` (definite from parent) and `available_space` per axis. `AvailableSpace` (`src/style/available_space.rs`) is the integration crux:
- `Definite(f32)` — definite parent-imposed size.
- `MinContent` — return the smallest size you can manage (longest unbreakable word for text).
- `MaxContent` — return your unconstrained natural size (text on one line).

This is when Taffy calls into the host: during flex/grid sizing, when the algorithm asks "what do you need?", it forwards through `compute_child_layout` → host dispatch → `compute_leaf_layout(inputs, &style, calc, |kd, av| measure(...))`. The host plugs text/image measurement here. `TaffyTree` exposes a `Context` generic for this; custom trees just close over `&node.text_data` etc. (see `examples/custom_tree_vec.rs`, `cosmic_text/` example).

## 4. Caching (`src/tree/cache.rs`)

Each node owns a `Cache` containing one `final_layout_entry: Option<CacheEntry<LayoutOutput>>` plus a 9-slot array `measure_entries: [Option<CacheEntry<Size<f32>>>; 9]`. Slots are picked by `compute_cache_slot(known_dimensions, available_space)`:
- slot 0: both dimensions known
- slots 1–4: one dimension known, other axis is min-content vs max-content/definite
- slots 5–8: neither known, 2×2 over (min-content, max/definite) per axis

Lookup tolerates "known dimension equals previously cached output dimension" via `is_roughly_equal`. `compute_cached_layout` (`compute/mod.rs`) wraps every recursive call: hit → return; miss → compute, store, return. The cache is per-node and **persists across layout runs** as long as the host keeps the `Cache` alive — re-running layout with unchanged inputs is nearly free. Style mutation must explicitly call `cache_clear` on affected nodes (this is what `TaffyTree::set_style` does, propagating to ancestors).

## 5. `compute_root_layout`

Single entry point: `compute_root_layout(tree, root, available_space)`. It pre-resolves block-mode root size (margin/padding/border, min/max clamping, aspect-ratio, RTL), then calls `tree.perform_child_layout(...)` on the root and writes `set_unrounded_layout(root, ...)`. Optionally followed by `round_layout(tree, root)` (requires `RoundTree`) — rounds against cumulative coordinates rather than per-node, avoiding the gap bug Yoga had (see referenced commit `aa5b296`).

## 6. Performance

Per the README benchmarks (M1 Pro, criterion): Taffy is on par with Yoga at moderate sizes and faster on deep trees ("big trees deep" 100k nodes: 63.8ms vs Yoga 76.8ms; "super deep" 1k×1k: 472µs vs 555µs). Slower than Yoga on extremely wide flat trees (100k: 247ms vs 135ms) — flex sizing iterates flat children. Time goes into: flex `resolve_flexible_lengths` iteration, grid track sizing (multi-pass per spec), and `compute_cached_layout` lookup overhead. Tree creation is *not* measured. `Cache` lookup is `O(9)` linear scan in `ComputeSize` mode. Comparison points: Morphorm (simpler, no Grid, faster but weaker semantics); Yoga (Flexbox only, C++).

## 7. Style model

`Style` (`src/style/mod.rs:430`) is a flat `#[derive(Clone)]` struct, ~50 fields. Always longhand — `padding: Rect<LengthPercentage>`, `margin: Rect<LengthPercentageAuto>`, `size/min_size/max_size: Size<Dimension>`, `inset: Rect<LengthPercentageAuto>`, `gap: Size<LengthPercentage>`, plus `display`, `position`, `overflow: Point<Overflow>`, `box_sizing`, `direction`, `aspect_ratio`, flex fields, `grid_template_*`, `grid_auto_*`, `grid_row/column`. No CSS shorthand parsing; the optional `parse` feature handles individual values. `Dimension` = `Length(f32) | Percent(f32) | Auto`. The host can keep its own style type and project to `CoreStyle` / `FlexboxContainerStyle` / etc. on demand — those are *traits*, not the `Style` struct.

## 8. Integration patterns in the wild

- **Dioxus** (`packages/native-core` / `blitz`): owns a DOM, implements the layout traits over its `RealDom`, plugs CosmicText-based measure for text leaves.
- **Bevy UI**: `bevy_ui` keeps its ECS world; a `LayoutContext` resource holds a `TaffyTree<Entity>`, and a sync system mirrors UI entity hierarchy + Style components into Taffy each frame.
- **egui_taffy**: rebuilds a `TaffyTree` per frame from egui widget calls, runs Taffy, then positions egui widgets at the resulting rects. Demonstrates per-frame rebuild is viable for small UIs.
- **Boilerplate that is unavoidable**: implementing `child_ids` iterator + `get_core_container_style` projection + the Display dispatch in `compute_child_layout`. Roughly 100–200 lines for a custom backend (see `examples/custom_tree_vec.rs`).

## 9. Lessons for Palantir

**Yes to a feature flag**, no to replacing the WPF panels. Reasons:

1. **Taffy's contract is close to WPF measure/arrange but not identical.** `LayoutInput.run_mode` of `ComputeSize` is the measure pass; `PerformLayout` writes the rect (the arrange equivalent). `available_space: Size<AvailableSpace>` maps cleanly to our `Sizing::Hug` (use `MaxContent`) and `Sizing::Fixed/Fill` (use `Definite`). So a `taffy` feature gate that wires our `Tree` into `LayoutPartialTree` is mechanical.

2. **A `LayoutPartialTree` over our arena** would store `taffy::Style` and `taffy::Cache` alongside our `Node` — or in a parallel `Vec<TaffyNodeData>` indexed by `NodeId`. `child_ids` walks our `first_child` / `next_sibling` linked list (build a small `ChildIter` adaptor — already trivial since we have `ChildIter`). `NodeId` = `u64::from(our NodeId)`. Custom widgets with intrinsic content (Button label, Image) become Taffy leaves whose host dispatch calls `compute_leaf_layout(inputs, style, calc, |kd, av| self.measure_text(id, kd, av))`.

3. **The cache invalidation pitfall is real.** We rebuild the tree every frame; Taffy's caching is keyed by `(NodeId, LayoutInput)` and only pays off if the same `NodeId` carries the same style + same children across frames. Two options:
   - **Discard caching**: store `Cache::new()` per node each frame. Cheap (the cache is 9 small entries), and Taffy still works correctly — you just lose intra-frame memoization within a flex/grid where one child is sized multiple times. Probably worth keeping for that reason.
   - **Stable IDs**: persist `Cache` in our state map keyed by `WidgetId` (we already hash call-site + user key for that). Reuse across frames; clear on style diff. This is the integration model Bevy uses.
   Going with the first is the right v1: rebuild includes fresh `Cache::new()`, intra-frame memoization is preserved, no cross-frame staleness bugs.

4. **Keep `HStack`/`VStack`/`Dock` native.** They're <100 LOC each, debuggable, fit the WPF semantics exactly, and avoid pulling Taffy's `Style` into the public API. Taffy is the answer for `ui.flex(...)` and `ui.grid(...)` containers where users want CSS semantics — those nodes set `display: Flex|Grid` and delegate measure/arrange to `compute_root_layout` for that subtree (Taffy is fine being run on a subtree if rooted on a node whose size is given).

5. **Mixing engines per subtree works** because Taffy's traits only require access to *one container and its direct children*. A native `VStack` whose child happens to be a Taffy-laid-out `flex` container just calls into Taffy for that node and treats the result as the child's measured size — exactly the WPF measure protocol. The reverse (Taffy parent, native child) requires the native child to expose a `MeasureFunc`-style closure as a leaf.

6. **Style cost.** `taffy::Style` is ~200 bytes; cloning per node per frame is fine for thousands of nodes but profile if we hit 100k. The `CoreStyle` traits exist precisely so we can implement them on a thinner Palantir style and avoid ever materialising `taffy::Style`.

**Bottom line**: feature-flag Taffy as `palantir/taffy`; expose `ui.flex(|ui| ...)` and `ui.grid(|ui| ...)` containers backed by it; native panels stay primary. Implement `LayoutPartialTree` + `LayoutFlexboxContainer` + `LayoutGridContainer` over our arena, with per-frame fresh `Cache`s, and `compute_leaf_layout` for widget intrinsic sizing.
