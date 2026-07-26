# Same-frame re-record (relayout) investigation

Investigation of every case where Palantir runs more than one record pass
inside a single frame, what each costs, and how to remove them.

Measurements taken on ASUS ROG Strix SCAR 18 (i9-13980HX), release build,
`cargo bench --features internals --bench caches`.

## 1. Inventory: every same-frame re-record

There are exactly **three** triggers, all in `Ui::frame` (`src/ui/mod.rs:207-276`):

| # | Trigger | Where | Frequency |
|---|---|---|---|
| **A** | Cold-start warmup | `ui/mod.rs:233-249` | once per window, ever |
| **B** | `action_flag` from `InputState::finish_record` | `ui/mod.rs:250-254` + `input/mod.rs:675,742,780,845,858` | **every press, every release, every keystroke, every drag latch** |
| **C** | `Ui::request_relayout` | `ui/mod.rs:554`; sole production caller `widgets/scroll/mod.rs:737-741` | once per `Scroll` cold-mount |

The one thing worth stating up front: **pass A's layout output is almost
entirely thrown away.** The only durable products of an action pass A are
user-state mutations, `StateMap` writes, the input-queue drain, and
`self.cascades`. `forest` and `layout` are cleared/overwritten by pass B.

`ContextMenu` no longer needs a relayout — the doc comment at
`ui/tests.rs:313-318` is stale. Anchor clamping now happens inside arrange
via `OverlayPosition::resolve(measured, bounds)`
(`layout/types/overlay/mod.rs:73`). That is the existing precedent for the
fix recommended below.

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

## 3. Case by case

### C — `request_relayout` (Scroll cold-mount): kill it outright

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

So: record the five bar nodes unconditionally, and resolve their rects in
arrange, exactly like `OverlayPosition` does for popups. Thumbs stay
`Sense::DRAG` leaves (the hit index is built from arranged rects, so this
works), their chrome paints their own arranged rect, and "no thumb" becomes
a zero-extent arrange. This removes the last caller and lets
`Ui::request_relayout` be **deleted from the API** — it is a footgun that
grants any widget the power to double a frame.

Not a perf win (cold-mount only), but an architecture win.

### A — cold-start warmup: leave it

One extra pass per window lifetime, zero steady-state cost. Not worth
touching.

### B — action settle: the only case that matters

Three findings, in increasing order of leverage.

#### B1. Presses almost never need the settle

`frame_had_action |= hit.is_some() || focused != prev_focus || buttons_subbed`
(`input/mod.rs:741-742`). But:

- The `Down` phase only feeds the press target's *own* theme picking —
  `slider.rs:76`, `scroll/mod.rs:308`, `theme/button.rs:128`,
  `theme/toggle.rs:55`. Nothing mutates state a prefix widget reads.
- **`focused` is read live**, not from the cascade
  (`input/mod.rs:1014-1022, 1032`), and `on_input` commits it *before* the
  frame. Every widget in pass A already sees the new focus. A press whose
  only effect is a focus change is already prefix-correct.
- Only `buttons_subbed` is genuinely opaque — and today the sole
  `PointerWake::BUTTONS` subscriber in-tree is `Modal` (`modal.rs:255`).

So presses could drop to `frame_had_action |= buttons_subbed`.

**Caveat to weigh:** an app that reacts to `TextEdit`'s `lost_focus` by
writing state a prefix widget shows would gain a one-frame lag.

#### B2. `ReleaseKind::Miss` doesn't need it

`input/mod.rs:779` sets the flag on `released.is_some()`. A `Miss` release
fires no click, moves no focus, and only tears down a capture read by
exactly one widget — which is precisely the rule the module doc at
`input/mod.rs:341-352` states as *not* qualifying. `Click` and
`DragStopped` must keep it (a graph-node drop rewires things a prefix
widget draws).

Together B1+B2 remove roughly half the settle passes in click-driven UI:
only the release-click and keystrokes settle.

#### B3. The structural answer for the rest: `App::update` is already a free settle pass

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

#### B4. For framework-mediated staleness, be exact instead of conservative

Popup open flags, combo/menu state, scroll offsets, and text-edit state all
live in `StateMap`, which Palantir owns. Stamp each row with
`last_read_idx` / `last_write_idx` (a `u32` record counter bumped in
`open_node`), and at the end of pass A compute
`settle_needed = any row where read_idx < write_idx`. Two compares per
access, 8 bytes per row. That makes every widget Palantir ships exact.

It does *not* cover user-struct writes — those fall back to B3 or an
explicit `ui.request_settle()`.

## 4. Recommended order

1. **Move scrollbar geometry into arrange** (the `OverlayPosition` pattern),
   then delete `Ui::request_relayout`. Removes trigger C and a public
   footgun.
2. **Narrow `frame_had_action`** — presses to `buttons_subbed` only,
   releases to `kind != Miss`. Biggest cheap win; pin each narrowing with a
   test, since these are exactly the "surprise behavior" cases.
3. **Document `App::update` as the zero-settle input path** and use it in
   Darkroom's hot subtrees. The only thing that removes the *remaining*
   passes rather than trimming them.
4. **Only if 2+3 leave it hot:** the `StateMap` read/write-index tracking in
   B4.

Explicitly **not** worth doing: making pass A "silent" by skipping its
`post_record` / layout / cascade. Measured at ~2% of a settle pass, and it
would cost the pass-A-geometry invariant that `ui/tests.rs:320` pins.

## 5. Known gap

No measurement of a *diverging* pass B (where B's tree differs from A's, so
the measure cache genuinely misses). The `broad/localized` arm (82 µs vs
53 µs cached) suggests localized divergence stays cheap, but this was not
confirmed — there is no double-layout bench arm in `benches/`, and adding
one means touching the crate.
