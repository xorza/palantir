# Stretch — Reference for Palantir

Stretch (vislyhq/stretch) was a Rust port of Yoga's Flexbox, built for the Visly design tool. Last commit `6879b9a` on 2020-05-22; the repo went silent shortly after, was unmaintained for over a year, and was forked into Taffy by DioxusLabs in late 2021. Source in `tmp/stretch/`. Eleven files, 2709 lines total — `src/algo.rs` alone is 1316.

## 1. Yoga port: what was kept, what diverged

The algorithm is a near-literal Rust translation of Yoga's `CalculateLayout.cpp`. `algo.rs:117 compute_internal` is the same big function with the same numbered "9.2. Line Length Determination" / "STEP 1..N" comments lifted from the spec. Style enums (`AlignItems`, `JustifyContent`, `FlexWrap`, `Display`, `PositionType`) all match Yoga's set. Box model is `border-box` always, like Yoga, like React Native.

What diverged: no errata flags, no baseline callback, no `direction` (LTR/RTL) — `Style` (`src/style.rs`) has none of Yoga's RTL plumbing. `aspect_ratio` is a `Number` field but applied in only one place (`algo.rs:275`); Yoga's per-step ratio handling is missing. Several spec clauses are stubbed `// TODO - Probably need to cover this case in future` (`algo.rs:290, 298`). So: a *partial* Yoga port, frozen mid-flight.

## 2. Node ownership and id model

Two-level identity, much messier than Taffy's:

- `node::Node` (`src/node.rs:23`) is `{ instance: Id, local: Id }` — a globally unique handle. `Stretch::new` pulls an `instance` id from a process-global `INSTANCE_ALLOCATOR` (`src/node.rs:19`, `src/id.rs:14` — atomic counter, never freed: `Allocator::free` is `pub fn free(&self, _ids: &[Id]) {}`).
- `Stretch` (`src/node.rs:28`) holds two `HashMap`s — `nodes_to_ids: Map<Node, NodeId>` and `ids_to_nodes: Map<NodeId, Node>` — translating between the public `Node` handle and the internal `usize` index into `Forest`.
- `Forest` (`src/forest.rs:30`) is the actual storage: parallel `Vec<NodeData>`, `Vec<ChildrenVec<NodeId>>`, `Vec<ParentsVec<NodeId>>` — three vecs indexed by the same `NodeId = usize` (`src/id.rs:7`).

So every public call hashes a `Node` into a `NodeId` (`find_node`, `src/node.rs:68`) before doing anything. Every node carries its parents in a `ParentsVec` because the API allows attaching the same node to multiple parents (`add_child` doesn't check). `swap_remove` (`forest.rs:77`) has to walk every parent of every child to fix up indices — 70 lines of pointer-chasing. This is the "ECS-like" structure the file comment claims, but in practice it's a hand-rolled slot table with two redundant hashmaps glued on.

## 3. Cache strategy (or lack thereof)

One slot. `NodeData::layout_cache: Option<Cache>` (`forest.rs:16`), `Cache` is `{ node_size, parent_size, perform_layout, result }` (`src/result.rs:18`). Yoga has *eight* measurement slots plus a layout slot for a reason — flex resolution remeasures children under multiple `SizingMode`s in one pass — and Stretch threw that away.

The lookup (`algo.rs:127-148`) tries to compensate with two ad-hoc rules: width/height "compatible" if the requested dimension equals the cached output (`sys::abs(width - cache.result.size.width) < f32::EPSILON`), or full equality on `(node_size, parent_size)`. The first rule is the only thing that lets re-entry during flex resolution hit at all. Anything else evicts.

`mark_dirty` (`forest.rs:157`) recursively walks `parents` and clears the cache, so cross-frame retention works in principle — but with one slot and no `MeasureMode`/`SizingMode` keying, intra-frame hit rate on a non-trivial flex layout is poor. Performance was the consequence (Visly app users reported slow layout on complex screens; never benchmarked vs Yoga in-tree).

## 4. Why abandoned

Last commit `6879b9a` in May 2020 merged a small PR. After that: nothing. Issues piled up unanswered through 2020 and 2021 (text measurement edge cases, percent in indefinite contexts, `flex-wrap` corner bugs, `min/max-size` interactions). Visly the company wound down its design tool and the maintainer stopped responding. The repo still exists as read-only.

Concrete things that didn't work:

- Single-slot cache → degenerate measure recursion on real flex trees.
- The `Node`-to-`NodeId` double indirection cost an `O(1)` hashmap probe on *every* style getter and child traversal — measurable.
- Two `unsafe fn`s (`forest.rs:145 remove_child`, called from `node.rs:156`) marked unsafe with no documented invariant — vestigial, not load-bearing.
- `INSTANCE_ALLOCATOR` is a leaky atomic that never frees ids; multiple `Stretch::new`/`drop` cycles in long-running processes (an editor) monotonically grow the counter.
- `no_std` support via `heapless` (`src/sys.rs:58`) hardcoded `MaxNodeCount = U256`, `MaxChildCount = U16` — unusable for any real UI, but in the public surface.

## 5. What Taffy fixed

DioxusLabs forked Stretch as Taffy in October 2021 (Nico Burns's blog post "Taffy: a new Flexbox layout engine for Rust"). The fork was a near-rewrite within months. The deltas, item by item:

- **One handle, no double map.** `taffy::NodeId(u64)` (`taffy/src/tree/node.rs`), no `(instance, local)` pair, no parallel `nodes_to_ids` / `ids_to_nodes`. Built-in `TaffyTree` uses `slotmap`.
- **Tree separated from algorithm.** Taffy's algorithms are generic over the `LayoutPartialTree` trait (see `references/taffy.md` §1) — the host owns the tree. Stretch hard-coded `Forest` and could not be reused without paying for both layers.
- **Real cache.** Nine slots keyed by `(known_dimensions, available_space)` with `compute_cache_slot` (`taffy/src/tree/cache.rs`), tolerant lookup via `is_roughly_equal` — restoring Yoga's caching semantics that Stretch had dropped.
- **`AvailableSpace` enum** (`Definite | MinContent | MaxContent`) replaces Stretch's bare `Number` — actual `SizingMode` keying, which the cache needs to be useful.
- **Grid added** (`taffy/src/compute/grid/`), then Block, then partial absolute. Stretch was Flex-only and stalled there.
- **`unsafe` removed.** Taffy is `#![forbid(unsafe_code)]`. Stretch's two `unsafe fn`s are gone.
- **`no_std` redone.** Taffy's `alloc` feature works without `std` on real-sized trees; Stretch's heapless mode was a dead end.
- **Spec corners filled.** Min/max sizing, percent in indefinite contexts, aspect-ratio in the right places, `gap`, `position: absolute` — all the Stretch `// TODO` clauses got real implementations.
- **Pixel rounding fixed** (commit `aa5b296`). Stretch and Yoga round per-node accumulating gaps; Taffy rounds against cumulative coordinates (`algo.rs:107-110` shows Stretch's per-node-additive approach — same bug).

## 6. Lessons for Palantir

1. **Don't build a `(instance_id, local_id)` handle plus dual hashmap.** Either the host owns the tree (Taffy) or you own a `Vec<Node>` with `usize` indices (our current approach in `src/tree.rs`). The middle ground costs a hash probe per access and buys nothing. We're already on the right side; stay there.

2. **A single-slot layout cache is worse than no cache.** If we ever add flexbox semantics, port Taffy's 9-slot scheme — not Stretch's `Option<Cache>` with epsilon-compare. Yoga §3 in `references/yoga.md` already argued this; Stretch is the empirical proof.

3. **Atomic global instance counters with no-op `free` are a leak.** If Palantir ever needs per-window/per-doc instance ids, use a real freelist or just drop the concept. Our `WidgetId` is already a content hash; no global counter exists. Keep it that way.

4. **Translating a 2500-line C++ algorithm into Rust verbatim doesn't make it Rust.** Stretch's `algo.rs:117 compute_internal` reads like Yoga's `CalculateLayout.cpp`. The borrow checker forced it into a single mega-function with `self.nodes[node].style` repeated everywhere because it can't hold a `&Node` across child recursion. Taffy split this by trait — clean. If we ever port a complex algorithm in, refactor as we go.

5. **`unsafe fn` without a documented invariant is a smell, not a feature.** Stretch's `forest.rs:145` is `pub unsafe fn` for no apparent reason — `remove_child_at_index` next door is safe and does the same dangerous swap. Don't sprinkle `unsafe` for vibes.

6. **The reason Stretch died is that no one was paid to maintain it.** Visly stopped using it; the project had no community contribution model. Worth remembering before adopting any single-vendor layout crate (Taffy is multi-vendor — Dioxus, Bevy, Zed-adjacent — which is why it survived).

**Bottom line:** Stretch is a museum piece. Useful as a cautionary example of what *not* to copy from Yoga (the verbatim algorithm port, the inadequate cache, the double-id model). For anything real, look at Taffy. For our WPF-aligned model, neither is a template — we already made the better choice by not porting flexbox at all.
