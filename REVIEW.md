# Palantir review — verified backlog

Supersedes `BACKEND_REVIEW.md`, `FRAME_PIPELINE_REVIEW.md`, and
`SIMPLIFICATION_REVIEW.md`, all audited 2026-07-25 at commit `86162a5a`.
Also absorbs `codex-review.md`, an independent frame-path review from
2026-07-26, whose material is folded into B1–B4 and E1 (it corroborated B3
independently, and was sharper than this document on the layout-skip gate, the
cascade preflight mechanism, and the moved-subtree slot arena).

Merged and re-verified against the working tree on 2026-07-26: every item
below was checked against current code, and everything the source documents
carried that has since shipped, been withdrawn on measurement, or isn't worth
doing is recorded at the bottom instead of taking up space at the top.

Ordered by impact, not by source document. Correctness first, then work with a
measurement behind it, then structural work whose payoff is real but large and
partly gated, then maintainability. Performance claims state whether they are
**measured**, **derived** from a measurement, or **unproven**.

| # | Item | Kind | Size | Status |
| --- | --- | --- | --- | --- |
| A1 | `run_on_main` silently drops owned work | Correctness | XS | **Shipped** |
| A2 | Modal misses Escape under popup keyboard capture | Correctness | S | **Shipped** |
| A3 | Popup capture resolves after popup bodies read events | Correctness | S | **Shipped** (D2) — one frame of lag remains by design |
| A4 | `DragValue` loses node configuration in edit mode | Correctness | S | **Shipped** |
| B1 | Whole-scene equivalence gate before layout | Perf (derived) | M | Verified open |
| B2 | Cascade preflight: O(N) verify, then unbounded re-work | Perf (derived) | M | Verified open |
| B3 | `restore_after_cache_hit` is now the layout hot path | Perf (derived) | M | **New** — not in any source doc |
| B4 | Damage's moved-subtree (scroll) leg | Perf (unproven) | M | Verified open |
| C1 | Record replay is a second lifecycle protocol | Structural | L | Verified open; gates C3 |
| C2 | Author-ordered render batches | Structural | L | Verified open |
| C3 | Fuse the cascade repair walk with the damage diff | Structural | L | Blocked on C1 |
| D1 | ~~Delegation macro~~ / composite translation | Maintainability | M | Macro **rejected** (tried); composite half open |
| D2 | Overlay keyboard authority / facade duplication | Mixed | M | **Closed** — keyboard half shipped; chrome/placement half found not to be duplication |
| D3 | Layout driver identity repeated across three dispatches | Maintainability | S | Verified open |
| D4 | Measure cache's manually synchronized shadow state | Maintainability | M | Narrowed by the arrange replay |
| D5 | Small wins bundle (3 items) | Mixed | S | **Shipped** |
| E1–E4 | Benchmark gaps | Enabler | S | Three of seven closed |

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

### A2. Modal keyboard ownership disagrees with layer ownership — **shipped**

**Keyboard capture is now layer-ordered rather than exclusive.** An uncaptured
read is blocked only by a capture at or above the reader's own layer, so a
`Modal` (z 2) still sees Escape while a `Popup` (z 1) holds capture, and every
layer at or below the owner stays silenced exactly as before. Layer
discriminants are already const-asserted to match `PAINT_ORDER`, so the
ordering is an `idx()` comparison. `Modal` now reads Escape *inside* its own
layer scope and after its body records, so an overlay the body opens on the
modal layer still wins Escape ahead of it.

**"Make `Modal` capture the keyboard" was rejected because capture was, at that
point, exclusive** — `text_edit/input.rs` drains the *uncaptured* stream, so
capturing would have silenced every `TextEdit` inside a modal. Layer-ordering
the read policy fixed dismissal without that cost. (With layer ordering now in
place the objection would no longer hold, since a capture registered on a lower
layer does not silence a reader above it — but the shipped design is the
simpler of the two.)

**It also fixed a second, unreported bug.** `Popup::show` holds capture across
its whole body, so before this change a `TextEdit` *inside a popup* received no
typed text at all — verified by reverting the predicate, which empties the
buffer. Nothing in the tree exercised the combination. Now pinned by
`popup::tests::text_edit_inside_a_popup_receives_typing`, which also records
the load-bearing detail: `Popup::show` calls `with_keyboard_capture` *outside*
`ui.layer(Layer::Popup, ..)`, so the capture registers at `Layer::Main` and the
body one layer up is not silenced by it. Moving that call inside the layer
scope would break typing again, silently.

Pinned at both levels: the policy in `input/tests/keyboard.rs` (a `Popup`
capture is visible from `Modal` and `Tooltip`, invisible from `Popup` and
`Main`), and end-to-end in `widgets/modal.rs` — Escape dismisses a modal with a
capturing popup open, **plus a control with no popup**, because a modal alone
has always dismissed and asserting only the popup case would not distinguish
"layer ordering works" from "Escape works".

### A3. Popup capture becomes authoritative after popup bodies read events — **partly fixed**

A2's layer ordering removes the **cross-layer** half: an overlay can no longer
be silenced by capture on a layer below it, whenever ownership is resolved. The
**same-layer** half stands. `capture_keyboard` still sets ownership eagerly when
none is held and `finish_record` still resolves last-wins afterwards, so on a
frame where the top popup appears, disappears, or reorders, a key can still go
to the previous frame's owner.

**Recorded so it is not re-attempted: resolving ownership live is wrong.**
Recomputing "topmost candidate so far" on each `capture_keyboard` looks like the
fix and is worse — with popups B then A recording in that order, B reads while it
is topmost-so-far and A reads after displacing it, so *both* receive the key.
The current stable-during-frame / resolve-at-end model is right; the defect is
only that the starting value can be stale.

That leaves the real fix as "resolve ownership before dispatching overlay key
actions", which needs an explicit overlay z-order rather than record order —
i.e. D2. There is also a residual double-delivery today: a `Modal` reading
Escape and a `Popup` holding capture both see it, so both close. Strictly better
than the modal being stuck, and it closes with the same D2 work.

### A4. `DragValue` loses node configuration in edit mode — **shipped**

One `inherit_chip_node` helper now carries the caller's node policy across the
chip → editor swap, with an exhaustive `Node` destructure modelled on
`scroll_wrappers`: every field is either carried or given a named reason for
being dropped, so a field added later cannot vanish silently.

Measured before and after by recording the same configured `DragValue` twice —
once as a chip, once focused as an editor — and diffing the *recorded* layout:

| Field | Chip | Editor before | Editor after |
| --- | --- | --- | --- |
| `margin` | 3.0 | **0.0** | 3.0 |
| `align` | `Align(18)` | **`Align(0)`** | `Align(18)` |
| `position` | `(23, 11)` | **`(0, 0)`** | `(23, 11)` |

**`padding` is deliberately *not* carried**, which the investigation changed my
mind about. Box parity across the swap is the theme's job —
`DragValueTheme::from_chip` mirrors the chip's padding onto the editor for
exactly that reason — and the chip resolves its own padding from the theme
rather than from the node. Forwarding it would make the editor honour a value
the chip ignores (a *new* divergence, not a fix) and it perturbed the editor's
intrinsic height in testing. `clip` is likewise dropped: `TextEdit::new` pins
`ClipMode::Rect` so glyphs cannot spill, and a caller must not relax that.

The test compares chip against editor rather than enumerating fields, so
fields added later are covered without editing it. Padding and minimum height
are excluded with a stated reason — both modes resolve those from their own
theme and intrinsics (a text editor has a line-height floor a chip does not),
so comparing them would pin theme configuration rather than this fix.

---

## B. Performance with a measurement behind it

### B1. Whole-scene equivalence gate before layout

`Ui::post_record` always runs layout before computing `cascade_fingerprint`,
which is late purely by placement: the fingerprint takes only `(&Forest,
Display)` and already covers root identity, complete subtree authoring,
placement, surface size and scale. Computing it after rollups and reusing the
retained `Layout` + `Cascades` on a match skips layout, cascade **and** the
structural damage walk on an identical recorded frame — which subsumes B3's
whole cost on that frame, leaving B3 to matter only for localized changes.

**Two retained fingerprints are required, not one.** `last_derived_fp` tracks
the most recent record pass, including an earlier pass in the *same* frame;
`previous_final_fp` tracks the scene the damage snapshot belongs to. Pass B can
equal pass A while both differ from the last rendered frame, so a single marker
would incorrectly skip structural damage on exactly that frame.

**The hazard is not optional.** `TextSystem::end_frame` retains only reuse rows
whose hot bit was set *during measure*. Skipping the layout pass marks nothing
hot, so every reuse row is evicted and the next real layout pays a full
re-measure — turning a saved frame into a much more expensive one two frames
later. A layout skip must also skip `text.end_frame` (or mark the frame as "no
measure ran"). A prototype missing this reads as a regression for the wrong
reason.

Two consequences worth planning for: `FrameProcessing::SingleLayout` /
`DoubleLayout` would start lying once layout can be skipped and should become
record-oriented names, and `frame/cached_cpu` **cannot measure this** — see E1.

Be honest about firing rate: `InputPolicy::OnDelta` (the default) already
suppresses frames where input changed nothing, and a `request_relayout` pass B
usually *does* differ from pass A. The reliable wins are `InputPolicy::Always`
hosts, `request_repaint` frames whose animation is shape-keyed, close-request
frames, and settling passes that changed no authoring.

### B2. Cascade preflight verifies O(N), then can throw the work away

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

Fold the per-node row count into `cascade_static` during the **existing**
rollup walk. `OpenFrame::paint_rows` (`scene/tree/recording.rs:33`) already
maintains exactly this count during recording — its own doc says it "mirrors
the row stream `cascade::compute_paint_rect` emits" — so this needs no extra
traversal and no new retained arena. A changed row count then fails
`can_update` *before* incremental repair starts, the late length mismatch
becomes a `debug_assert`, and `run_tree` no longer needs a recoverable failure
result.

The rect scan could fold into a rolling hash written during layout — but
**sequence this after B3**, because the arrange replay now leaves whole
subtrees' rects unwritten, so there is no longer a single pass that touches
every rect to fold a hash into.

Needs E2 (no bench arm covers a row-count change).

### B3. `restore_after_cache_hit` is now the layout hot path — **new finding**

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

Concrete shapes for both. **Sparse scroll content:** store sorted
`(relative_node, Size)` rows for scroll nodes only; restoration rebases and
appends just those rows, and the widget-side lookup can binary-search because a
subtree holds few scroll containers. **Relative text spans:** store snapshot
spans relative to the cached subtree's text range rather than absolute. That is
sharper than it looks — on a root hit `dest_start` is 0, so the per-node loop
with its branch collapses to a `copy_from_slice`, which is precisely the hot
case.

Both are **derived, not measured** — split `restore_after_cache_hit` in the
bench (E4) before committing to either. That discipline is what made the
arrange replay provable and what killed the backend's bind-tracking item.

### B4. Damage's moved-subtree (scroll) leg

Tier 1.5 in `DamageEngine::compute` handles the scroll/pan case, and for
**every node** in a jumped subtree does a `prev_map` hash probe, a
`union_screens` fold, a `copy_from_slice`, and a `cascade_input` write. That is
every frame of every scroll gesture over a long list.

Inside the jump the structure is known identical to last frame, so N hash
probes could become N pointer bumps. Concretely: change the retained snapshot
from `WidgetId -> NodeSnapshot` to `WidgetId -> stable slot` plus
`slot -> NodeSnapshot + next_preorder`, with the slot arena a retained `Vec` and
a free list. A moved subtree then pays one hash lookup for its root and
sequential slot access for every descendant, while structural paths keep the map
for identity, additions, removals and reparenting. The
copy is a separate question and a bigger change (owner-local rows plus a
per-node screen origin).

**Unproven**, and the code notes it was already optimized once — a per-row hash
matcher that was ~18% of a scrolling frame. Needs E3 to say whether probe or
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

**The delegation macro was attempted and rejected.** Two reasons, both found by
building it:

- A single roster in `widgets/mod.rs` cannot work: `node` is private to each
  widget's module, so the parent cannot reach it. It would take 19 field
  visibility escalations to buy a cosmetic win.
- Falling back to per-widget invocation gives up the roster — the only real
  benefit — leaving "5 lines → 1 line, 19 times" in exchange for a textually
  scoped `macro_rules!` that must sit above the `mod` declarations to be
  visible. An explicit, greppable, compiler-checked impl is worth more than
  that.

The 19 identical impls stay. **The composite-translation half is still open and
is the part that ever mattered** — it is what produced the `Scroll` and
`DragValue` defects, both now fixed with per-site exhaustive destructures
(`scroll_wrappers`, `inherit_chip_node`). A third instance would justify
extracting the shared shape; two do not.

Original finding:

19 near-identical `impl Configure for` blocks in `src/widgets`, and — the part
that actually bites — composite widgets hand-translating a `Node` into other
widgets. **A4 is an instance of this, and C2-the-correctness-finding (`Scroll`)
was another before it was fixed.** The class produces silent field loss.

A crate-private delegation macro for the identical impls; explicit `Node`
transformation helpers for composites, with exhaustive destructuring inside and
named policies for fields intentionally consumed or rejected. Do not add
generic accessors for every field.

### D2. Overlay keyboard authority — **shipped**; chrome/placement examined and closed

#### Chrome and placement: one real duplicate, the rest isn't duplication

Examined after the keyboard half landed. Exactly **one** genuine duplicate
existed and is now gone: `Modal`'s `BLOCK` constant and `Popup`'s eater sense
were the same four pointer senses, written two different ways in two files,
each documented as "so nothing leaks to `Main`". Two independent copies of "all
four" drift the moment a fifth sense is added, so it is now
`Sense::ABSORB_POINTER`, defined where `Sense` lives.

The rest does not consolidate, and the evidence is worth recording so it is not
re-attempted:

- **Chrome resolution looks shared and isn't.** All three apply theme
  fallbacks then resolve chrome, but against different slots and different
  fields — `Popup` uses `panel_background` + `panel_clip` via the existing
  `resolve_container_chrome`; `Tooltip` uses `tooltip.panel` + `padding` +
  `max_size`; `Modal` uses `modal.card` + `padding` + `min_size`. Widening
  `resolve_container_chrome` to cover them turns a 4-line helper into a config
  object over three policies.
- **Placement is already two library calls.** `Popup` and `Tooltip` position
  through `overlay_layer(layer, OverlayPosition)`; `Modal` fills the surface
  through `layer(..)`. There is no third thing to factor.
- **The scrims are structurally different.** `Popup`'s eater is a transparent
  `Frame` occupying its own layer root; `Modal`'s backdrop *is* the layer root,
  is painted, and centres the card. A shared recorder would need to branch on
  both, which moves the branching rather than removing it.

With the keyboard policy now uniform, a general "overlay recorder" would be a
configuration struct whose fields are the differences. Not worth it.

#### Keyboard authority

Scoped to the keyboard half deliberately. That is where both correctness
residuals lived, and it has a forcing function; chrome / placement / backdrop
dedup does not, `Tooltip` has no input policy at all, and `ContextMenu` /
`ComboBox` already build *on* `Popup` — so "four duplicated facades" is really
Popup-vs-Modal for the input parts.

The root cause was sharper than duplication. One `layer` value was doing two
jobs and the more important one was unused:

- **Ordering** — `finish_record` committed `candidates.last()`, ignoring layer
  entirely, so a `Popup` recorded after a `Modal` took the keyboard from it.
  Now the topmost candidate wins, with record order breaking ties inside a
  layer.
- **Blocking** — a capture now silences only readers *strictly below* it. That
  is what lets an overlay's own body keep reading, which is why a `TextEdit`
  inside a popup can be typed into.

`Modal` claims capture instead of reading the raw stream, so exactly one
overlay consumes a given Escape.

**The API moved from a scoped closure to a claimed value.**
`with_keyboard_capture(owner, layer, body)` became
`Ui::claim_keyboard(owner) -> KeyboardCapture` reading the *ambient* layer, and
`KeyboardCapture::release(&self, ui)` acting immediately. Overlays claim from
inside their own layer scope, so the recorded layer and the recording site can
no longer disagree — the explicit argument made that a caller's responsibility,
which is a bug waiting to happen. `Ui::layer` / `overlay_layer` /
`placed_layer` now forward their body's value so a claim can escape the scope
that created it, which is what `Popup` needs: it enters `Layer::Popup` twice
(full-screen eater, positioned body) and reads dismissal after both.

Residual, by design: ownership resolves at frame end, so an overlay opening on
top gets the keyboard one frame late. Resolving live is *worse* — with two
same-layer popups recording in sequence, the first reads while topmost and the
second reads after displacing it, so both receive the key. Closing that needs a
pre-pass over overlay claims before any body records.

Original finding:

### D2 (original). Overlay behaviour duplicated across four facades

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
freebie. Do it with B3.

### D5. Small wins bundle — **shipped**

- **`encode_node` no longer copies the 56-byte `ChromeRow`.** The `.copied()`
  turned out to be unnecessary rather than load-bearing: `LayerCtx::tree` is a
  shared-reference field, so `ctx.tree.chrome(id)` borrows the *`Tree`*, not
  `ctx`, and never collided with the `&mut ctx` that `brush_source` needs. One
  cacheline saved per chromed node — most nodes — on every encode.
- **The two per-node release asserts are now `debug_assert`** —
  `quantize_available` (per node per frame) and `capture_tree`'s text-contiguity
  check (per node per rebuild frame). Both are internal invariants on hot paths,
  which is exactly what the crate's assert policy reserves `debug_assert!` for.
- **`classify_frame` → `take_frame_plan`.** It drains the wakes that fired, and
  a reader is entitled to assume a `classify_*` is pure. The doc now states that
  the drain is the point — a wake must drive exactly one frame — rather than a
  side effect.

---

## E. Benchmark gaps

Three of the original seven are closed. What remains gates B1, B2 and B4.

| | Gap | Metric | Control |
| --- | --- | --- | --- |
| E1 | Identical-record lifecycle: record + rollup + gate + damage, **without** forced frontend work | CPU per frame | `frame/cached_cpu` |
| E2 | Cascade with a paint-row **count** change | CPU in `CascadesEngine::run` | Paint-only change (incremental succeeds) |
| E3 | Scroll over a long list (moved subtree, no authoring change) | CPU in `DamageEngine::compute`; probes vs bytes copied | Static list |
| E4 | `restore_after_cache_hit` split by column | CPU per restored column | Forced miss, and B1's whole-scene skip |

`frame/cached_cpu` cannot serve as E1: it deliberately substitutes a `Full`
plan after `Damage::Skip` so every CPU arm measures the same pipeline, which
means it *always* includes whole-tree encode + compose. It is a valid
whole-frame number and the wrong instrument for a lifecycle skip.

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
