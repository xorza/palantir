# Test harness

Palantir drives ~500 CPU frame-tests of its own, but the machinery is
crate-private or half-exposed. Consumers — darkroom first — want the
same thing: synthetic input, a real record pass, assertions on
responses and on whatever the app derives from them.

This proposes one gated type, `palantir::internals::UiHarness`, as the
whole external surface.

## How palantir tests itself today

Three tiers. Only the middle one is what a consumer wants.

**Pure.** No recorder at all — layout math, `EditState` editing,
geometry. A large share of the ~118 `mod tests` files.

**CPU frame driving.** The dominant idiom: construct, feed
`InputEvent`s, run a frame recording a closure, read `response_for`.

| helper | calls | visibility |
|---|---|---|
| `ui.run_at(size, record)` | 483 | `pub` (gated) |
| `ui.on_input(event)` | 443 | `pub`, ungated |
| `Ui::for_test()` | 496 | `pub(crate)` |
| `ui.run_at_without_baseline` | 116 | `pub(crate)` |
| `ui.response_for(id)` | 81 | `pub`, ungated |
| `Ui::for_test_at_text(size)` | 61 | `pub(crate)` |
| `ui.click_at` / `press_at` / `release_left` | 93 | `pub` (gated) |
| `ui.run_at_value{,_without_baseline}` | 69 | `pub(crate)` |
| `ui.under_outer` | 30 | `pub(crate)` |
| `ui.encode_paint{,_for}` | 27 | `pub(crate)` |
| `main_child_rects` / `_ids` / `node_for_widget_id` | 48 | `pub(crate)` |
| `Ui::for_test_at(size)` | 1 | `pub` (gated) |

**External integration.** `tests/alloc` already drives CPU frames from
outside the crate with `Ui::default()` + `record_test_frame`.
`tests/visual` goes the other way — real wgpu through
`OffscreenHost::frame_offscreen`, golden PNG diff, no input.

## The five protocol rules

None of these appear in a signature. They are what an outside consumer
gets silently wrong.

1. **Warm the recorder.** `Ui::default()` is cold — the first frame
   double-records. `for_test*` seed `prev_stamp` to prevent it.
   `tests/alloc` survives only because it warms up explicitly.
2. **Prime before reading.** `response_for` resolves against *last*
   frame's cascade. Any input assertion needs at least one prior frame;
   a stable arranged rect needs two.
3. **Read the response inside the record.** Between frames you get the
   prior frame's input — the `frame_quiescent` snapshot is taken at
   record-pass start. All 81 test reads go through the `resp()` helper
   in `input/tests/drag.rs` for this reason.
4. **Read the first record pass.** A frame with pending action input
   records twice; `drain_per_frame_queues` clears the one-frame edges
   (`clicked`, `drag.started()`) between passes. `resp()` uses
   `get_or_insert_with` to capture pass one.
5. **Mono vs. real text.** `Ui::default()` / `for_test_at` use the mono
   fallback shaper; `for_test_text` / `for_test_at_text` use cosmic.
   Anything whose width follows its label measures wrong under mono.

## What is exposed now, and why it is the wrong cut

Ungated already, and enough to author input: `on_input`, `InputEvent`,
`Key`, `KeyPress`, `Modifiers`, `TextChunk`, `PointerButton`,
`ResponseState`, `Drag`, `response_for`, `pointer_pos`,
`escape_pressed`, `focused_id`, `focus_within`, `hover_within`,
`Display`, `FrameReport`.

Gated-`pub`: `Ui::default`, `record_test_frame`, `for_test_text`,
`for_test_at`, `run_at`, `move_to`, `press_at`, `release_left`,
`click_at`, `secondary_click_at`.

Two problems with that set:

- **The wrong constructor went out.** `for_test_at` has one caller
  inside palantir; `for_test` has 496 and stayed private. Neither
  text-capable constructor is out at all.
- **The mechanism went out, the protocol did not.** `run_at` returns
  `FrameReport`, so getting a value out of the closure means a
  `let mut x = …; run_at(|ui| x = …)` dance — that is `run_at_value`,
  still private. Nothing carries rules 3–4, so the first external test
  asserting on `clicked()` between frames reads a stale edge, and the
  failure looks like a consumer bug.

Missing at any visibility: keyboard/text driving (every `text_edit`
test hand-builds `KeyPress { key, mods, repeat, physical }`), a drag
helper (`DRAG_THRESHOLD` is private, so consumers encode it as a
comment), and a settle loop.

## Why a wrapper type, not more `pub fn` on `Ui`

1. `lib.rs` declares `palantir::internals` the door out of the crate
   and states everything in it is a re-exported type. Inherent `pub
   fn`s cannot go through that door — they leak straight onto `Ui`.
   A type restores the invariant.
2. `Ui` has ~45 genuine public methods. Adding `click_at` / `press_at`
   means a consumer's *production* autocomplete on `ui.` offers test
   drivers, permanently.
3. A wrapper holds what `Ui` should not — surface, scale, clock — so
   `surface` stops being repeated at every call, and it can **enforce**
   the protocol instead of documenting it.

It lives in `src/ui/internals.rs` as a façade over the existing
crate-private helpers, so none of the 483 internal `run_at` calls move.

## Proposed surface

```rust
// palantir::internals::UiHarness
pub struct UiHarness {
    ui: Ui,
    surface: UVec2,
    scale: f32,
    time: Duration,
    pressed_at: Option<Vec2>,
}

impl UiHarness {
    /// Mono-fallback text — fast, deterministic, wrong for
    /// width-follows-label assertions.
    pub fn new(surface: UVec2) -> Self;
    /// Real cosmic shaping. Use when anything under test sizes to text.
    pub fn with_text(surface: UVec2) -> Self;
    pub fn scale(self, dpr: f32) -> Self;

    // frames
    pub fn frame(&mut self, record: &mut impl FnMut(&mut Ui)) -> FrameReport;
    /// Returns the value from the **first** record pass — the pass that
    /// observes one-frame edges (clicked, drag.started).
    pub fn frame_value<R>(&mut self, record: &mut impl FnMut(&mut Ui) -> R) -> R;
    /// `n` discarded frames. Two is the standard prime: layout, then a
    /// stable rect.
    pub fn settle(&mut self, n: u32, record: &mut impl FnMut(&mut Ui));
    /// Advance the clock — animation / spring / caret-blink tests.
    pub fn advance(&mut self, dt: Duration);
    /// Explicit display + time, for callers driving their own schedule.
    pub fn frame_at(
        &mut self,
        display: Display,
        time: Duration,
        record: &mut impl FnMut(&mut Ui),
    ) -> FrameReport;

    // pointer
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
    /// crossed DRAG_THRESHOLD — the capture would not latch and the
    /// test would pass for the wrong reason.
    pub fn drag_to(&mut self, pos: Vec2);
    pub fn scroll_lines(&mut self, d: Vec2);
    pub fn scroll_pixels(&mut self, d: Vec2);
    pub fn pinch(&mut self, factor: f32);

    // keyboard
    pub fn key(&mut self, key: Key);
    pub fn key_mods(&mut self, key: Key, mods: Modifiers);
    pub fn type_text(&mut self, s: &str);
    pub fn set_modifiers(&mut self, mods: Modifiers);

    // reading
    /// Arranged rect. Safe between frames — geometry is stable, edges
    /// are not.
    pub fn rect(&self, id: WidgetId) -> Option<Rect>;
    /// Center of `id`'s arranged rect. Panics with the id if unmeasured.
    pub fn center_of(&self, id: WidgetId) -> Vec2;
    /// `id`'s response captured inside the first record pass — the only
    /// correct way to read one-frame edges.
    pub fn response_in(
        &mut self,
        id: WidgetId,
        record: &mut impl FnMut(&mut Ui),
    ) -> ResponseState;

    pub fn ui(&mut self) -> &mut Ui;
}
```

Design calls worth defending:

- `frame_value` returning the **first** pass is the load-bearing
  decision. It makes rule 4 the default rather than a footgun, and
  subsumes the `let mut x = …` boilerplate.
- `response_in` is `frame_value` specialised. Kept separate because it
  is the most common read, and naming it is how rule 3 gets taught.
- `drag_to` panics below `DRAG_THRESHOLD` instead of exposing the
  constant. The threshold stays private and the precondition becomes
  enforced rather than commented.
- `settle(2, …)` names the prime instead of `for _ in 0..2`.
- `new` vs `with_text` puts rule 5 at the constructor, so it is a
  decision the author makes rather than one they inherit.
- `pressed_at` is the only added state, and exists solely so `drag_to`
  can check the threshold.

Deliberately **not** exposed: `main_child_rects`, `main_child_ids`,
`node_for_widget_id`, `encode_paint`, `under_outer`, the
`*_without_baseline` variants. Those reach into the tree and encoder —
palantir's own concerns. Consumers address everything by `WidgetId`.
Paint assertions, if ever wanted, are a separate decision.

## Phasing

1. Add `UiHarness` plus `pub use crate::ui::internals::UiHarness;` in
   `lib.rs`'s `internals`. Nothing else changes.
2. Revert `for_test_at`, `run_at`, `move_to`, `press_at`,
   `release_left`, `click_at`, `secondary_click_at` to `pub(crate)`.
   The 500+ internal callers are unaffected and the external door
   becomes exactly one type.
3. Port darkroom's dock test onto it, then build darkroom's own
   `Editor::frame`-level harness on top.
4. *Optional:* migrate `tests/alloc` off `record_test_frame` onto
   `frame_at`, letting `record_test_frame` / `for_test_text` /
   `Default` return to `pub(crate)`. `UiHarness` is then the whole
   gated surface.

## What it unlocks downstream

Darkroom has **67 `response_for` call sites** — 67 input paths — and
one driven test. `Ui::default()` appears ~15 times, but only as a text
arena for `ui.intern` while building `Scene` projections; its
`TestEditor` bypasses the UI entirely, calling `apply_edit` /
`drain_intents` directly. So everything between pointer event and
intent is unpinned:

| area | sites | what a driven test pins |
|---|---|---|
| `dock/` | 14 | chip drag → drop zone → `MoveTab`; close beats activate; rename-label click routing |
| `node/` | 18 | play chip vs. title vs. badge routing; port drag → `SetInput`; dbl-click disconnect |
| `canvas/` gestures | 25 | pane scoping — breaker inert on foreign panes, pan anchor not stolen, in-flight wire drawn once |
| inspector, menus, popup | 6 | chip cycling `Closed→Open→Pinned`; RMB palette; outside-action close |
| `image_viewer` | 2 | pan / zoom / dbl-click reset |
| toolbar, preferences | 2 | command routing |

The pane-scoping row is the sharpest case. Those fixes are currently
pinned by unit tests on the *predicate* — `PanAnchor::apply` with a
hand-passed key, `BreakerUI::probe` with a hand-built state. None
proves the predicate is wired to the pane the pointer is over, which is
exactly the bug that shipped.

Sketch of the ported dock test:

```rust
fn drag_arms_on(doc: &Document, tab: TabRef) -> bool {
    let theme = Theme::default();
    let viewer_labels = HashMap::new();
    let cx = DockContext { doc, theme: &theme, viewer_labels: &viewer_labels };
    let mut dock = DockUi::default();
    let mut h = UiHarness::with_text(UVec2::new(600, 200));

    h.settle(2, &mut |ui| {
        dock.render(ui, cx, &mut Intents::default(), |_, _, _| {})
    });

    let chip = h.center_of(strip::tab_chip_wid(tab));
    h.press_at(chip);
    h.drag_to(chip + Vec2::new(40.0, 0.0));

    h.frame_value(&mut |ui| {
        dock.render(ui, cx, &mut Intents::default(), |_, _, _| {});
        dock.scan(ui, doc, &mut Vec::new());
        dock.tab_drag.is_some()
    })
}
```

Nine lines of protocol become four, `with_text` fixes the mono-metrics
inaccuracy, and the `DRAG_THRESHOLD` comment becomes an assertion.
</content>
</invoke>
