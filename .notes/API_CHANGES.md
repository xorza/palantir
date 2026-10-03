# Public API changes

This file holds every fix from `.notes/REVIEW.md`, `.notes/TEST_REVIEW.md` and `.notes/ISSUES.md`
that adds, renames, removes or re-signs an exported item. Under AGENTS.md, each one needs a
go-ahead before work starts. `.notes/REDESIGN.md` holds the rest, and names an interim step where
one exists.

When an item is done or rejected, delete it.

Each item gives the findings it closes, the options, a recommendation, and what it touches.
The recommendations are starting positions. Before you settle a signature, read its neighbours
(the sibling setters, the trait it joins, the `Response` its peers return) and match their names,
argument shapes and order, as AGENTS.md requires.

Line numbers are against `50b34a49`.

---

## A7. `golden` module surface

**Findings.** REVIEW "the public `golden` API takes and returns `image::RgbaImage` without
re-exporting `image`". TEST_REVIEW 7: `Tolerance` caps the share of differing pixels but not how
far a pixel may differ, and `render*` drops the `FrameReport`.

**Recommendation.**

- `pub use image` from `golden` (the same reason `lib.rs:225-238` re-exports `wgpu`).
- The `render*` functions return the `FrameReport` beside the image, so a suite can assert on
  repaint and paint mode without a second frame.
- The tolerance is A25 and the adapter sidecar and orphan report are A26; the three items land
  together (phase 5 of the plan).

## A10. Super modifier

**Findings.** REVIEW "Platform key events": `Modifiers` has no super bit, so Super+L arrives as
bare `l`. REDESIGN D2 clears the text in the host, which fixes the typing without this item.

**Recommendation.** Add a `meta` field to `Modifiers` and `ShortcutMods` alike, after A33 has made
the two types convert, so apps can bind Super chords. `super` is a keyword, so the field takes the
W3C `KeyboardEvent.metaKey` name; winit reads it from `ModifiersState::super_key()`. On macOS
Command already lands in `ctrl`, so `meta` is the Windows / Super key elsewhere. `Shortcut`'s
display gets the platform glyph. Low priority.

## A19. Features the docs imply: IME and focus traversal

**Findings.** REVIEW "Stale input docs": docs mention IME text, but winit `Ime` is never enabled
or translated, and there is no Tab focus traversal. REDESIGN D16 fixes the docs.

**Recommendation.** Treat each as a feature with its own design: IME needs `InputEvent` variants
for preedit and commit; focus traversal needs a focus order (the `KeyClass::Focus` class it reads
exists since the key-class split). Out of scope for the defect work.

## A20. Keyboard support on the toggle and range widgets

**Findings.** REVIEW "Small widgets design": ColorField, ColorStrip and Expander are focusable and
key-driven; Slider, Checkbox, RadioButton and Switch are not.

**Recommendation.** Make them focusable with Space/arrow handling. If this needs a public setter
(for example `focusable(bool)`), match Expander's. Pairs with A19's focus traversal.

## A22. Wheel sense per axis

**Findings.** REVIEW "Wheel routing" / REDESIGN D9: `hit_test_targets` sends the whole wheel delta
to the topmost `Sense::SCROLL` row. Every `TextEdit` senses `SCROLL` and a tab strip band is a
horizontal scroll, so a vertical wheel over either is swallowed and the page under it does not
scroll. Routing each axis to the nearest row that can pan along it (browser scroll chaining, CSS
Overscroll Behavior §2) needs each row to say which axes it pans *this frame* — a `Scroll` only
where its content overflows its viewport or its zoom is above 1, a single-line `TextEdit` only
along x and only when its text overflows. The cascade cannot infer that: the row that senses the
wheel is a `Scroll`'s outer frame, not the viewport node layout knows the extent of, and a
`TextEdit` pans inside one leaf. Widgets reach only the public API, so the declaration has to be
public.

**Options.**

1. `Sense::SCROLL_X` and `Sense::SCROLL_Y`, with `SCROLL = SCROLL_X | SCROLL_Y`. A widget senses the
   axes it can pan this frame; `hit_test_targets` keeps the topmost row per axis, and the wheel
   delta splits by axis (after the Shift swap) before delivery.
2. A separate `pan_axes(ScrollAxes)` configure setter beside `sense`. More explicit, but a second
   knob that has to agree with the `SCROLL` bit.

**Recommendation.** Option 1: the axes are what the sense means, and existing `Sense::SCROLL`
callers keep their behaviour. `Scroll` senses its declared axes intersected with the axes it can
pan (from last frame's `ScrollGeometry`), and `TextEdit` senses `SCROLL_X` while its text
overflows; the y→x wheel mapping then moves into routing — a horizontal-only row takes a pure-y
delta only when no row under the pointer pans y. Touches `Sense`, `Scroll`, `TextEdit`,
`TabStrip`, `Cascade::hit_test_targets` and `InputState::on_scroll`. Tests: wheel y over a field in
a `Scroll::vertical()` scrolls the page; an overflowing tab strip pans on wheel x and Shift+wheel y
(Linux) and passes wheel y to the page; a lone field with overflowing text pans on wheel y.

## A25. Golden tolerance that bounds how far a pixel may differ

**Findings.** TEST_REVIEW "The visual suite's tolerance and capture lose information":
`golden::Tolerance { per_channel, max_ratio }` caps the share of pixels past `per_channel`, but not
how far those pixels may move, so a loosened golden lets a few pixels be wholly wrong. Its
`Default` (2 per channel, 0.1 %) hid stale goldens in this repo's own suite: switching the suite to
exact comparison turned up five goldens 1–3 steps off on a large share of their pixels. The suite
now compares exactly through its own wrapper (`tests/visual/goldens.rs`); the public type and its
default are unchanged.

**Recommendation.** The WPT fuzzy shape: `Tolerance { max_delta, max_pixels }` — at most
`max_pixels` pixels may differ, and none by more than `max_delta` on any channel — with
`Tolerance::EXACT` as the `Default`. A loosening then names two numbers a reader can derive. Touches
`golden::Tolerance`, `DiffReport::passes`, `Goldens::tolerance` and any downstream suite.

## A26. Golden bookkeeping a suite cannot do from outside

**Findings.** TEST_REVIEW "The visual suite's tolerance and capture lose information": `Goldens`
does not know which adapter wrote a golden, so a driver update reads as pixel diffs across the
suite; and nothing reports a golden no test compares against any more.

**Recommendation.** `Goldens::adapter(info)` writes an adapter sidecar beside the goldens and
fails a comparison against a sidecar from another adapter with that reason instead of a pixel
diff; `Goldens::orphans(names)` lists golden files not among `names`, for a suite to assert empty
from one test that names them all. Touches `golden::Goldens`.

---

# Surface review (2026-10-04)

The items below come from a review of the whole exported surface, listed in `API_SURFACE.md`
(rustdoc JSON of `59b93e30`). Each item names what reads inconsistent, asymmetric, or
non-canonical, and what to do about it. Where a finding extends an earlier item, the earlier item
took it: A7, A9, A10, A11, A15, A18 and A19. The plan at the end of the file orders all
items, old and new.

## A31. Text arguments take one type

**Blocked** on `QUESTIONS.md` Q1.

**Findings.** Every widget label and caption takes `impl Into<TextInput<'a>>`, so a caller passes
`&str`, `String`, an interned string or `fmt!` output alike. (Window titles take
`impl Into<String>`, correctly: they outlive the frame.) `TextEdit::placeholder(&str)` and
`DragValue::suffix(&str)` take only `&str`.

**Recommendation.** Both take `impl Into<TextInput<'a>>`. `TabItem::new(key, label: InternedStr)`
stays as it is: a `TabItem` is `Copy` data in a slice, so it cannot own a borrow.

---

# Implementation plan

## Phase 0 — decisions before any code

All seven are decided. Each item named here carries its decision in its own text.

| Decision | Item | Recommendation | Decided (2026-10-04) |
|---|---|---|---|
| Release behaviour of a per-frame contract violation | A50 | the kind's neutral, with a debug panic | **a release panic**, with an `is_*` predicate per kind |
| A stale selection index | A50 | clamp to the last option | **clamp to the last option for display; the bound index is not written back** |
| The attach-to-trigger verb and argument order | A14, A51, A23 | one verb for `Tooltip` and `ContextMenu`, the snapshot first and the text second | **`on(&snapshot)`**, text second, no `ui` in a constructor |
| Page identity in `TabbedView` | A12 | an optional key function, index keys by default | **`.keyed(\|page\| impl Hash)`**, index keys by default |
| Fallible render target conversion | A16 | reject: no application chooses a target format at run time | **a checked `RenderTarget::new` / `TargetFormat::new`**; the `From` impls go |
| GPU tests in every `cargo test` | A17 | option 1 | **option 1**; the test server needs lavapipe installed |
| How the chrome setters are shared | A39 | a `Chrome` trait | **no trait**: inherent `default_background` on all eight, kept in step by one test |

## Phase 1 — foundations the later phases build on

1. Done: the `domain` module (A50 mechanism, A36).
2. Done: the `const` sweep (A24).
3. Done: flag sets (A32).
4. Done: no strum on public types (A6).
5. Done: the chainer, wrapper, text and validation rules are in AGENTS.md, and
   `scripts/api_surface.py` regenerates `API_SURFACE.md`.

## Phase 2 — renames and removals

Each line is one commit; none depends on another inside the phase.

1. Done: names (A28, A29, A30, A42, A45, A46).
2. Done: chainers (A47, A44).
3. Done: removals (A8, A13, A34, A35, A38, A40, A43, A48, A49).
4. Done: argument types (A33 with A9, A37). A31 waits on `QUESTIONS.md` Q1.
5. Done: test features (A17).

## Phase 3 — structural API

1. Done: one value response (A27).
2. Done: wrappers (A39).
3. Done: overlays (A14 with A51, A41, A23).
4. Done: dock and tabs (A11, A12).
5. Done: colour button (A15).

## Phase 4 — the validation rollout (A50 rules 2–4)

Done. The rules are in AGENTS.md, the kinds in `widget::domain`, and
`widgets::tests::coercing_inputs_at_their_worst_paint_no_nan` holds the frame property.

## Phase 5 — goldens

A7, A25 and A26 together: `golden` re-exports `image`, `render*` returns the `FrameReport`,
`Tolerance { max_delta, max_pixels }` with `EXACT` as default, the adapter sidecar and the orphan
report. Downstream suites change once.

## Phase 6 — features

1. **A22** wheel sense per axis, after phase 1 step 3.
2. **A20** keyboard on toggles and ranges, after phase 3 step 1.
3. **A19** IME and focus traversal, each its own design.
4. **A10** the `meta` modifier, after phase 2 step 4.

## Order at a glance

Phase 0 decides; phase 1 must land first; phases 2 and 3 may interleave, except where a step names
a predecessor; phase 4 needs phase 1 step 1 and phase 3 step 1 (the index coercion touches
`ComboBox`, which A27 changes); phase 5 and phase 6 are independent of each other.
