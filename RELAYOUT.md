# Same-frame re-record (relayout) investigation

Investigation of every case where Palantir runs more than one record pass
inside a single frame, what each costs, and how to remove them.

Measurements taken on ASUS ROG Strix SCAR 18 (i9-13980HX), release build,
`cargo bench --features internals --bench caches`. Everything attributed to
darkroom below is code-read, not measured — see §7.

## 1. Inventory: every same-frame re-record

All re-records funnel through the `double_layout` arm in `Ui::frame`
(`src/ui/mod.rs:254-268`). Four things reach it:

| # | Trigger | Where | Frequency |
|---|---|---|---|
| **A** | Cold-start warmup | `ui/mod.rs:233-249` | once per window, ever |
| **B** | `action_flag` from `InputState::finish_record` | `ui/mod.rs:250-253` + `input/mod.rs:675,742,780,845,858` | **every press, every release, every keystroke, every drag latch** |
| **C** | `Ui::request_relayout` — in-crate | `ui/mod.rs:556`; `widgets/scroll/mod.rs:737-740` | once per `Scroll` cold-mount |
| **D** | `Ui::request_relayout` — **downstream (darkroom)** | `gui/app/editor/mod.rs:341`, `gui/main_window.rs:154` | **every node-drag frame, every divider-drag frame, every tab switch** |

D is the one that matters and the one an earlier revision of this document
missed entirely — it claimed `Scroll` was the sole production caller. It is
the sole caller *inside this crate*; the crate's only real consumer calls it
far more often, on the two hottest gestures in the app.

The one thing worth stating up front: **pass A's layout output is almost
entirely thrown away.** The only durable products of an action pass A are
user-state mutations, `StateMap` writes, the input-queue drain, and
`self.cascades`. `forest` and `layout` are cleared/overwritten by pass B.

`ContextMenu` no longer needs a relayout, but the invariant the doc comment
at `ui/tests.rs:308-318` pins — cascade runs in `post_record`, so a pass-B
record reads pass A's arranged rects through `response_for` — is still the
mechanism every one of these triggers relies on. Anchor clamping itself now
happens inside arrange via `OverlayPosition::resolve(measured, bounds)`
(`layout/types/overlay/mod.rs:73`). That is the existing precedent for §5.

## 2. What a second pass actually costs (measured)

`record_test_frame_without_baseline` = record + `post_record` hashing +
measure + arrange + cascade + damage; **no encode/compose**:

```
caches/measure/cached          218 µs   measure 3.50 µs   arrange 1.05 µs
caches/measure/forced_miss     578 µs   measure 233 µs    arrange 99 µs
caches/broad/measure/cached     53 µs
caches/broad/measure/localized  82 µs
```

**On a cache-warm tree, measure+arrange is 4.5 µs of a 218 µs frame — 2%.**
The other 98% is the record closure, `Forest::open_node` / `Ui::widget` /
`WidgetId::auto_stable`, shape lowering, and `Tree::post_record` hashing.

Pass B is *already* mostly optimized away by existing machinery:

- `cache_snapshot_matches_forest` (`layout/engine.rs:588`) sees pass A's
  snapshot as `previous`, so `cache_rebuild = false` and measure collapses
  to a root blit.
- `cascade_fingerprint` (`ui/mod.rs:406-414`) skips pass B's cascade
  whenever B's tree matches A's.

**Conclusion that reorders the whole problem: making pass A "silent"
(skip post_record/measure/arrange/cascade) buys ~2%. There is no clever way
to make the second pass cheap — the second pass *is* the record closure.
The only lever is not running it.**

Two corollaries used throughout the rest of this document:

- **Re-arranging is nearly free; re-recording costs a whole frame.** Any
  design that converts a re-record into extra post-arrange work wins by
  ~50×, and it does not have to be clever to win.
- A downstream pass B costs *more* than 218 µs, because the host's `record`
  closure is not just widget authoring. Darkroom's re-runs `Editor::frame`
  end to end: navigate, `Scene::rebuild` (re-interns every port name into
  the text arena, re-flattens the pooled per-node slices),
  `CanvasGeometry::rebuild` (one `response_for` per port glyph), prepass,
  drain, then the record. All of it scales with graph size.

## 3. The downstream picture (darkroom)

`Editor` accumulates a per-frame `needs_relayout` from
`UndoStep::requires_relayout()` (`core/edit/intent/query.rs:34`) via
`StepSignals::fold` → `absorb_signals`, and fires one `ui.request_relayout()`
at the bottom of `Editor::frame` (`gui/app/editor/mod.rs:341`).

The load-bearing detail: **`absorb_signals` folds identically regardless of
which drain applied the step**, and three of darkroom's four drain points run
*before* the record.

| Drain | Runs | Is pass A already correct? |
|---|---|---|
| `navigate` (`editor/mod.rs:399`) — tab activate/close, undo/redo replay | pre-record | yes |
| `sync_target` (`editor/mod.rs:419`) — active graph changed | pre-record | yes |
| prepass (`editor/mod.rs:314`) — node drag, pan/zoom, connection commit, port dblclick | pre-record | mostly |
| post-record (`editor/mod.rs:336`) — rename commit, node menu, divider ratio | post-record | no |

### 3.1 Three over-triggers — fixed, §6-1 and §6-2

Kept as the diagnosis; all three are resolved.

**Node drag.** `GroupDrag::advance` (`gui/canvas/drag_anchor.rs:117`) pushes
an `Intent::MoveSelection` on *every frame of the gesture*, and a
`MoveSelection` naming any node returns `requires_relayout() == true`
(`query.rs:61`). It drains pre-record, so pass A already arranges at the
cursor — `gui/node/mod.rs:285` says exactly that ("no Pass B relayout
retry"). The comment describes the intent, not the behaviour: today **every
drag frame runs the whole editor pipeline twice.**

**Divider drag.** `render_node` (`gui/dock/mod.rs:220`) emits
`DockOp::SetRatio` whenever `live_ratio != ratio`, i.e. every frame of a
divider drag, and every `DocStep::Dock` returns `requires_relayout() == true`.
But `Splitter` already lays out at the live pointer-derived ratio and writes
back only the *arranged* one (`widgets/splitter/mod.rs:130-152`, pinned by
`divider_drag_maps_pointer_to_ratio_without_relayout`). The intent merely
persists what pass A already drew. **Second double-record on a per-frame
gesture.**

**Tab switch.** `sync_target` sets the flag directly. `CanvasGeometry` is
built to survive exactly this case: its `offsets` tier is kept across tab
switches so connections "draw on that first frame instead of popping in one
frame late" (`gui/canvas/geometry.rs:60`). The relayout looks like
belt-and-braces over a mechanism that already handles it.

`gui/main_window.rs:154` adds a one-shot `first_frame` relayout — meant as a
*third* record pass on frame 1, on top of the cold-start warmup in §4-A. It
was **dead code**: the warmup calls `record_pass`, which reaches
`MainWindow::frame`, so `take(&mut self.first_frame)` fires *inside the
blackout pass* — and `ui/mod.rs:247` then clears exactly that pass's relayout
request, by design. Pass A saw `first_frame == false` and never asked. Deleted;
nothing observable changes.

### 3.2 The residue

Strip those and one genuine case survives, documented at
`gui/canvas/mod.rs:142`: committing a connection removes an input's inline
const editor, so **the node resizes**. `CanvasGeometry`'s per-widget
`offsets` (`widget_rect.center − node_rect.min`) are cached across frames, so
after a resize the cached offset is stale and the new wire anchors to the old
port position. Pass B rebuilds geometry against pass A's cascade and fixes it.

Every other true arm of `requires_relayout` is the same shape: a rename
changes a label's width, a boundary-port add/remove resizes the boundary
node, `AddNode`/`RemoveNode` introduce nodes with no cached geometry at all.
The two arms that are *not* a size change are precisely the over-triggers
above — `MoveSelection`, and `DocStep::Dock` being one opaque
before/after-layout snapshot with no way to tell a ratio nudge from a
structural move.

## 4. Case by case (in-crate)

### C — `request_relayout` (Scroll cold-mount)

`Scroll` asks for a relayout because pass A has no prior arranged rect
(`widget_size.is_none()`), so `outer = Size::ZERO` and it cannot compute
thumb geometry. But what actually depends on geometry is narrower than it
looks:

- **Gutter reservation** is already content-independent — `bar_space`
  (`scroll/mod.rs:142-167`) reserves whenever `pan.y && reserve`,
  deliberately *not* toggled by overflow.
- Only `bar_plan`'s track/thumb rects and the "no thumb when content fits"
  cull need geometry, and both are pure functions of
  `(bar_viewport, scaled_content, offset)` — **all known after arrange**.

So: record the five bar nodes unconditionally and resolve their rects after
arrange. Thumbs stay `Sense::DRAG` leaves (the hit index is built from
arranged rects, so this works), their chrome paints their own arranged rect,
and "no thumb" becomes a zero-extent arrange. Cold-mount only, so not a perf
win — but it is the second consumer that justifies building §5 generically
rather than hand-rolling one resolver.

### A — cold-start warmup: leave it

One extra pass per window lifetime, zero steady-state cost. Not worth
touching. (Check whether it already subsumes darkroom's `first_frame`.)

### B — action settle

Independent of everything above, and still worth doing. Three findings, in
increasing order of leverage.

B1 and B2 below are **done** (§6-3); B3 and B4 are not.

#### B1. Presses almost never need the settle

`let observable = hit.is_some() || self.focused != prev_focus || buttons_subbed`
(`input/mod.rs:741-742`). But:

- The `Down` phase only feeds the press target's *own* theme picking —
  `slider.rs:76`, `scroll/mod.rs:308`, `theme/button.rs:128`,
  `theme/toggle.rs:55`. Nothing mutates state a prefix widget reads.
- **`focused` is read live**, not from the cascade
  (`input/mod.rs:1015,1032`), and `on_input` commits it *before* the frame.
  Every widget in pass A already sees the new focus. A press whose only
  effect is a focus change is already prefix-correct.
- Only `buttons_subbed` is genuinely opaque — and today the sole
  `PointerWake::BUTTONS` subscriber in-tree is `Modal` (`modal.rs:255`).

So presses could drop to `frame_had_action |= buttons_subbed`.

**Caveat to weigh:** an app that reacts to `TextEdit`'s `lost_focus` by
writing state a prefix widget shows would gain a one-frame lag.

#### B2. `ReleaseKind::Miss` doesn't need it

`input/mod.rs:779-780` sets the flag on `released.is_some()`. A `Miss`
release fires no click, moves no focus, and only tears down a capture read by
exactly one widget — which is precisely the rule the module doc at
`input/mod.rs:341-352` states as *not* qualifying. `Click` and `DragStopped`
must keep it (a graph-node drop rewires things a prefix widget draws).

Together B1+B2 remove roughly half the settle passes in click-driven UI:
only the release-click and keystrokes settle.

#### B3. `App::update` is already a free settle pass

`update` runs once per `FullRecord` frame, *before* pass A
(`ui/mod.rs:218`), takes `&Ui`, and `response_for` is `&self` — so an app
can read `response_for(id).left.clicked()` there and mutate its own state
before anything is recorded. **An app that handles its edges in `update`
never needs a settle pass at all.** The cost is explicit `.id()` on anything
handled that way, instead of `#[track_caller]` auto-ids.

The automatic settle exists only to support inline
`if button.clicked() { … }` in `record`. The honest framing is: `update` is
the fast path, and the automatic settle is the compatibility path for
inline handling.

Note that darkroom already applies the *shape* of this advice — its prepass
emits every layout-affecting input-derived intent before the record
(`gui/canvas/mod.rs:130-147`). §3.1 is what happens when the signal that
says "I needed a settle" isn't retired alongside the restructuring.

#### B4. For framework-mediated staleness, be exact instead of conservative

Popup open flags, combo/menu state, scroll offsets, and text-edit state all
live in `StateMap`, which Palantir owns. Stamp each row with
`last_read_idx` / `last_write_idx` (a `u32` record counter bumped in
`open_node`), and at the end of pass A compute
`settle_needed = any row where read_idx < write_idx`. Two compares per
access, 8 bytes per row. That makes every widget Palantir ships exact.

It does *not* cover user-struct writes — those fall back to B3 or an
explicit `ui.request_settle()`.

## 5. The general fix: late-bound geometry (`Anchor`)

§4-C's recipe — "resolve it in arrange, like `OverlayPosition` does" — is
per-widget work inside `src/layout/`, repeated for every new case. The
downstream evidence says arrange was never the right place anyway. Look at
what §3.2 actually is: **paint that wants another widget's geometry, and
settles for a cached copy of the previous pass's.** Not a layout failure —
layout ran once, on a correct tree.

So make the reference late-bound instead of cached. Record an identity, not a
number:

```rust
/// A geometry reference resolved after arrange, against this frame's
/// arranged rects — not a value captured at record time.
enum Anchor {
    Widget(WidgetId),
    Parent,
    Surface,
}
```

Two surfaces, both emitted from the **recorder**, so adding an anchored thing
touches zero files under `src/layout/`:

1. **Anchored shape coordinates.** Curve endpoints, polyline points, and rect
   corners accept `{ anchor: Anchor, frac: Vec2, offset: Vec2 }` in place of
   a resolved `Vec2`.
2. **Anchored placement.** `OverlayPosition::anchor`
   (`layout/types/overlay/mod.rs:37`) is already a dedicated anchor-rect
   field, pre-resolved at record time; late-binding it to `Anchor` is a
   one-field change. Add `Sizing::fraction_of(Anchor, axis, …)` for the
   scroll thumb.

Resolution is one sweep over a sparse side table of `(target, AnchorSpec)`,
structurally identical to `PaintAnims` (`scene/tree/paint_anims/`), which
already does exactly this shape of sparse post-arrange patching — so frames
using no anchors pay nothing. The `WidgetId → Rect` map it needs is what the
cascade's hit index already builds.

Three properties that make it worth building:

- **It degrades to today.** A dangling id or a reference cycle resolves to
  last frame's rect, which is precisely current `response_for` semantics.
  Strictly better, no new failure mode. `debug_assert` on the cycle.
- **The budget is there.** Measure+arrange is 4.5 µs of a 218 µs pass. A
  sparse fixup sweep — or even a whole second arrange — is free next to one
  extra record.
- **It fixes a correctness wart, not just a counter.** Darkroom's wires lag
  one pass across any resize today. Anchored curves make them exact,
  `CanvasGeometry.live` shrinks to the hover/drag bits that genuinely *are*
  last-frame input state, the connection-commit pass B disappears, and
  `SetInput` stops needing a relayout at all.

### Open questions

- **Placement anchors feed layout, shape anchors don't.** A shape anchor only
  has to resolve before encode. An anchored *node* rect must resolve before
  cascade, and if that node has descendants their subtree needs re-arranging
  from the patched rect. Cheapest correct answer is probably a second arrange
  restricted to anchored subtrees; at 1.05 µs for the whole tree, measure it
  before optimising it.
- **Ordering.** Because references are by `WidgetId` and resolution happens
  after the full arrange, forward references work for shapes for free. Node
  placement referencing a node that is itself anchored needs either a
  topological order or one fixpoint iteration with the graceful-degradation
  fallback above.

### What this deliberately does not cover

Tree *structure* that depends on geometry — virtualised lists, "how many
items fit". That case stays uncovered and should: the answer there is a
zero-extent arrange (§4-C's thumb-visibility argument) or an accepted frame
of lag. Worth noting `request_relayout` is capped at one retry anyway
(`ui/mod.rs:264-267`), so it never solved a converging-feedback problem —
an oscillating widget is already broken today.

Once §5 lands and §4-C is converted, nothing calls `Ui::request_relayout`
and it can be **deleted from the API** — a footgun that grants any widget
the power to double a frame.

## 6. Recommended order

1. ~~**Split `requires_relayout` into resize-vs-move.**~~ **Done.** Now
   `UndoStep::invalidates_cached_geometry`, asking the one question that
   matters — does this strand `CanvasGeometry`'s cross-frame caches?
   `MoveSelection` and every `DocStep::Dock` arm are false; resizes and
   node-introducing steps stay true. No phase plumbing was needed in the
   end: once the arms are right, the pre-record drains simply fold `false`,
   so `absorb_signals` stays unconditional. Pinned by
   `invalidates_cached_geometry_splits_resizes_from_moves`, which also
   asserts every table entry is a real (non-no-op) step — the first draft
   silently pinned a degenerate `ActivateTab`.
2. ~~**Delete `main_window.rs:154`.**~~ **Done** — see §3.1, it was dead.
3. ~~**Narrow `frame_had_action`** (B1+B2).~~ **Done.** Presses now flip it
   only on `buttons_subbed`; releases only on `kind != Miss`. Both are
   strictly narrower than the `observable` return value feeding the
   `OnDelta` wake gate, which is unchanged — a press on inert surface still
   records, it just doesn't settle. Pinned by `input/tests/settle.rs`, whose
   two narrowing assertions were confirmed to fail (2 vs 1) against the
   pre-change code before landing; the drag arm there is a regression guard
   that passes either way.
4. **Build `Anchor`** (§5) and land it on darkroom's wires first — the case
   with a visible bug to prove it against, and the one that retires the last
   genuine downstream caller.
5. **Convert the scrollbars** to anchored placement, then delete
   `Ui::request_relayout`.
6. **Only if 3+4 leave it hot:** the `StateMap` read/write-index tracking in
   B4.

Explicitly **not** worth doing: making pass A "silent" by skipping its
`post_record` / layout / cascade. Measured at ~2% of a settle pass, and it
would cost the pass-A-geometry invariant that `ui/tests.rs:308-318` pins.

## 7. Known gaps

- **No measurement of a *diverging* pass B** (where B's tree differs from A's,
  so the measure cache genuinely misses). The `broad/localized` arm (82 µs vs
  53 µs cached) suggests localized divergence stays cheap, but this was not
  confirmed — there is no double-layout bench arm in `benches/`, and adding
  one means touching the crate.
- **Nothing in §3 is measured.** The frequencies and the "runs twice" claims
  are read off the call graph, and §6-1 shipped on that reading. Confirming
  it is one counter: `FrameProcessing::DoubleLayout` tallied over a real
  drag, before and after. The classification is already on `FrameReport`.
- **No measurement of darkroom's pass B in absolute terms.** §2's corollary
  argues it exceeds 218 µs and scales with graph size; that is reasoning, not
  a number. `profile-with-tracy` spans both crates and would settle it.
