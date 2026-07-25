# Aperture review — verified backlog

Supersedes `BACKEND_REVIEW.md`, `FRAME_PIPELINE_REVIEW.md`, and
`SIMPLIFICATION_REVIEW.md`, all audited 2026-07-25 at commit `86162a5a`.
Merged and re-verified against the working tree on 2026-07-26: every item
below was checked against current code, and everything the three documents
carried that has since shipped, been withdrawn on measurement, or isn't worth
doing is recorded at the bottom instead of taking up space at the top.

Ordered by impact, not by source document. Correctness first, then work with a
measurement behind it, then structural work whose payoff is real but large and
partly gated, then maintainability. Performance claims state whether they are
**measured**, **derived** from a measurement, or **unproven**.

| # | Item | Kind | Size | Status |
| --- | --- | --- | --- | --- |
| A1 | `run_on_main` silently drops owned work | Correctness | XS | **Shipped** |
| A2 | Modal misses Escape under popup keyboard capture | Correctness | S | Verified open |
| A3 | Popup capture resolves after popup bodies read events | Correctness | S | Verified open |
| A4 | `DragValue` loses node configuration in edit mode | Correctness | S | Verified open |
| B1 | Cascade preflight: O(N) verify, then unbounded re-work | Perf (derived) | M | Verified open |
| B2 | `restore_after_cache_hit` is now the layout hot path | Perf (derived) | M | **New** — not in any source doc |
| B3 | Damage's moved-subtree (scroll) leg | Perf (unproven) | M | Verified open |
| C1 | Record replay is a second lifecycle protocol | Structural | L | Verified open; gates C3 |
| C2 | Author-ordered render batches | Structural | L | Verified open |
| C3 | Fuse the cascade repair walk with the damage diff | Structural | L | Blocked on C1 |
| D1 | `Node` delegation and composite translation | Maintainability | M | Verified open — A4 is an instance |
| D2 | Overlay behaviour duplicated across four facades | Maintainability | M | Verified open — A2/A3 live here |
| D3 | Layout driver identity repeated across three dispatches | Maintainability | S | Verified open |
| D4 | Measure cache's manually synchronized shadow state | Maintainability | M | Narrowed by the arrange replay |
| D5 | Small wins bundle (3 items) | Mixed | S | Verified open |
| E1–E3 | Benchmark gaps | Enabler | S | Two of five closed |

---

## A. Correctness

The source documents put these in a section titled "fix before broad
refactors", and only `SIMPLIFICATION_REVIEW.md` carried them — neither of the
other two surfaced them at all, which is how they stayed unaddressed while
performance items got attention. They are user-visible defects.

### A1. `HostHandle::run_on_main` silently drops owned work — **shipped**

`run_on_main` now returns `Result<(), HostDisconnected>`, mapped from winit's
`EventLoopClosed`. `HostDisconnected` is a new zero-sized public error in
`host/winit/error.rs`, exported from `lib.rs`.

**Status, not the closure back.** Winit hands the event back inside its error,
so returning the task was available — but there is no `&mut T` left to run it
against once the loop is gone, and its captures drop either way, so the payload
would be dead weight in the public signature. The caller's actionable fact is
*that* the mutation was lost.

`request_repaint` and `quit` stay fire-and-forget, and the asymmetry is now
documented on all three: `run_on_main` is the only poke carrying owned work,
so it is the only one whose loss is unobservable otherwise. A dropped repaint
or quit against an exiting loop costs nothing.

The failure path is not unit-tested — it needs a live-then-closed event loop,
which is not constructible headlessly. The shipped test pins the error's
contract instead: zero-sized, `Error`-boxable for propagation off a background
thread, and a message that names the *consequence* ("the scheduled work was not
delivered") rather than just the cause, since "event loop exited" reads as
routine shutdown.

### A2. Modal keyboard ownership disagrees with layer ownership

`src/widgets/modal.rs:100` still reads `ui.escape_pressed()` — the *uncaptured*
keyboard stream. Any popup capture empties that stream (`src/input/mod.rs:428`),
so a modal that paints above every popup and swallows pointer input through its
backdrop can silently fail to dismiss on Escape.

Modal must either preempt lower-layer keyboard capture or join one
layer-ordered overlay keyboard policy. See D2 — the real fix probably lands
there.

### A3. Popup capture becomes authoritative after popup bodies read events

`capture_keyboard` appends candidates (`src/input/mod.rs:443`) but
`finish_record` picks the topmost owner only afterwards
(`src/input/mod.rs:460`) — after every popup and context-menu body has already
read captured keys. On any frame where the top popup appears, disappears, or
reorders, a key can be delivered to the previous popup or to nobody.

Resolve ownership before dispatching overlay key actions, or defer overlay
keyboard consumption until the candidate stack is final.

### A4. `DragValue` loses node configuration in edit mode

`show_editing` (`src/widgets/drag_value/mod.rs:396`) rebuilds a `TextEdit` from
a subset of the caller's node — verified: `size`, `min_size`, `max_size`.
Padding, margin, parent alignment, grid placement, canvas position, clipping
and visibility are dropped, so **entering edit mode can move, resize, or
re-clip the widget**.

Transfer the applicable node policy through one explicit helper with an
exhaustive destructure, and add a table-driven test over every `Configure`
field. Note this is an instance of D1, and its sibling defect (`Scroll`'s
non-exhaustive destructure) has already been fixed that way — the guard comment
now in `scroll_wrappers` is the model to copy.

---

## B. Performance with a measurement behind it

### B1. Cascade preflight verifies O(N), then can throw the work away

`CascadesEngine::can_update` (`src/scene/cascade/mod.rs:489`) runs before every
incremental cascade and does, per layer, a full `Rect` slice comparison
(16 B/node of traffic) plus a full `subtree_ends` zip — on every frame where
anything changed at all.

The sharper half is the fallback. `run_tree::<true>` bails when a node's
paint-row *count* changed, and `run` then calls `run_full` and redoes every
layer from scratch — after the preflight scans, and after however much of the
incremental walk already ran. **Adding one shape to one node pays preflight +
partial incremental + full rebuild.** That is an ordinary authoring edit, not a
pathological one.

Row count per node is derivable from `chrome.is_some() + shape_span.len +
child count` without walking, so the bail can move up front; alternatively let
the incremental walk widen into a rebuild in place rather than restarting.

The rect scan could fold into a rolling hash written during layout — but
**sequence this after B2**, because the arrange replay now leaves whole
subtrees' rects unwritten, so there is no longer a single pass that touches
every rect to fold a hash into.

Needs E1 (no bench arm covers a row-count change).

### B2. `restore_after_cache_hit` is now the layout hot path — **new finding**

Not in any source document, and it is a direct consequence of shipping the
arrange replay. Measured on `caches`, min µs over 64 frames:

| Arm | measure | arrange |
| --- | --- | --- |
| `measure/cached` | 3.16 | 1.17 |

On a root cache hit measure does no measuring, so that **3.16 µs is almost
entirely restore work**: a `desired` memcpy, a `scroll_content` memcpy, a
`text_shapes` extend, and a **per-node `text_spans` rebase loop over every node
in the tree**. Arrange, the item everyone was looking at, is now the cheaper
half.

Two consequences:

- **`scroll_content` stops being a tidy-up.** A dense `Vec<Size>` sized to
  every node, cleared and zero-filled per layer per frame, duplicated in the
  snapshot, and slice-copied on every cache hit — for data with exactly one
  production writer (`src/layout/scroll/mod.rs:44`) and one reader
  (`src/widgets/scroll/mod.rs:244`). Both source documents ranked this P3
  cosmetic; it is now one of three memcpys on the critical path. A sparse
  arena removes it from the live layout, the snapshot, and the hit path at once.
- **The `text_spans` rebase loop is probably the single largest piece** and
  appears in no review. It is a per-node loop with a branch and an add, over
  the whole tree, on every cached frame.

Both are **derived, not measured** — split `restore_after_cache_hit` in the
bench (E3) before committing to either. That discipline is what made the
arrange replay provable and what killed the backend's bind-tracking item.

### B3. Damage's moved-subtree (scroll) leg

Tier 1.5 in `DamageEngine::compute` handles the scroll/pan case, and for
**every node** in a jumped subtree does a `prev_map` hash probe, a
`union_screens` fold, a `copy_from_slice`, and a `cascade_input` write. That is
every frame of every scroll gesture over a long list.

Inside the jump the structure is known identical to last frame, so N hash
probes could become N pointer bumps via a per-snapshot pre-order link. The
copy is a separate question and a bigger change (owner-local rows plus a
per-node screen origin).

**Unproven**, and the code notes it was already optimized once — a per-row hash
matcher that was ~18% of a scrolling frame. Needs E2 to say whether probe or
memcpy dominates before choosing.

---

## C. Structural — large, high payoff, partly gated

These are protocol deletions, not cleanups. Each should start as a benchmarked
experiment, not a rewrite.

### C1. Record replay is a second lifecycle protocol

Same-frame record replay means a double-layout frame runs record, rollups,
cascade and layout twice. It is the reason `frame_had_action` exists with its
own reset semantics, and it is the direct blocker on C3.

Removing or sharply constraining it deletes a framework-wide protocol. The
strongest argument for doing it is no longer the complexity — it is that C3
cannot start until it lands.

### C2. Author-ordered render batches

Fixed per-kind GPU replay forces the composer to *recover* authoring order it
was handed, via `HigherKindRects`, two `TextRectGrid`s, `quad_forces_flush`,
and `closed_hit`. Preserving order through the renderer instead would delete
the composer's largest compensating subsystem.

Everything currently in that subsystem is a reasonable optimization *of* the
existing design, so nothing local there is worth touching while this question
is open.

### C3. Fuse the cascade repair walk with the damage diff — **blocked on C1**

Back to back each frame, two walks traverse the same tree in the same order,
skip on substantially the same fact, and copy the same rows: cascade builds
paint rows into scratch then copies into `paint_arena.rows`; damage reads
`paint_arena.rows` and copies into `arena.snaps`. A dirty node's rows are built
once and copied **twice**, with two ancestor stacks maintained over the same
ancestry.

Blocked because cascade runs inside `record_pass` (twice on a double-layout
frame) while damage runs once at the tail of `Ui::frame` — it needs
`ids.removed` from `finalize_frame`. Fusing would make damage run twice and the
first pass's diff would corrupt the snapshot baseline.

---

## D. Maintainability

### D1. `Node` delegation and composite translation

19 near-identical `impl Configure for` blocks in `src/widgets`, and — the part
that actually bites — composite widgets hand-translating a `Node` into other
widgets. **A4 is an instance of this, and C2-the-correctness-finding (`Scroll`)
was another before it was fixed.** The class produces silent field loss.

A crate-private delegation macro for the identical impls; explicit `Node`
transformation helpers for composites, with exhaustive destructuring inside and
named policies for fields intentionally consumed or rejected. Do not add
generic accessors for every field.

### D2. Overlay behaviour duplicated across four facades

`Popup` owns positioning, layer selection, chrome, outside-click policy,
keyboard capture and dismissal; `ContextMenu` exposes a hand-picked subset of
`Node` setters and then overwrites `Popup`'s crate-visible `node`; `Tooltip`
and `Modal` rebuild related layer/chrome/placement behaviour independently.

**A2 and A3 are both symptoms of this.** One crate-private overlay recorder —
configured by layer, placement, backdrop/input policy, keyboard policy and body
node — is where a coherent layer-ordered keyboard policy would live. Fixing A2
and A3 tactically first is fine; doing them *here* fixes them once.

### D3. Layout driver identity repeated across three dispatches

Verified: three exhaustive `LayoutMode` matches — `measure_dispatch` and
`arrange` in `src/layout/engine.rs`, plus `src/layout/intrinsic/mod.rs:206`.
Adding a driver needs three synchronized edits, and Scroll delegates
differently in each phase. Generate them from one roster, or colocate the three
operations per mode. Compile-time consolidation only — no dynamic dispatch on
the node hot path.

### D4. The measure cache's manually synchronized shadow state

The category-(2) contract on `LayoutScratch` — "three coordinated edits…
forgetting any one corrupts arrange silently" — still stands, but the arrange
replay **narrowed it**: skip and translate never read `grid.hugs`, so the
restore is dead work on the hot path and live only on the resize-bail path.
Making it lazy is the remaining piece, and it is a measured change, not a
freebie. Do it with B2.

### D5. Small wins bundle

- **`encode_node` copies a whole 56-byte `ChromeRow`**
  (`src/renderer/frontend/encoder/mod.rs:606`) purely so `ctx.brush_source` can
  take `&mut ctx` after. Reading `bg.fill` and `bg.corners` first lets the
  reference stand — a cacheline copy per chromed node, which is most nodes, on
  every encode.
- **Release `assert!` on genuinely per-node paths** —
  `quantize_available` (`src/layout/cache/mod.rs:69`) runs per node per frame;
  `capture_tree`'s text-contiguity check (`:257`) per node per rebuild frame.
  The crate's assert policy names exactly this case; `debug_assert!` is the
  conforming form.
- **`FrameRuntime::classify_frame` mutates** — it drains fired wakes as a side
  effect of "classifying". Not worth restructuring; the name should say so
  (`take_frame_plan` / `begin_frame`).

---

## E. Benchmark gaps

Three of the original five are closed. What remains gates B1 and B3.

| | Gap | Metric | Control |
| --- | --- | --- | --- |
| E1 | Cascade with a paint-row **count** change | CPU in `CascadesEngine::run` | Paint-only change (incremental succeeds) |
| E2 | Scroll over a long list (moved subtree, no authoring change) | CPU in `DamageEngine::compute`; probes vs bytes copied | Static list |
| E3 | `restore_after_cache_hit` split by column | CPU per restored column | — |

Measure release builds. Counts explain a result; they never replace elapsed
time.

---

## Shipped since the audits

Backend: debug markers feature-gated (`bfe5c493`); `ImageTextures` owns its
layout + sampler (`c37d8006`); image draws coalesce adjacent same-texture runs
into one instanced draw (`images/shared` 3.6–4.1 → **1.3 µs**, control flat);
`text/mod.rs` split 1032 → 401 production lines; `PartialScissors` collapsed to
a plain `ArrayVec`; `bind_clear`'s redundant `set_stencil_reference(0)` dropped,
making the schedule's dedup invariant true as written; hand-rolled span
arithmetic removed from all three draw arms; command-recording benchmark added
(`record_pass`).

Frame pipeline: **arrange replay** — a measure-cache hit now replays the
subtree's rects instead of re-running the drivers, verbatim or translated.
Arrange 89.18 → **1.17 µs** on `measure/cached`, 25.03 → **0.83 µs** on
`localized`, 30–150× across arms, with `forced_miss` and `resizing` flat.
Whole layout pass 92.4 → 4.36 µs. Plus the measure/arrange split instrumentation
that made it provable, `arrange_size` extracted to `support.rs`, and
`root_available` given one definition.

Crate-wide: paint-sink pipeline replaced the command buffer (`210b4866`);
gradient atlas grown (`495ba7ba`); `Scroll`'s drift guard made exhaustive.

## Dropped as noise

Recorded so the ground is not re-walked.

- **Backend bind-state tracking split, including text** — *withdrawn on
  measurement*, not skipped. `record_pass` gives ~11 ns per recorded step and a
  ~29 ns premium per text batch; the full fix recovers ~4 µs on a fixture
  engineered to produce 256 consecutive text batches, which real frames do not
  produce (batches span groups by design). The `PreClear → Quads` seam is
  ~0.2 µs. Whole main-pass recording is single-digit µs against a ~146 µs CPU
  frame.
- **Backend last-binding cache** — structurally impossible alongside run
  coalescing: once adjacent runs merge, no two consecutive lookups can share an
  id, so a one-entry cache has a guaranteed 0% hit rate.
- `Backbuffer::size`'s cached field — its justifying measurement predates
  wgpu 30 making `Texture::size()` an inline field read. Comment is wrong; the
  cost is not there.
- Two hand-built full-viewport quads; the debug dim quad missing
  `FillKind::SOLID.with_fast()`; `shader_template::specialize`'s 13 string
  copies at startup; release asserts in the backend; partitioning retained
  render targets by owner; uploading unused mesh payload ranges.
- **Physical module reorganization** (`SIMPLIFICATION_REVIEW` #14) — its own
  advice is to let ownership move first; a directory shuffle now is churn.
- Input/frame-scheduling state machine consolidation and text-edit orchestration
  plumbing — both real, both maintainability-only, both large. Revisit after C1.
- Grid track data's three forms and `FillKind`'s GPU-wire leak — same category.
- Merging the three "did it change?" hash families, `Cascades::by_id.clone_from`,
  the composer's `OcclusionPruner`, per-layer iteration over five layers, and
  `Tree::compute_rollups` re-hashing — all examined in the source documents and
  correctly rejected there.
