# Frame pipeline review — record → rollups → layout → cascade → damage → frontend

Reviewed 2026-07-25 at Aperture commit `86162a5a`.

Scope: everything a frame passes through from `Ui::frame` to the point it
reaches the GPU backend — `src/ui/`, `src/scene/` (tree, rollups, seen ids,
cascade, damage, record store), `src/layout/` (engine, measure cache, drivers),
and `src/renderer/frontend/` (encoder, composer). Production code only; the
backend was covered separately in `src/renderer/backend/REVIEW.md`.

Static audit. Nothing below was benchmarked for this review, and every
performance claim states the measurement that would settle it.

## Relationship to the existing reviews

`SIMPLIFICATION_REVIEW.md` covers this ground crate-wide. Two of its items have
shipped since (`210b4866` replaced the command buffer with the paint-sink
pipeline — its finding 7; `495ba7ba` grew the gradient atlas — its C5). The
rest of its structural findings in this scope are still open, and I am not
restating their reasoning. This document reports what is **new**, plus a status
table for the prior items at the end.

## Conclusion

The pipeline is in good shape and the invariants are unusually well documented.
The per-frame skip machinery — measure cache, cascade fingerprint, cascade
incremental repair, damage tier-1 subtree skip — is genuinely effective, and
most of what looks redundant on first read turns out to be load-bearing.

The real opportunities are concentrated in one place: **the cross-frame work
skip covers measure but stops at arrange, and the three passes that follow
(arrange, cascade repair, damage diff) each re-walk the tree with their own
version of "did this subtree change?"**. That is four traversals per frame
keyed on substantially the same fact.

| Priority | Change | Expected benefit | Risk | Code effect |
| --- | --- | --- | --- | --- |
| P1 | Extend the subtree cache from measure to arrange | Removes a full arrange walk over unchanged subtrees | Medium | **Reduction** — deletes the `hugs` restore contract |
| P1 | Compute one scene-identity value once; gate layout with it | Skips measure+arrange on unchanged frames | Low–medium | Reduction (two gates → one) |
| P2 | Fuse the cascade repair walk with the damage diff | One traversal, one row copy instead of two of each | High | Large reduction, blocked by record replay |
| P2 | Cascade reuse preflight: 2× O(N) verify, then unbounded fallback | CPU on every changed frame | Low | Neutral |
| P2 | Damage's moved-subtree (scroll) leg | Per-node hash probe + memcpy per gesture frame | Medium | Neutral |
| P3 | `scroll_content` dense column (confirms prior finding 6) | Memory + per-frame memset + per-hit memcpy | Low | Reduction |
| P3 | Small cleanups (5 items) | Clarity, a few per-node checks | Low | Reduction |

---

## P1 — The cross-frame cache covers measure but stops at arrange

This is the largest single asymmetry in the pipeline.

`MeasureCache::try_lookup` can short-circuit an **entire subtree**: same
`WidgetId`, same `subtree_hash`, same quantized `available` → `desired` is
blitted from last frame's snapshot and recursion is skipped
(`layout/engine.rs`, `LayoutEngine::measure`). By the subtree-hash induction the
skipped subtree is authoring-identical to last frame.

Arrange has no such path. `LayoutEngine::run` calls `self.arrange(tree, root,
…)` for every root unconditionally, and every driver's arrange walks **all**
children — including collapsed ones, which route through `zero_subtree` — so
every node's `rect` is recomputed and rewritten on every `FullRecord` frame,
including the subtrees measure just proved unchanged.

That has three costs, and the third is the interesting one:

1. The obvious one: full driver dispatch, `build_stack_plan`,
   `freeze_distribute`, `justify_offsets`, `cross_place` per child, over
   subtrees whose output cannot have changed.
2. `LayerLayout::resize_for` zero-fills the whole `rect` column first, so those
   nodes are written twice.
3. **It forces the measure cache to carry measure→arrange side state.**
   `restore_after_cache_hit` exists almost entirely to feed the arrange pass
   that follows a measure hit: it splats `grid.hugs` back into the live pool
   because "without that, arrange reads zeros and every cell collapses to
   (0, 0)". The `LayoutScratch` doc spells out the resulting contract — a new
   retained field needs "three coordinated edits… forgetting any one corrupts
   arrange silently". That whole category-(2) hazard exists only because
   arrange re-runs over cached subtrees.

Recommended direction:

- Store the subtree's arranged `rect` slice in the snapshot alongside `desired`
  (the `NodeArenas` double-buffer already has exactly the right shape).
- On a hit where the arranged **slot** also matches last frame's, blit the rect
  slice and skip the arrange recursion for that subtree.
- The far more common case is a slot that moved but did not resize — a sibling
  above grew, so everything below shifts by `dy`. That is a pure
  `for r in slice { r.min += delta }` over a contiguous `Vec<Rect>`: still
  O(subtree), but a vectorizable add instead of driver dispatch plus child
  iteration plus per-node sizing math.
- `available_q` already keys the offered size; the missing key is the slot
  origin. Either extend the key or compare the resolved slot at the hit site.

Payoff beyond the CPU: a subtree that blits its rects doesn't need `grid.hugs`
restored (arrange never reads them), so the restore branch and part of the
category-(2) contract can go. That is code *reduction* from a performance
change, which is rare enough to be worth pursuing on its own.

Verification:

- `caches` bench, all arms — it already has cached, forced-miss, resizing,
  localized, deep, broad, heavy-text, and grid-intrinsic arms. The localized
  and broad arms are the ones this targets.
- `frame/cached_cpu` and `frame/partial_cpu` for the end-to-end number.
- Every `layout/cache/integration_tests.rs` fixture, plus the per-driver
  `cache_hit_preserves_*_rects` cases, which already pin exactly the corruption
  mode a wrong blit would produce.

## P1 — Two near-identical scene-identity gates, computed a phase apart

Within one `Ui::post_record` the pipeline computes two different keys over the
same roots, a layout pass apart:

- `LayoutEngine::cache_snapshot_matches_forest` (`layout/engine.rs`) walks
  every layer's roots building `RootSnapshotKey { wid, subtree_hash,
  available_q }`, plus total node and root counts. Result: `cache_rebuild`.
- `cascade_fingerprint` (`scene/cascade/mod.rs`) walks every layer's roots
  hashing `wid`, `subtree_hash`, and `placement`, plus the exact surface.
  Result: skip the cascade or not.

They differ only in how they treat the surface — quantized `available` vs. the
exact rect — and in the placement fold. Neither reads anything the other
couldn't.

More useful than merging the two walks: **`cascade_fingerprint` takes only
`(&Forest, Display)`**. Every input is available immediately after
`forest.post_record()`, before `layout_engine.run`. It is computed after the
layout pass purely by placement in the function. Hoisting it gives a
frame-level gate on the layout pass with exactly the soundness argument the
cascade skip already makes and the codebase already accepts: identical root
subtree hashes plus an identical surface ⇒ the tree was rebuilt with identical
structure ⇒ measure and arrange are pure functions of those inputs ⇒ last
frame's retained `Ui::layout` is still correct, and its NodeId-indexed columns
still line up.

Be honest about how often it fires. `InputPolicy::OnDelta` (the default)
already suppresses frames where input changed nothing, and a `request_relayout`
pass B usually *does* differ from pass A — that's its purpose. The reliable
wins are `InputPolicy::Always` hosts, `request_repaint` frames whose animation
is shape-keyed (`PaintAnim`) rather than record-keyed, close-request frames,
and settling passes whose action changed no authoring. This is a cheap gate on
top of P1 above, not a substitute for it.

**One hazard, and it is not optional.** `TextSystem::end_frame` retains only
reuse rows whose `hot` bit was set during measure (`text/system.rs`). Skipping
the layout pass marks nothing hot, so every reuse row is evicted and the next
real layout pays a full re-measure — turning a saved frame into a much more
expensive one two frames later. A layout skip must also skip `text.end_frame`
(or mark the frame as "no measure ran"). Any prototype that misses this will
look like a regression for the wrong reason.

Verification: `frame/*_cpu` on all four arms; `caches`; the alloc suite (a
skipped pass must not change steady-state allocation); the full text-wrap
cross-driver suite for the `end_frame` interaction.

## P2 — The cascade repair walk and the damage diff are the same walk, twice

Back to back, each frame:

- `CascadesEngine::run_tree::<true>` walks the tree pre-order with its own
  `stack: Vec<Frame>`, skipping a subtree when
  `lc.subtree_hashes[i] == tree.rollups.subtree[i]`, and for dirty nodes builds
  paint rows into `self.paint_scratch`, then `copy_from_slice`s them into
  `lc.paint_arena.rows`.
- `DamageEngine::compute` walks the same tree pre-order with its own
  `parent_stack: Vec<ParentFrame>`, skipping a subtree when `subtree_hash`,
  `cascade_input`, and `parent_key` all match, and for dirty nodes reads
  `lc.paint_arena.rows` and `copy_from_slice`s them into `arena.snaps`.

Same traversal order, same skip fact, same rows. A dirty node's paint rows are
built once and copied **twice**; two parent/frame stacks are maintained over
the same ancestry; two sets of per-node column loads.

A fused walk would build each dirty node's rows once, diff them against the
snapshot in place, and keep one ancestor stack. That is the concrete,
incremental form of prior finding 3 — it does not require redesigning the
storage into a double-buffered scene snapshot first.

**It is blocked today, and the blocker is worth naming.** The cascade runs
inside `record_pass`, so a double-layout frame runs it twice; damage runs once
at the tail of `Ui::frame` because it needs `ids.removed` from
`finalize_frame`. Fusing would make damage run twice, and the first pass's diff
would corrupt the snapshot baseline. So this finding is gated on prior finding
1 (remove or constrain record replay) — which is the strongest argument I found
for doing that work, beyond the complexity reduction the prior review already
made.

Verification: `cascade` and `damage` benches on unchanged trees, localized
paint changes, structural reorders, reparenting, clipping, transforms, and
paint animation; the full visual suite. Do not attempt this before the replay
question is settled.

## P2 — The cascade reuse preflight verifies O(N), then can throw the work away

`CascadesEngine::run` → `can_update` runs before every incremental cascade and
performs, per layer:

- a full `Rect` slice comparison,
  `cascades.entries.layout_rect()[base..base + n] != layout.layers[layer].rect`
  — 16 bytes/node of comparison traffic;
- a full zip comparison of `subtree_ends` against `tree.records.subtree_end()`.

Both are O(N) over every node in every layer, on every frame where the
fingerprint missed — i.e. every frame where anything at all changed. They exist
for a good reason: the incremental path does not recompute `cascade_inputs` or
re-push `entries`, so it must first prove those retained columns are still
valid.

Two things worth changing:

1. **The rect scan can become a compare of one number.** The layout pass
   already writes every rect; folding a rolling hash there (or in the same pass
   that fills `LayerLayout::rect`) turns the preflight into a `u64` compare.
   The pass cost moves rather than disappears — but it disappears entirely
   under P1's arrange skip, which would leave whole subtrees unwritten.
2. **The fallback is unbounded re-work.** `run_tree::<true>` returns `false`
   when a node's paint-row *count* changed (`old_span.len != new_span.len`), at
   which point `run` calls `run_full` and redoes every layer from scratch —
   after the preflight scans and after however much of the incremental walk
   already ran. A frame that adds one shape to one node therefore pays
   preflight + partial incremental + full rebuild. Either detect the row-count
   change up front (the row count per node is derivable from
   `chrome.is_some() + shape_span.len + child count` without walking), or let
   the incremental walk widen into a rebuild in place rather than restarting.

Verification: `cascade` bench with a row-count-change arm — I don't believe one
exists, and it is the case this finding is about.

## P2 — Damage's moved-subtree leg pays a hash probe and a memcpy per node, per frame

Tier 1.5 in `DamageEngine::compute` handles the scroll/pan case: `subtree_hash`
matches but `cascade_input` changed. It jumps the subtree, and for **every node
`j` in it** does a `prev_map.get_mut(&widget_ids[j])` hashmap probe, a
`union_screens` fold over that node's snapshot rows, a `copy_from_slice` of its
current rows, and a `cascade_input` write — plus mini parent-stack maintenance.

This is the scroll hot path: every frame of a scroll gesture over a large list
pays that per node. The code notes this arm already replaced a per-row hash
matcher that was ~18% of a scrolling frame, so it has been optimized once; the
remaining cost is the probe and the copy.

Worth exploring, in order of increasing ambition:

- The probe is the likely dominant term. Inside the jump the structure is known
  identical to last frame (that's what `subtree_hash` matching means), so the
  snapshot slots for the subtree were created in the same relative order — a
  per-snapshot "next in pre-order" link, or a per-node side index refreshed
  only when structure changes, would replace N hash lookups with N pointer
  bumps.
- The copy exists because `Paint.screen` is absolute. Storing rows
  owner-locally plus a per-node screen origin would make a pure translation a
  one-field update per node instead of a row-slice copy, and would let the
  extent fold reuse a cached local union. Bigger change; only worth it if the
  copy — not the probe — is what shows up.

Verification: a `damage` bench arm that scrolls a large list (many nodes, one
moved subtree, no authoring change). Confirm whether probe or memcpy dominates
before choosing.

## P3 — `scroll_content` is a dense per-node column for sparse data (confirms prior finding 6)

Confirmed exactly as the prior review described, with the full cost chain:

- `LayerLayout::scroll_content` is `Vec<Size>` sized to **every** node, cleared
  and zero-filled per layer per frame (`layout/mod.rs`, `resize_for`).
- `NodeArenas::scroll_content` duplicates it for every node in the measure
  snapshot, extended on every `capture_tree`.
- Every measure-cache hit copies the subtree's slice back
  (`restore_after_cache_hit`).

Production writes: one (`layout/scroll/mod.rs:44`). Production reads: one
(`widgets/scroll/mod.rs:244`, called from one site at line 605). Everything
else in the codebase touching this column is a test.

A sparse `(NodeId, Size)` arena or a per-scroll-container ordinal removes a
per-node column from the live layout, the snapshot, and the cache-hit copy at
once. The prior review's gate stands: a replacement index that costs more than
the eight bytes it removes defeats the change.

## P3 — Small cleanups

1. **`rect`'s per-frame zero-fill is redundant.** `LayerLayout::resize_for`
   does `rect.clear(); rect.resize(n, Rect::ZERO)`, so all N rects are zeroed
   before arrange overwrites all N. Every arrange driver iterates
   `tree.children(node)` (not `active_children`) and routes collapsed children
   through `zero_subtree`, so full coverage is already guaranteed. Dropping the
   `clear()` leaves stale values that are all overwritten. Caveat: that makes
   an undocumented driver invariant load-bearing — if taken, document it on
   `LayerLayout` and keep a debug-only sentinel fill plus an assert.
   `scroll_content` and `text_spans` genuinely need their zero-fill (sparse
   writers), so this applies to `rect` alone.

2. **Release asserts on per-node paths.** The crate's assert policy reserves
   release `assert!` for public-API misuse outside hot paths and names per-node
   checks as exactly what must not pay:
   - `quantize_available` (`layout/cache/mod.rs:64`) — called from
     `LayoutEngine::measure` for every node, every frame.
   - `capture_tree`'s "a measured subtree's text runs must be contiguous"
     (`layout/cache/mod.rs:240`) — per node, on every cache-rebuild frame.
   The per-layer ones (`Tree::post_record`'s two, `run_full`'s two,
   `capture_tree`'s five length checks) are per-frame rather than per-node and
   are cheap enough to leave; the two above are the ones the policy names.

3. **`encode_node` copies the whole `ChromeRow`.** `ctx.tree.chrome(id).copied()`
   copies the row so `ctx.brush_source(bg.fill)` can take `&mut ctx` afterwards.
   Reading `bg.fill` (and the corners the rounded-clip branch needs) before the
   `&mut` borrow lets the reference stand, saving a multi-cacheline copy per
   chromed node — which is most nodes in a real UI.

4. **Stale comment in `Ui::frame`.** The comment above the damage call says
   PaintOnly should "pass an empty set instead of stale state from the previous
   frame", but the code routes PaintOnly to `compute_paint_only`, which takes no
   `removed` argument at all. The behaviour is right; the comment describes a
   mechanism that no longer exists.

5. **`FrameRuntime::classify_frame` mutates.** It drains fired wakes out of
   `repaint_wakes` as a side effect of "classifying". Not worth restructuring,
   but the name should say so (`take_frame_plan` / `begin_frame`), since the
   function is the frame's single entry decision and a reader reasonably assumes
   a classifier is pure.

---

## Examined and deliberately not recommended

Recorded so this ground isn't re-walked.

- **Merging the three "did it change?" hash families** (`Tree::rollups`
  node/subtree/cascade_static, `CascadeInputHash`, `NodeSnapshot`'s four
  fields). They look redundant but partition cleanly by what they must *not*
  react to: `node_hash` deliberately excludes cascade state so a scroll doesn't
  invalidate `MeasureCache`; `cascade_input` deliberately excludes authoring so
  a paint change doesn't read as a move; `parent_key` covers the one thing
  neither captures (reparenting at an identical rect). Collapsing any pair
  re-couples caches that were deliberately decoupled.

- **`Cascades::by_id.clone_from(&forest.ids.curr)`** on every full rebuild. It
  is a real O(N) map copy, but `hashbrown`'s `clone_from` reuses the allocation
  and copies buckets without rehashing, and the alternative (reading
  `seen.curr` live) is unsound because `pre_record` clears it mid-frame while
  `response_for` still needs the previous pass's entries. The existing doc
  comment states the trade correctly.

- **The composer's ordering subsystem** (`HigherKindRects`, two `TextRectGrid`s,
  `quad_forces_flush`, `closed_hit`). Prior finding 2 already proposes the only
  change worth making here — author-ordered batches — and everything in the
  current implementation is a reasonable optimization *of* that design. Nothing
  local is worth changing while the design question is open.

- **`OcclusionPruner`.** The suffix-max reject, the lockstep cursor, and the
  in-place compaction are all sound, and the shadow exclusion is correct. No
  finding.

- **Per-layer iteration over five layers when only `Main` is populated.** Six
  passes per frame each iterate `Layer::PAINT_ORDER`; the empty-layer arms are
  length checks and empty-Vec clears. Real, and entirely noise.

- **`Tree::compute_rollups` re-hashing an identically-rebuilt tree every record
  pass.** This is the cost that makes every downstream skip possible; it can
  only be avoided by not re-recording, which is prior finding 1.

## Status of prior `SIMPLIFICATION_REVIEW.md` findings in this scope

| # | Finding | Status |
| --- | --- | --- |
| 1 | Record replay is a second lifecycle protocol | Open — and now also blocking P2's walk fusion |
| 2 | Fixed per-kind GPU replay creates a large ordering subsystem | Open, unchanged |
| 3 | Cascade + damage mirror the scene several times | Open — P2 above is the incremental first step |
| 4 | Measure cache is a manually synchronized shadow layout | Open — P1 above attacks the reason the shadow state exists |
| 5 | Grid track data has three forms | Open, unchanged |
| 6 | Scroll output is sparse data in a dense column | Open — confirmed above with the full cost chain |
| 7 | Command buffer is a transient duplicate schema | **Shipped** (`210b4866`) |
| 13 | Layout driver identity repeated across dispatch trees | Open, unchanged |

## Benchmark gaps

The bench suite is good — `caches` alone has eight arms, and `frame` cleanly
separates CPU from GPU. Three gaps block the findings above:

| Missing benchmark | Primary metric | Control |
| --- | --- | --- |
| Arrange over a cache-hit subtree (localized change, wide tree) | CPU time in `LayoutEngine::run`, split measure vs arrange | Forced full miss |
| Cascade with a paint-row count change | CPU time in `CascadesEngine::run` | Paint-only change (incremental succeeds) |
| Scroll over a large list (moved subtree, no authoring change) | CPU time in `DamageEngine::compute`, probes vs bytes copied | Static list |

Measure release builds. Traversal and probe counts explain a result; they don't
replace elapsed time.
