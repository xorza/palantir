# Test harness

Palantir drives ~500 CPU frame-tests of its own, but the machinery is
crate-private or half-exposed. Consumers — darkroom first — want the
same thing: synthetic input, a real record pass, assertions on
responses and on whatever the app derives from them.

This proposes one type, `UiHarness`, as **the** frame-driving test API
— for palantir's own ~500 driven tests and for consumers alike. Not a
consumer wrapper over an internal API that stays: a replacement. When
it lands, `impl Ui` has no test methods left and `src/ui/internals.rs`
holds the harness instead of a pile of reach-ins.

One type serves both audiences through two visibility rungs on the same
struct — `pub` for the protocol-enforcing surface that leaves the crate
via `internals`, `pub(crate)` for the tree/encoder/damage reach-ins that
must not. That is the whole trick, and it is why "consolidate" and
"expose something to darkroom" are the same piece of work rather than
two.

## How palantir tests itself today

Three tiers. Only the middle one is what a consumer wants.

**Pure.** No recorder at all — layout math, `EditState` editing,
geometry. A large share of the ~118 `mod tests` files.

**CPU frame driving.** The dominant idiom: construct, feed
`InputEvent`s, run a frame recording a closure, read `response_for`.

| helper | calls | visibility |
|---|---|---|
| `ui.run_at(size, record)` | 502 | `pub` (gated) |
| `Ui::for_test()` | 491 | `pub(crate)` |
| `ui.on_input(event)` | 482 | `pub`, ungated |
| `ui.run_at_without_baseline` | 119 | `pub(crate)` |
| `ui.click_at` / `press_at` / `release_left` | 103 | `pub` (gated) |
| `ui.response_for(id)` | 93 | `pub`, ungated |
| `ui.run_at_value{,_without_baseline}` | 75 | `pub(crate)` |
| `Ui::for_test_at_text(size)` | 62 | `pub(crate)` |
| `main_child_rects` / `_ids` / `node_for_widget_id` | 53 | `pub(crate)` |
| `ui.encode_paint{,_for}` | 38 | `pub(crate)` |
| `ui.under_outer` | 31 | `pub(crate)` |
| `Ui::for_test_text()` | 13 | `pub` (gated) |
| `Ui::for_test_at(size)` | 2 | `pub` (gated) |

(Counts are `rg -F` over `src/` + `tests/`, so each includes its own
definition. They drift; the shape is the point.)

**External integration.** `tests/alloc` already drives CPU frames from
outside the crate with `Ui::default()` + `record_test_frame` +
`Ui::for_test_text()` — those three, and nothing else. `tests/visual`
goes the other way — real wgpu through `OffscreenHost::frame_offscreen`,
golden PNG diff, no input. Neither touches `run_at`, `for_test_at`, or
any pointer helper: **every gated driving helper has zero external
users today.** They went `pub` for darkroom's one dock test.

## The one fact everything follows from

`Ui::frame` calls the record closure **one, two, or three times**, and
each call sees different input.

| pass | when it runs | what it sees |
|---|---|---|
| warmup | `prev_stamp.is_none()` — the very first frame | `InputState` swapped for an empty one: no pointer, no keys. Purely to build the cascade so pass A hit-tests against something. |
| A | always (except `PaintOnly`) | The real input, including one-frame edges — `clicked`, `drag.started()`. |
| B | `frame_had_action` \|\| `relayout_requested` | Post-`drain_per_frame_queues`: the edges are **gone**. Capped at one retry. |

`FrameProcessing` reports `SingleLayout` / `DoubleLayout` — it counts A
and B, not the warmup. `PaintOnly` is a fourth case that runs **zero**
record passes.

Every protocol rule below is a corollary of that table. It is also the
reason `run_at` cannot be the exported primitive: it hands back a
`FrameReport`, so getting a value out means a `let mut x = …;
run_at(|ui| x = …)` dance whose result depends on which pass wrote
last — and nothing in the signature says there is more than one.

## The protocol rules

None of these appear in a signature. They are what an outside consumer
gets silently wrong.

### Passes

1. **Warm the recorder.** `Ui::default()` is cold, so frame 1 runs the
   warmup pass. `for_test*` seed `prev_stamp` to skip it — the split
   the harness keeps as `UiHarness::cold` vs. every other constructor.
   `tests/alloc` survives only because it warms up explicitly. On a
   cold recorder "the first pass" means the input-blind one — every
   read in rules 3–4 resolves to the wrong pass.
2. **Prime before reading.** `response_for`'s `rect` / `layout_rect` /
   `hovered` / `disabled` come from *last* frame's cascade. Any input
   assertion needs at least one prior frame; a stable arranged rect
   needs two, and content whose size is only known after arrange
   (scroll thumbs, container text) can need more.
3. **Read the response inside the record.** Between frames you get the
   prior frame's input — the `frame_quiescent` snapshot is taken at
   record-pass start. All in-tree reads go through the `resp()` helper
   in `input/tests/drag.rs` for this reason.
4. **Read the first record pass.** Pass B runs after
   `drain_per_frame_queues`, which clears the one-frame edges.
   `resp()` uses `get_or_insert_with` to capture pass one.
5. **The record closure's *side effects* run once per pass too.** The
   write-direction peer of 3–4, and the easiest one to miss. A closure
   that pushes into a `Vec`, mutates retained widget state, or drains
   an intent queue does it twice on an action frame
   and twice on frame 1. Darkroom is currently safe by accident: pass
   B sees no `clicked()`, so no intent is re-emitted, and the warmup
   pass sees no input at all. A consumer harness that accumulates
   *across* frames must either read pass one only or make the closure
   idempotent.

### Clocks — there are two, and they diverge

6. **The frame clock only moves at a frame boundary.**
   `Ui::frame` copies its stamp into `input.frame_time`; events fed
   between frames are stamped with the value the *last* frame
   published. So advancing time is `advance(dt)` → `frame(…)` →
   *then* the input. Advance-then-input without a frame in between
   changes nothing.
7. **`run_at` pins time at `Duration::ZERO`, so every click is
   simultaneous.** `DOUBLE_CLICK_WINDOW` is 500 ms against
   `input.frame_time`. With the clock frozen, a second `click_at`
   within `DOUBLE_CLICK_RADIUS` (5 px) of the first *always* reports
   `double_clicked` and bumps `press_count`. Two deliberately separate
   clicks are only expressible by advancing the clock (rule 6) or by
   moving more than 5 px. The comment in
   `two_left_clicks_within_window_emit_double_clicked` — "tests run in
   real time but well under the window" — describes a wall clock the
   frame runtime does not use.
8. **Animation time is not the frame clock.** `advance_clock` clamps
   per-frame animation dt to `MAX_ANIM_DT = 0.1 s` and quantizes it
   through an accumulator at `ANIM_SUBSTEP_DT = 1/240 s`. One frame at
   `+500 ms` moves the double-click clock 500 ms and animations 100 ms;
   one frame at `+1 ms` moves animations by nothing at all. The in-tree
   idiom (`animation/tests.rs`) is an absolute `now` stepped 16 ms per
   frame, and that is what a consumer needs too.
9. **A frame can run no record pass.** `FrameProcessing::PaintOnly`
   fires when a paint-anim wake is the only reason for the frame — no
   input, no `repaint_requested`, valid damage baseline. A focused
   `TextEdit` is enough: its caret blink is a `PaintAnim::BlinkOpacity`
   re-queueing an `ANIM` wake every frame for 30 s after the last caret
   change. So "advance the clock and read a value out of the record"
   can legitimately have no value to return.

### Coordinates, text, routing

10. **Surface size is physical; pointer positions are logical.**
    `Display::from_physical(size, dpr)` derives logical size as
    `physical / dpr`, and `InputEvent::PointerMoved` is logical. At
    `dpr = 1.0` they coincide, which is exactly why a DPI test written
    against the current helpers looks right and isn't.
11. **Mono vs. real text.** `Ui::default()` / `for_test_at` use the
    mono fallback shaper; `for_test_text` / `for_test_at_text` use
    cosmic with bundled Inter + JetBrains Mono — the split the harness
    keeps as `new` vs. `with_text`. Anything whose width
    follows its label measures wrong under mono — darkroom's dock chips
    are the live example. But real shaping is *not* pixel-identical
    across machines: `FontSystem::new_with_fonts` also loads platform
    fonts as fallback. Assert relations (b is right of a, the press
    landed inside the chip), not exact widths.
12. **Scroll and pinch route to the widget under the pointer at event
    time.** `ScrollPixels` / `ScrollLines` / `Zoom` carry no position;
    `InputState` resolves them against `scroll_target` / `pinch_target`,
    set from the last `PointerMoved`. A scroll with no prior move goes
    nowhere. Signs follow winit: positive `y` means content scrolls
    down, and `ScrollLines` is the raw line count sign-flipped.
13. **Modifiers are sticky state, not per-event.**
    `InputEvent::ModifiersChanged` carries a full snapshot that
    persists until the next one. A helper that sets modifiers for one
    key must restore them, or every later key inherits them.
    `Modifiers.ctrl` is already platform-normalized — it is Cmd on
    macOS, Ctrl elsewhere.
14. **Typed text arrives as `KeyDown { key: Key::Char(c) }`, not
    `InputEvent::Text`.** The winit host only emits `Text` from
    `Ime::Commit`, and never calls `set_ime_allowed`, so that path is
    dead in production today. `TextEdit` consumes **both** — so a
    harness that emits `Text` pins a path no window produces, and a
    harness that emits both double-inserts.
15. **Keyboard events are discarded at ingress when nothing is
    focused.** `InputState::on_input` gates both `KeyDown` and `Text` on
    `focused.is_some() || subs.matches_press(kp) || keyboard_mask`, and
    a non-observable event is *dropped*, not queued and ignored. So a
    keyboard test has to establish focus first — by clicking, or via the
    already-ungated `Ui::request_focus` — and one that forgets sees an
    empty event queue rather than an unconsumed one. Found by writing
    the harness's own text test, which asserted on a queue that could
    never fill.

## What is exposed now, and why it is the wrong cut

Ungated already, and enough to author input: `on_input`, `InputEvent`,
`Key`, `KeyPress`, `Modifiers`, `TextChunk`, `PointerButton`,
`ResponseState`, `Drag`, `response_for`, `pointer_pos`,
`escape_pressed`, `focused_id`, `focus_within`, `hover_within`,
`request_focus`, `set_focus_policy`, `Display`, `FrameReport`.

Gated-`pub`: `Ui::default`, `record_test_frame`, `for_test_text`,
`for_test_at`, `run_at`, `move_to`, `press_at`, `release_left`,
`click_at`, `secondary_click_at`.

Three problems with that set:

- **The wrong constructor went out.** `for_test_at` has one caller
  inside palantir; `for_test` has ~490 and stayed private. Neither
  text-capable-*and*-sized constructor is out at all, so the one thing
  a consumer reliably needs — real text at a chosen surface — is the
  one thing missing.
- **Size is passed twice and can disagree.** `for_test_at(a)` seeds
  `prev_stamp` with display `a`; `run_at(b)` frames at `b`. Nothing
  checks. The mismatch reads as `display_changed`, silently forcing a
  full frame and resetting the damage baseline.
- **The mechanism went out, the protocol did not.** Nothing carries
  rules 3–5, so the first external test asserting on `clicked()`
  between frames reads a stale edge, and the failure looks like a
  consumer bug.

Missing at any visibility: keyboard/text driving (every `text_edit`
test hand-builds `KeyPress { key, mods, repeat, physical }`), a drag
helper (`DRAG_THRESHOLD` is private, so consumers encode it as a
comment), a priming loop, and any way to ask *what is actually at this
point* when a press misses.

Note that only the first problem is about *visibility*. The other two,
and everything in the "missing" list, are defects the ~500 in-crate
callers live with too — `SURFACE` threaded through every `run_at`,
`resp()` reimplemented wherever a test needs a one-frame edge, the
drag threshold as a comment. Fixing them for a consumer and fixing
them for palantir is the same work, which is the argument for doing it
once.

## Why one type with two rungs

1. `lib.rs` declares `palantir::internals` the door out of the crate
   and states everything in it is a re-exported type. Inherent `pub
   fn`s cannot go through that door — they leak straight onto `Ui`.
   A type restores the invariant.
2. A wrapper holds what `Ui` should not — surface, scale, clock,
   modifiers, press origin — so `surface` stops being repeated at
   every call and cannot disagree with the constructor, and so the
   protocol can be **enforced** instead of documented.
3. **Visibility is per-method on an inherent impl, so one type can be
   two APIs.** `main_child_rects` and `frame_without_baseline` sit on
   `UiHarness` as `pub(crate)`; `click_at` and `response_in` sit on it
   as `pub`. In-crate tests see both rungs and lose nothing; a consumer
   importing `palantir::internals::UiHarness` sees only the first. This
   is the only shape that makes consolidation and exposure the same
   change — anything else means an internal API plus a consumer facade
   over it, i.e. two things to keep in step.

An extension trait (`trait UiTestExt: sealed`) satisfies point 1 and
keeps production autocomplete clean too — trait methods only appear
where the trait is imported, which is arguably better than today's
inherent `pub fn`s. It fails point 2 (nowhere to put the clock or the
press origin, so every rule stays a comment) and it fails point 3: a
trait's methods are all as visible as the trait, so the reach-ins
would have to live on a *second*, crate-private trait. Two traits is
two APIs again.

It lives in `src/ui/internals.rs`, which is where the reach-ins already
are — so this is mostly a move, not a new layer.

### What the internal audience needs that a consumer must not get

Measured across `src/`, this is the whole list. Each is a `pub(crate)`
method on the harness; none is a design compromise, and none is
reachable from outside.

| need | sites | why a consumer can't have it |
|---|---|---|
| `ui` field: `forest` / `damage_engine` / `frame_runtime` / `input` / `cascades` / `resources` | 420 | Every one is a private engine's internals |
| `damage_region()` | 47 | Renderer-plan internals |
| `encode_paint{,_for}()` | 45 | Encoder output |
| `main_child_rects` / `_ids` / `node_for_widget_id` | 53 | Addresses the tree by `NodeId`, not `WidgetId` |
| `under_outer()` | 31 | Returns a `NodeId` |
| `anim_row_count::<T>()` | 9 | State-map internals |
| `frame{,_value}_without_baseline` | 119 | `damage_baseline_valid: false` — a damage-engine knob |
| `frame_at(display, time, …)` | benches | Explicit schedule; the `pub` rung has `advance` |
| `cold(surface)` | 1 block | `ui/tests.rs` deliberately tests cold start (rule 1) |
| `from_resources(res, surface)` | 2 | Two recorders sharing one `HostShared` |

The 420 field accesses are the interesting number. They are all on the
*outer* binding after a frame (`ui.damage_engine.dirty.len()`,
`ui.cascades.by_id[…]`), never on the closure's `&mut Ui`, so each one
would grow a `.ui()` if the field were private. Make it
`pub(crate) ui: Ui` instead: `h.ui.damage_engine.dirty.len()` costs two
characters over today and no accessor. The `pub fn ui(&mut self)`
escape hatch stays for consumers, who cannot see the field.

## Prerequisites inside palantir

Small, and each is a real coupling rather than a courtesy:

- `input::sense::DRAG_THRESHOLD` is `pub(super)`, invisible from
  `ui::internals`. Promote to `pub(crate)` so `drag_to` can enforce it.
  Same for `DOUBLE_CLICK_WINDOW` / `DOUBLE_CLICK_RADIUS` if the harness
  is to name the separation in `advance_past_double_click`.
- `emit_text_chunks` is a private free fn in `host/winit/input/mod.rs`,
  behind the `winit-host` feature. If the harness is to drive the IME
  path at all it must use the same splitter, not a second copy — hoist
  it to `input::keyboard` as `TextChunk::split(s)`.
- `Cascades::hit_test` and `Forest::collisions` are already
  `pub(crate)`; `hit_at` and `collisions` below are one line each.

## Proposed surface

Two impl blocks on one struct. The first is what leaves the crate.

```rust
// palantir::internals::UiHarness
pub struct UiHarness {
    /// `pub(crate)` so in-crate tests keep writing `h.ui.forest` rather
    /// than `h.ui().forest` at 420 sites. Invisible to consumers, who
    /// go through `ui()`.
    pub(crate) ui: Ui,
    surface: UVec2,      // physical px
    scale: f32,
    time: Duration,      // absolute; frames stamp with it
    mods: Modifiers,     // sticky, mirrors what the Ui was last told
    pressed_at: Option<Vec2>,
}

impl UiHarness {
    /// `UiResources::isolated_mono` — mono-fallback text: fast,
    /// deterministic, wrong for width-follows-label assertions.
    pub fn new(surface: UVec2) -> Self;
    /// `HostShared`'s cosmic resources. Use when anything under test
    /// sizes to text. Metrics are not guaranteed identical across
    /// machines — assert relations, not exact widths.
    pub fn with_text(surface: UVec2) -> Self;
    /// A harness that is never framed — its `ui()` is a string-interning
    /// arena for tests that build `InternedStr`-bearing projections
    /// without recording. Nominal surface, so nothing reads it. Exists
    /// because `InternedStr` is public and `Ui::intern` is the only
    /// public way to mint one; delete it if that changes.
    pub fn arena() -> Self;
    /// Device pixel ratio. `surface` stays physical; every `Vec2`
    /// position below is logical, i.e. `physical / dpr`.
    pub fn scale(self, dpr: f32) -> Self;
    /// The remaining two `Display` knobs, for the callers that care —
    /// wake coalescing reads the refresh rate, the composer reads the
    /// snap flag. Builders rather than a raw `frame_at(display, …)`:
    /// keeping `Display` construction inside the harness is what stops
    /// surface and scale from drifting apart across calls.
    pub fn refresh_millihertz(self, mhz: u32) -> Self;
    pub fn pixel_snap(self, on: bool) -> Self;
    /// Change the surface between frames — the resize path. Not a
    /// builder: `ui/tests.rs` alone frames at four distinct sizes, and
    /// darkroom's pane/dock splits want the same. Reads to the `Ui` as
    /// `display_changed`, exactly as a real resize does.
    pub fn resize(&mut self, surface: UVec2);

    // frames
    pub fn frame(&mut self, record: impl FnMut(&mut Ui)) -> FrameReport;
    /// Returns the value from the **input-observing** pass — pass A,
    /// the one that sees one-frame edges. Panics naming `PaintOnly` if
    /// the frame ran no record pass (rule 9).
    pub fn frame_value<R>(&mut self, record: impl FnMut(&mut Ui) -> R) -> R;
    /// `None` instead of the panic, for callers deliberately driving
    /// paint-anim frames.
    pub fn try_frame_value<R>(&mut self, record: impl FnMut(&mut Ui) -> R) -> Option<R>;
    /// `n` discarded frames. Two is the usual minimum: layout, then a
    /// settled rect. Named `prime`, not `settle` — palantir already
    /// uses "settle" for the *second record pass within one frame*
    /// (`input/tests/settle.rs`), and overloading it here would make
    /// the two unreadable together.
    pub fn prime(&mut self, n: u32, record: impl FnMut(&mut Ui));
    /// Frames until every arranged rect matches the previous frame's,
    /// up to `max`. Panics if it never converges. Replaces the magic
    /// `2` for content sized after arrange. Not for animated UIs —
    /// those never converge; use `prime`.
    pub fn prime_stable(&mut self, max: u32, record: impl FnMut(&mut Ui));

    // time
    /// Move the absolute clock. Takes effect on the **next** frame,
    /// and only then do subsequent input events carry it (rule 6).
    /// Animation dt is separately clamped to `MAX_ANIM_DT` (rule 8).
    pub fn advance(&mut self, dt: Duration);
    /// `n` frames stepping `dt` each — the correct way to move an
    /// animation, since one big jump is clamped. Panics if
    /// `dt > MAX_ANIM_DT`, which would silently under-integrate.
    pub fn advance_frames(&mut self, n: u32, dt: Duration, record: impl FnMut(&mut Ui));
    /// One frame past `DOUBLE_CLICK_WINDOW`, so the next click starts a
    /// fresh press run instead of doubling (rule 7).
    pub fn advance_past_double_click(&mut self, record: impl FnMut(&mut Ui));

    // pointer — positions logical
    pub fn move_to(&mut self, pos: Vec2);
    pub fn pointer_left(&mut self);
    pub fn press_at(&mut self, pos: Vec2);
    pub fn press_button_at(&mut self, b: PointerButton, pos: Vec2);
    pub fn release(&mut self);
    pub fn release_button(&mut self, b: PointerButton);
    pub fn click_at(&mut self, pos: Vec2);
    pub fn right_click_at(&mut self, pos: Vec2);
    pub fn double_click_at(&mut self, pos: Vec2);
    /// Move while pressed. Panics if travel since the press has not
    /// crossed `DRAG_THRESHOLD` — the capture would not latch and the
    /// test would pass for the wrong reason.
    pub fn drag_to(&mut self, pos: Vec2);
    /// Scroll / pinch carry no position, so these take one and emit the
    /// `PointerMoved` that sets the target first (rule 12).
    pub fn scroll_lines_at(&mut self, pos: Vec2, d: Vec2);
    pub fn scroll_pixels_at(&mut self, pos: Vec2, d: Vec2);
    pub fn pinch_at(&mut self, pos: Vec2, factor: f32);

    // keyboard
    pub fn key(&mut self, key: Key);
    /// Sets modifiers, emits the key, restores. `self.mods` is the
    /// only writer of `ModifiersChanged`, so nothing leaks (rule 13).
    pub fn key_mods(&mut self, key: Key, mods: Modifiers);
    pub fn set_modifiers(&mut self, mods: Modifiers);
    /// One `KeyDown { key: Key::Char(c) }` per char — the path a real
    /// window produces (rule 14).
    pub fn type_text(&mut self, s: &str);
    /// The IME path: `InputEvent::Text`, split by the same chunker the
    /// winit host uses. Separate because emitting both double-inserts.
    pub fn ime_commit(&mut self, s: &str);

    // reading
    /// Arranged rect, previous frame's cascade. Safe between frames —
    /// geometry is stable, edges are not.
    pub fn rect(&self, id: WidgetId) -> Option<Rect>;
    /// Center of `id`'s arranged rect. Panics with the id if unmeasured.
    pub fn center_of(&self, id: WidgetId) -> Vec2;
    /// `id`'s response captured inside pass A — the only correct way to
    /// read one-frame edges. `frame_value` specialised.
    pub fn response_in(&mut self, id: WidgetId, record: impl FnMut(&mut Ui)) -> ResponseState;
    /// Topmost widget the pointer would hit at `pos`. Turns "the click
    /// didn't land and I don't know why" into one assertion.
    pub fn hit_at(&self, pos: Vec2) -> Option<WidgetId>;
    /// Explicit-id collisions recorded last frame, as the colliding
    /// pairs. Today these only surface as a magenta overlay at runtime
    /// — invisible to a test, and exactly the failure mode of ids
    /// derived from domain data in a loop.
    pub fn collisions(&self) -> Vec<(WidgetId, WidgetId)>;
    pub fn assert_no_collisions(&self);
    /// Memory clipboard behind the `Ui`, for copy/cut/paste assertions.
    pub fn clipboard_text(&self) -> String;
    pub fn set_clipboard_text(&mut self, s: &str);

    /// Escape hatch for consumers. Reading `response_for` off this
    /// between frames breaks rule 3 — prefer `response_in`.
    pub fn ui(&mut self) -> &mut Ui;
}

/// The in-crate rung. Same type, never leaves the crate.
impl UiHarness {
    /// Cold recorder — `prev_stamp` unseeded, so frame 1 runs the
    /// warmup pass. Exists for the tests that pin cold start itself
    /// (rule 1); everything else wants the warm constructors above.
    pub(crate) fn cold(surface: UVec2) -> Self;
    /// Two recorders over one `HostShared`, for the shared-text-cache
    /// and idle/active-window tests.
    pub(crate) fn from_resources(res: UiResources, surface: UVec2) -> Self;

    /// `damage_baseline_valid: false` — forces a full frame. A damage
    /// knob, so it stays in-crate.
    pub(crate) fn frame_without_baseline(&mut self, record: impl FnMut(&mut Ui)) -> FrameReport;
    pub(crate) fn frame_value_without_baseline<R>(&mut self, record: impl FnMut(&mut Ui) -> R) -> R;
    /// Explicit display + time, bypassing `surface` / `scale` / the
    /// clock. The benches drive their own schedule.
    pub(crate) fn frame_at(
        &mut self,
        display: Display,
        time: Duration,
        record: impl FnMut(&mut Ui),
    ) -> FrameReport;

    // Tree / encoder / state reach-ins, moved verbatim off `impl Ui`.
    pub(crate) fn under_outer(&mut self, f: impl FnMut(&mut Ui) -> NodeId) -> NodeId;
    pub(crate) fn main_child_ids(&self, parent: NodeId) -> Vec<NodeId>;
    pub(crate) fn main_child_rects(&self, parent: NodeId) -> Vec<Rect>;
    pub(crate) fn node_for_widget_id(&self, id: WidgetId) -> NodeId;
    pub(crate) fn encode_paint(&self) -> RecordedPaint;
    pub(crate) fn encode_paint_for(&self, region: DamageRegion) -> RecordedPaint;
    pub(crate) fn damage_region(&self) -> DamageRegion;
    pub(crate) fn anim_row_count<T: Animatable>(&mut self) -> usize;
}
```

Design calls worth defending:

- `frame_value` returning pass A is the load-bearing decision. It makes
  rule 4 the default rather than a footgun, and subsumes the
  `let mut x = …` boilerplate. It is only unambiguous because the
  harness is always warm (rule 1) — the warmup pass would otherwise be
  "first".
- **`frame_value` must still run `record` on every pass.** Only the
  *value* is pass A's. The obvious implementation —
  `frame(|ui| { first.get_or_insert_with(|| record(ui)); })` — skips the
  scene entirely on pass B, so pass B records an empty tree and wipes
  the cascade the *next* frame hit-tests against. The symptom is remote
  from the cause: a later click silently misses. `resp()` in
  `input/tests/drag.rs` has this right — `build(ui)` sits outside the
  `get_or_insert_with`, and only the `response_for` sits inside. This is
  the subtlest line in the type and is worth a pinning test of its own
  (`frame_value must not skip pass B's recording`).
- Closures are taken **by value** (`impl FnMut`), not `&mut impl
  FnMut`. `&mut F` is itself `FnMut`, so a caller reusing one scene
  across `prime` + `frame_value` writes `&mut scene` once instead of
  `&mut |ui| …` at every call site.
- The harness owns the scene *closure* per call rather than storing
  one. A stored `Fn(&mut Ui)` would have to capture `&mut dock`, which
  freezes `dock` for the harness's lifetime and makes
  `assert!(dock.tab_drag.is_some())` uncompilable. Per-call is the
  ergonomic price Rust charges here, and it is cheaper than the
  alternative.
- `drag_to` panics below `DRAG_THRESHOLD` instead of exposing the
  constant. The threshold stays crate-private and the precondition
  becomes enforced rather than commented.
- `prime(2, …)` names the prime instead of `for _ in 0..2`;
  `prime_stable` exists because 2 is a guess that is wrong for
  after-arrange content.
- `new` vs `with_text` puts rule 11 at the constructor, so it is a
  decision the author makes rather than one they inherit.
- `hit_at` and `collisions` are the two *diagnostic* additions. Neither
  drives anything; both convert a silent wrong-answer into a named
  failure, which is most of what a harness is for.
- Every return type above is already in the supported export list.
  That is a constraint, not a coincidence: `CollisionRecord` and
  `Clipboard` are both `pub(crate)`, so returning them would either not
  compile or force three more `pub use`s through `internals`. Hence the
  pair-of-`WidgetId`s and the `String` — flattening at the boundary is
  cheaper than widening it.
- Added state is `time`, `mods`, and `pressed_at` — one per enforced
  rule (6/8, 13, and the drag threshold respectively).

The rung boundary is the `WidgetId` / `NodeId` line, and it falls out
of the type rather than being imposed: everything on the `pub` rung
addresses widgets by `WidgetId`, everything on the `pub(crate)` rung
addresses the tree by `NodeId` or reads an engine's internals. That
does mean **anything a consumer tests must carry an explicit
`.id(…)`** — `auto_stable` ids are not addressable from outside.
Darkroom already keys its widgets off domain ids, so this costs it
nothing.

Free on the `pub` rung and worth saying out loud: `FrameReport::paint()` and
`FrameReport::processing` are already public. A consumer can assert
"this interaction repainted partially" or "this frame did not
double-layout" today, through `frame`'s return value, with no new API.

## Open questions

- **`InputEvent::Text` is a dead production path.** No `set_ime_allowed`
  call exists, so nothing but a test ever emits it, while `TextEdit`
  maintains a full consumer for it. Either the host should enable IME
  or the path should be marked as forward-looking; the harness having
  to offer two text entries is a symptom, not the problem.
- **Window requests.** `record_test_frame` hardcodes `WindowToken(0)`,
  and a frame recording `Ui::open_window` on a bare `Ui` queues into
  `window_frame` where nothing drains it. `OffscreenHost` panics on
  exactly this. The harness should do one of the two — panic like the
  offscreen host, or expose the drained requests — not silently
  swallow. Multi-window driving is out of scope either way.
- **Focus seeding.** `Ui::request_focus` is already ungated, so
  keyboard tests can skip the click. Whether that is a convenience or a
  way to test a state the user cannot reach is a judgement call per
  test, not something the harness should decide.

## What gets deleted

The consolidation is only real if `impl Ui` ends up with no test
methods. The full kill list, all currently in `src/ui/internals.rs`
unless noted:

`Ui::for_test`, `for_test_at`, `for_test_text`, `for_test_at_text`,
`run_at`, `run_at_value`, `run_at_without_baseline`,
`run_at_value_without_baseline`, `record_test_frame`,
`record_test_frame_without_baseline`, `move_to`, `press_at`,
`release_left`, `click_at`, `secondary_click_at`, `under_outer`,
`main_child_ids`, `main_child_rects`, `node_for_widget_id`,
`encode_paint`, `encode_paint_for`, `damage_region`, `anim_row_count`
— plus every `Default` impl that only exists under a `test` /
`internals` gate. A `Default` that a production build never compiles is
a test constructor wearing a trait's clothes: it hides which specific
choice it makes, and on a `pub` type it is public under the feature no
matter how the rest of the surface is arranged. There turned out to be
exactly three in the crate — `Ui` (`ui/mod.rs`), `UiResources`
(`ui/resources.rs`), and `TextSystem` (inside `text/system.rs`'s gated
`internals` mod). **All three are gone.** `DamageEngine::default` looks
like a fourth but is ungated production code; only a field inside it
is gated.

### The three `Default`s

The `Ui` one is a deletion with **no replacement**, which is the part
worth being precise about.

`impl Default for Ui` is a trait impl on a `pub` type, so it cannot be
`pub(crate)`: under the `internals` feature `Ui::default()` is a public
constructor for a recorder nobody outside should build, whatever else
the kill list does. But it is only

```rust
fn default() -> Self { Self::new(UiResources::default()) }
```

and **`Ui::new(resources)` already exists and is already
`pub(crate)`**. So this needs no new constructor — adding a
`Ui::headless(resources)` beside it would be `Ui::new` under another
name. Delete the impl; the door closes by itself.

That leaves `UiResources::default()` as the one actually hiding
meaning. It reads as "the obvious resources" and is in fact a specific,
non-obvious choice — mono-fallback shaper, memory clipboard, no texture
cap, sharing nothing with any other recorder — which is exactly the
information a reader needs and `Default` deletes. Name it:

```rust
#[cfg(any(test, feature = "internals"))]
impl UiResources {
    /// Recorder capabilities that share nothing: a mono-fallback
    /// shaper (no font loading, deterministic metrics, wrong for
    /// width-follows-label), a memory clipboard, and no texture cap.
    /// The cosmic-shaping peer goes through `HostShared::new`, which
    /// is also what pairs two recorders onto one text cache.
    pub(crate) fn isolated_mono() -> Self;
}
```

Two call sites, and it turns `UiHarness::new` into a line that says
what it picked. The three constructor axes then live in one place —
`new` takes `isolated_mono`, `with_text` takes `HostShared`'s cosmic
resources, `from_resources` takes whatever the caller already has —
instead of being spread across `Default`, `for_test_text`, and a
hand-rolled `Ui::new(shared.resources.clone())`.

`TextSystem`'s is the same story one layer down and became
`TextSystem::mono()`: four call sites, all in `text/tests.rs`, all of
which read better naming the shaper they picked.

Nothing in production constructs any of the three; every impl was
already `#[cfg(any(test, feature = "internals"))]`.

**The `Ui` one is not free downstream.** Darkroom had 23 `Ui::default()`
sites, all `#[cfg(test)]`, none of which drive a frame: they want a
string-interning arena, because `Scene` projections hold `InternedStr`
and `Ui::intern` is the only public way to mint one. Deleting
`Default` breaks all 23. Hence `UiHarness::arena()` on the `pub` rung
— a harness with a nominal surface that is never framed, whose `ui()`
is the interner. The migration was mechanical (`Ui::default()` →
`UiHarness::arena()`, then `&ui` / `&mut ui` → `arena.ui()`, which
coerces), but it is 23 edits in another crate plus their imports, and
it is what forced the export to land before the deletion.

The deeper point that survives: `InternedStr` is public and there is
no public way to make one without a whole `Ui`. That is worth fixing
on its own merits, and would delete `arena()` again.

## Migration

~500 call sites across 55 files, big-bang, in the posture the crate
already takes toward breaking changes. Three shapes, in order of count:

**1. The dominant idiom (~500 sites, mechanical).** The surface stops
being an argument, so the constructor absorbs it:

```rust
let mut ui = Ui::for_test();              let mut h = UiHarness::new(SURFACE);
ui.run_at(SURFACE, |ui| { … });     →     h.frame(|ui| { … });
```

Not an `ssr` rewrite — the constructor and the call site are separate
statements, and `SURFACE` has to travel between them. Per-file with
`sd` after a rename of the binding, checking each file compiles before
moving on.

**2. Reach-ins (185 sites, pure rename).** `ui.main_child_rects(p)` →
`h.main_child_rects(p)`. The methods move verbatim; only the receiver
changes. This is the one `ssr` handles cleanly.

**3. Field access (420 sites, one character).** `ui.damage_engine.…` →
`h.ui.damage_engine.…`, because the field is `pub(crate)`. If it were
private this would be 420 `.ui()` calls and the migration would be
worth arguing about; it isn't, so it's `sd 'ui\.' 'h.ui.'` scoped per
file.

Ordering matters and is the standard phase discipline: rename bindings
first, then signatures, then call sites, then imports, then
`clippy --fix`, then `fmt`. Don't interleave.

Two things are *not* mechanical and should land first, alone:

- `ui/tests.rs`'s cold-start block (rule 1) — it must move to
  `UiHarness::cold`, and it is the one place where getting warmth
  wrong silently changes what is being tested rather than failing.
- The four distinct surfaces `ui/tests.rs` frames at, which need
  `resize` rather than a second constructor.

## Phasing

`Default` is what orders this. Darkroom consumes it today, so it
cannot go until darkroom has `UiHarness::arena()` to replace it —
which means the export step lands *before* the deletion, not after.

1. **Landed.** `DRAG_THRESHOLD` and `DOUBLE_CLICK_WINDOW` promoted to
   `pub(crate)`; `UiResources::isolated_mono`, `TextSystem::mono`, and
   `TextChunk::split` added; `UiHarness` lives in `src/ui/harness/` with
   both rungs, forwarding to the existing reach-ins, and 22 tests
   pinning the rules above.
2. **Landed, out of order.** Steps 3–6 below came first, because
   deleting the gated `Default`s was taken up before the bulk
   migration. `UiHarness` is exported, `tests/alloc` runs on it,
   darkroom's arena sites are migrated, and all three gated `Default`
   impls are gone. `impl Ui` still carries the rest of the kill list.
3. **Landed.** `pub use crate::ui::harness::UiHarness;` in `lib.rs`'s
   `internals`; the first `impl` block is now `pub`. Note the lint
   wrinkle: `palantir::internals` is `#[cfg(feature = "internals")]`, so
   a plain `cargo test` build does not compile the door the `pub` rung
   leaves by and `unreachable_pub` fires — hence the module-level allow,
   which is about the build shape rather than about the design.
4. **Landed.** `tests/alloc` runs on `UiHarness::new` / `with_text` and
   no longer touches `Ui::default`, `Ui::for_test_text`, or
   `record_test_frame`. Its fixed `DISPLAY` was already the
   constructor's defaults, so nothing was lost, and the 24 audits pass
   unchanged — the harness's warm start costs no steady-state allocation.
5. **Landed.** Darkroom's 23 arena sites are on `UiHarness::arena()`.
6. **Landed.** All three gated `Default` impls deleted. `Ui::new` was
   already the `pub(crate)` constructor, so `Ui` needed no replacement;
   `UiResources` and `TextSystem` got named ones.
7. **Remaining.** The bulk migration (step 2's ~500 sites) and then
   darkroom's dock test plus its own `Editor::frame`-level harness. Rule
   5 is the one to watch there: an editor-level harness accumulating
   intents across frames is the first thing in-tree that could
   double-count.

The ordering constraint that drove this: darkroom consumed
`Ui::default()`, so the export had to land *before* the deletion. That
is why 3–6 ran ahead of 2 — the bulk migration is independent of the
door-closing, and only the door-closing was on darkroom's critical
path.

## What it unlocks downstream

Darkroom has **61 `response_for` call sites** — 61 input paths — and
one driven test (`dock::tests::a_subgraph_chip_arms_a_tab_drag_…`,
whose helper holds the crate's only two `run_at` calls).
`Ui::default()` appears 24 times, every one of them
a text arena for `ui.intern` while building `Scene` projections — not
one drives a frame. Its `TestEditor` bypasses the UI entirely, calling
`apply_edit` / `drain_intents` directly. So everything between pointer
event and intent is unpinned:

| area | sites | what a driven test pins |
|---|---|---|
| `canvas/` gestures | 23 | pane scoping — breaker inert on foreign panes, pan anchor not stolen, in-flight wire drawn once |
| `node/` | 15 | play chip vs. title vs. badge routing; port drag → `SetInput`; dbl-click disconnect |
| `dock/` | 13 | chip drag → drop zone → `MoveTab`; close beats activate; rename-label click routing |
| `canvas/` menus + inspector | 6 | chip cycling `Closed→Open→Pinned`; RMB palette; outside-action close |
| `image_viewer` | 2 | pan / zoom / dbl-click reset |
| toolbar, preferences | 2 | command routing |

The pane-scoping row is the sharpest case. Those fixes are currently
pinned by unit tests on the *predicate* — `PanAnchor::apply` with a
hand-passed key, `BreakerUI::probe` with a hand-built state. None
proves the predicate is wired to the pane the pointer is over, which is
exactly the bug that shipped. `hit_at` is what makes that failure
legible when it recurs.

Sketch of the ported dock test:

```rust
fn drag_arms_on(doc: &Document, tab: TabRef) -> bool {
    let theme = Theme::default();
    let viewer_labels = HashMap::new();
    let cx = DockContext { doc, theme: &theme, viewer_labels: &viewer_labels };
    let mut dock = DockUi::default();
    let mut scene = |ui: &mut Ui| {
        dock.render(ui, cx, &mut Intents::default(), |_, _, _| {})
    };
    let mut h = UiHarness::with_text(UVec2::new(600, 200));

    h.prime(2, &mut scene);

    let chip = h.center_of(strip::tab_chip_wid(tab));
    // The regression was the press landing somewhere other than the
    // chip. `hit_at` is what turns that from `armed == false` into a
    // failure that names the culprit.
    assert!(
        h.hit_at(chip).is_some(),
        "nothing senses input at {chip:?}; collisions: {:?}",
        h.collisions(),
    );

    h.press_at(chip);
    h.drag_to(chip + Vec2::new(40.0, 0.0));

    h.frame_value(|ui| {
        dock.render(ui, cx, &mut Intents::default(), |_, _, _| {});
        dock.scan(ui, doc, &mut Vec::new());
        dock.tab_drag.is_some()
    })
}
```

Nine lines of protocol become four, `with_text` fixes the mono-metrics
inaccuracy, the `DRAG_THRESHOLD` comment becomes an assertion, and the
read moves from "whatever pass wrote last" to a named one.
</content>
</invoke>
