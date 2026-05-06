# `src/layout/` review

Scope: `mod.rs`, `axis.rs`, `intrinsic.rs`, `support.rs`, `result.rs`, `cache/`, `stack/`, `wrapstack/`, `zstack/`, `canvas/`, `grid/`, `types/`. Review-only — no fixes applied.

Overall: the architecture is healthy. Sizing is consistent across drivers, the freeze loop is well-confined to Stack, and the cache key scheme (`(WidgetId, subtree_hash, available_q)`) is right. The findings below are mostly seam-tightening and duplication.

## Architectural issues

### A1. Measure cache "knows about" Grid internals
`mod.rs:267-278` and `mod.rs:340-347`. The cache hit path explicitly checks `tree.hashes.subtree_has_grid` and restores per-grid hug arrays via `scratch.grid.hugs.restore_subtree`. The miss path snapshots them with `snapshot_subtree`. This is the only driver-specific carve-out in the otherwise driver-agnostic measure pipeline, and `Tree::hashes` carries an entire `subtree_has_grid` bitset just to make the carve-out cheap.

The coupling is real: any future driver that publishes per-descendant scratch (animation runs? grid v2?) will need its own bitset, its own snapshot/restore pair, and its own conditional in `measure`. Better shape: a `SubtreeArenas` slot per driver registered via a trait, or — simpler — put `hugs` inside the cache's per-snapshot payload and let drivers blindly read/write through a `&mut Scratch` they get back from the engine. Don't redesign now; flag the next addition as the trigger.

### A2. `measure_dispatch` owns padding/margin shrinking; drivers re-derive `inner`
`mod.rs:404-478` (per agent; verify if changed) computes `inner_avail` from `style.size` and padding/margin, then dispatches. Each driver still calls `child_avail_per_axis_hug(style.size, inner_avail)` (canvas:27, zstack:47) and grid recomputes its own per-track allocations. The shared step in `mod.rs` is a duplicated effort — neither owning it cleanly nor delegating it cleanly. Either push padding/margin entirely into drivers (drivers see the parent's outer rect and slice), or have the engine pass the fully shrunken `inner` and forbid drivers from re-shrinking. Pick one.

### A3. `LayoutResult.available_q` is dual-written: per-frame at `mod.rs:254`, per-snapshot in the cache
On a miss, `available_q[node]` is written before dispatch *and* the same range is later snapshotted into the cache. On a hit, the snapshot's `available_q` slice is copied back to `result.available_q`. The two writes converge on the same value today, but no test pins "snapshot's root `available_q` == this-frame's `cache_avail`". Add an `assert_eq!` in `write_subtree` to catch refactors that drop one side.

## Simplifications

### S1. Canvas vs ZStack `measure` is the same loop with one extra term
`canvas/mod.rs:19-41` and `zstack/mod.rs:39-56`: identical `child_avail_per_axis_hug` + active-children fold; canvas adds `pos.x` / `pos.y`, zstack doesn't. Extract:

```rust
fn measure_per_axis_hug(layout, tree, node, inner_avail, text, contrib: impl Fn(NodeId, Size) -> Size) -> Size
```

Canvas passes `|c, d| Size::new(pos.x + d.w, pos.y + d.h)`, ZStack passes `|_, d| d`. ~25 lines saved across measure + intrinsic. Arrange differs (canvas places by `position`, zstack by `align`) — leave it.

### S2. Stack and WrapStack share the justify-to-(offset, gap) translation
`stack/mod.rs:226-238` and `wrapstack/mod.rs:205-216` (per agent; both files have a 12-line match on `Justify` that returns `(start_offset, gap)`). Extract a free fn in `support.rs`:

```rust
struct JustifyOffsets { start: f32, gap: f32 }
fn justify_offsets(j: Justify, leftover: f32, gap: f32, count: usize) -> JustifyOffsets
```

(Named struct, per CLAUDE.md "no tuple returns".) Both drivers call it; one source of truth for spacing semantics. jscpd flags this clone.

### S3. Grid row/col intrinsic phases mirror each other
`grid/mod.rs` Phase-1 column-intrinsic walk vs the row variant (jscpd flagged a 9-line clone around lines 398/559). Parameterize by `Axis`. This is the third `for axis in [X, Y]`-style duplication in the file — at some point the whole grid measure deserves an axis-loop refactor, but the local extraction is cheap and lossless.

### S4. `LayoutResult::available_q(id) -> Option<AvailableKey>`
`result.rs:58-62` does sentinel-unwrap (real work, not trivial) but is on a hot per-node path read by the encoder. Either inline the sentinel at call sites (cheap, kills indirection) or leave alone; either way, it's borderline OK under the no-trivial-accessor rule because it does conversion. Note as "borderline, leave unless profiling motivates", not a fix.

### S5. `leaf_text_shapes` iterator with two for-loop callers
`support.rs:29-48`. Two consumers, both `for shape in leaf_text_shapes(...)` with no map/filter chains. The iterator hides 8 lines of slice logic per call site; two for-loops would also hide it. Marginal — keep, but don't add a third caller without revisiting.

## Smaller improvements

- `axis.rs` is small and load-bearing; consider pinning the `Axis::main_v` / `cross_v` mapping with a one-line table-driven test if not already covered. Easier than chasing a swap mistake later.
- `types/grid_cell.rs` is 22 lines and `types/justify.rs` is 20; both could move into `types/mod.rs` if you ever consolidate. Not today — splitting is correct now that types/ is a folder.
- `intrinsic.rs` and `intrinsic.md` should be cross-checked: docs drift fast (CLAUDE.md says "Docs are starting positions, not commitments"). Keep an eye on whether `intrinsic.md` still describes the implemented algorithm.
- WrapStack cross-axis intrinsic is a known conservative over-estimate (`wrapstack/mod.rs:341-349`). Documented; leave until a workload demands height-given-width.
- `cache/` — three parallel arenas (`desired` / `text_shapes` / `available_q` / `scroll_content`) plus grid hugs — the invariant `desired.live == text.live == available.live == scroll.live` is asserted via shared `LiveArena`. Good. If a fourth parallel arena lands, that's the trigger to extract a `MultiArena<T>` rather than adding a fourth field.
- Style: visibility, no inline `crate::` paths, no re-exports, no test-only methods on production types — all clean across the layout tree. Good.

## Open questions

1. **Pad/margin shrinking ownership** (A2): is the current split deliberate (engine shrinks for the cache key, drivers re-derive for their own reasons)? If so, comment it. If not, pick a side.
2. **Grid hugs in the measure cache** (A1): when the second non-trivial driver-scoped scratch arrives, do you want to keep extending the carve-out or refactor to a per-driver snapshot trait? Worth an answer before adding the third one.
3. **Two-pass measure / WPF growth** (`mod.rs:308-312` comment): still officially deferred? If yes, leave the single-dispatch comment as the contract statement and don't add speculation.
4. **WrapStack cross-axis intrinsic**: any near-term workload that actually exercises a wrap-stack of mixed-height items inside a Hug parent? If yes, the conservative bound becomes a layout bug; if no, leave.

## Top 5 if you say "go"

1. **Extract `justify_offsets`** (S2) — smallest blast radius, clearest win, kills a real clone.
2. **Extract `measure_per_axis_hug`** for Canvas + ZStack measure/intrinsic (S1).
3. **Add the `available_q` snapshot-vs-current assertion** in `MeasureCache::write_subtree` (A3) — single line, locks an invariant.
4. **Parameterize Grid Phase-1 by axis** (S3) — chips at the grid duplication without a full rewrite.
5. **Decide on the padding/margin split** (A2) — even just a doc comment claiming the current shape as intentional. Pick the answer once so the next driver author doesn't re-derive it.
