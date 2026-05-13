# `src/widgets/text_edit/` — review

Scope: `mod.rs` (1245 lines), `tests/` (sharded, 12 files), `design.md`.
Cross-checked against project posture in `CLAUDE.md`.

Code is in good shape after the prior pass — tests are sharded, the
state-borrow choreography is consolidated, and the `apply_key` /
`handle_input` split reads cleanly. Remaining findings cluster around
(a) one steady-state allocation that's structural, (b) some
parameter-bag smells now that input is feature-rich, and (c) a few
small contract drifts.

## Architectural issues

### 1. Per-frame buffer clone is still the dominant alloc

`mod.rs:577` — `Cow::Owned(text_ptr.clone())` is unconditional on every
recorded frame for a focused / non-empty editor. With a 10 KB buffer
that's a 10 KB heap copy per frame to satisfy `Shape::Text.text:
Cow<'static, str>`. This is the only systematic violation of the
"steady-state heap-alloc-free after warmup" rule in this widget.

Root cause is upstream (`Shape::Text` lifetime). Three viable shapes:

- Widen to `Cow<'frame, str>` carried through `Shapes` and copied once
  at the arena boundary.
- Add an `Arc<str>` variant and cache last frame's clone in
  `TextEditState`, keyed by a `text.len() + hash` guard so we only
  re-clone when the host actually mutates.
- Smallest local change: hash the buffer into `TextEditState`,
  re-use an `Arc<str>` (or boxed `String`) when unchanged. Costs one
  `Arc::clone` per frame instead of an N-byte memcpy.

The hash-and-cache approach is feasible without touching `Shape`.

### 2. `handle_input` / `update_scroll` parameter bags

`handle_input` takes 10 parameters (`#[allow(clippy::too_many_arguments)]`
at `mod.rs:711`); `update_scroll` takes 8 (`mod.rs:261`). Five of those
parameters in each — `font_size`, `line_height_px`, `padding`,
`wrap_target`, `family` — are the resolved text-style triplet plus
geometry, computed once at the top of `show()` and threaded down two
levels.

Suggestion: introduce a `ShapeCtx { font_size, line_height_px,
wrap_target, family, padding }` (locally in this module — not a public
type) constructed once in `show()` and passed by value. Same data, one
name, no clippy allow.

`update_scroll`'s `caret_x` / `caret_y_top` / `line_height` /
`caret_width` group is a `CaretGeom`-shaped quad — the call-site at
`mod.rs:481` already builds it from `caret_pos` and `theme.caret_width`,
so passing `(caret_pos, theme.caret_width)` would be cleaner.

### 3. `apply_key` carries pointer / menu concerns it shouldn't

`mod.rs:938` — `apply_key` is documented as the keyboard path but takes
`clipboard_active: bool` (= "no context menu open") and threads it
through to gate the clipboard shortcut routes. That's input-layer state
leaking into a function that should be `(text, state, keypress, mode)`.

Cleaner factoring: run a `dispatch_shortcut(...)` step in
`handle_input` *before* the per-key loop. It owns clipboard / undo /
select-all / paste, returns `bool consumed`. `apply_key` shrinks to
chars + edits + navigation. Bonus: shortcut dispatch becomes reusable
for a future command palette without dragging in editor specifics.

The vertical-motion side-channel (`out_vertical: &mut
Option<VerticalMotion>`) is the same pattern — an out-param emitted
because `apply_key` lacks shaper access. With the dispatch split above,
returning an enum from `apply_key` (`Consumed { blur, vertical:
Option<VerticalMotion> }`) drops the `&mut` out-param.

### 4. `update_scroll` returns the value it just stored

`mod.rs:271-298` mutates `state.scroll` and returns `Vec2` — the
returned value is always `state.scroll`. Caller binds it at
`mod.rs:482`. Either return the new scroll and let the caller assign,
or just mutate and read back. Both-at-once is a low-grade footgun:
future refactors can drift the two.

Pick mutation-only — the function is named `update_scroll` and already
takes `&mut state`. Drop the return type.

### 5. State row touched 6+ times per `show()`

`mod.rs:441, 478, 644, 656, 675, 686` — every call is a `HashMap::entry`
probe. The prior pass collapsed three reads in the scroll/blink block;
the remaining four are inside the context menu closure. Each menu item
that mutates does its own `ui.state_mut::<TextEditState>(id)` lookup.

Not a perf issue (single-digit FxHashMap lookups), but it muddles the
"who saw which mutation" picture: a `Cut` followed by a `Paste` in the
same menu invocation each rebinds the state row. A small
`with_state<R>(ui, id, |s| -> R)` helper or a single `state_mut` at the
top of the closure would unify the four touches.

## Simplifications

### 6. `apply_history`'s defensive clamps are dead

`mod.rs:130-137` — `snap.caret.min(text.len())` and
`selection.filter(|a| a <= text.len())` both run after
`*text = snap.text`, so `text.len() == snap.text.len()`. The clamps
only fire if `EditSnapshot` was constructed inconsistently, which
`record_edit` (the only constructor) doesn't permit. Either delete the
clamps or replace with a release `assert!(snap.caret <= snap.text.len())`
per CLAUDE.md invariant-assert posture.

### 7. `drag_anchor` + `prev_pressed` + `click_count` model one state machine

`mod.rs:59-62, 93` — `drag_anchor: Option<usize>` is `Some` while a
single-click drag is active and `None` otherwise; `prev_pressed: bool`
records last frame's `pressed`; `click_count` tracks the multi-click
streak. On the drag branch we check `drag_anchor.is_some()`
(`mod.rs:825`) instead of `prev_pressed` directly.

A single `press_state: enum { Up, Dragging { anchor }, WordSelected,
AllSelected }` (with multi-click history kept separate) would model
the state machine in one place. Not urgent — current trio works — but
the three-field encoding has redundancy.

### 8. `state.scroll: Vec2` for a one-axis-at-a-time scroll

`mod.rs:77, 280-296` — single-line uses `.x`, multi-line uses `.y`, and
`update_scroll` zeros the inactive axis on every call. The data shape
is "a single `f32` whose axis depends on `multiline`". Probably leave
`Vec2` for forward-compat with bi-directional scroll in multiline mode,
but drop the zero-the-other-axis writes — they're only there because
the type allows the inactive axis to drift.

### 9. `clipboard_active` is misnamed

`mod.rs:734, 943` — the bool means "no context menu is open, so
keyboard shortcuts should fire here." It does not gate the clipboard
subsystem; it gates whether *this widget* claims the shortcut. Rename
`shortcuts_owned_by_widget` or invert to `menu_open`.

### 10. `is_word_nav` predicate inlined while `Shortcut` exists elsewhere

`mod.rs:1086-1092` hand-rolls platform mod gating. Everywhere else in
the file we use `Shortcut::cmd(...)`. The word-nav binding doesn't have
a key (just modifiers), so it doesn't quite fit `Shortcut`, but the
asymmetry is noticeable. Consider extending `Shortcut` (or a sibling)
to carry a modifier-only matcher.

## Smaller improvements

- `mod.rs:1107` — `next_grapheme_boundary` constructs a fresh
  `GraphemeCursor` per call. Backspace on a long string + word-nav
  walks call this in a loop. Cosmic-text already shapes the buffer; we
  could ride its cluster boundaries instead of segmenting a second
  time. Defer until profiling motivates.
- `mod.rs:1148-1165` / `1171-1190` — `next_word_boundary` /
  `prev_word_boundary` are mirror images. Acceptable duplication; flag
  if word semantics ever change so both edit together.
- `mod.rs:506-510` — blink-phase `(elapsed / BLINK_HALF).floor() as u64`
  on `f32`. With `BLINK_STOP_AFTER_IDLE = 30s` and `BLINK_HALF = 0.5s`,
  `phase` stays ≤ 60 before the early-out kicks in, so f32 precision is
  fine. The earlier review's "long-running editor" concern is mooted.
- `mod.rs:166-171, 184-188, 191-199` — every mutator calls `record_edit`
  *before* mutating, so the snapshot captures the pre-edit buffer.
  Correct, but easy to invert under refactor. One-line comment at the
  top of `record_edit` ("snapshot before mutate") earns its place.
- `mod.rs:439-443, 478-481` — `text_len_before` only approximates
  "buffer changed" (typing `a` then deleting `a` in one frame would
  tie). Consequence of a false negative is a one-frame late blink
  reset, invisible in practice. Flag only if a user reports flicker.
- `mod.rs:467-474` — `caret_pos` is computed via `ui.text.cursor_xy`
  once for scroll, and again inside `handle_input`'s vertical-motion
  resolver (`mod.rs:877`) for each Up/Down keypress. The shaper
  caches, so cost is small, but the duplication says the caret-pos
  derivation belongs on a small helper. Folds into item 2.
- `mod.rs:386-417` — `show()` is now 300+ lines with five named phases
  in comments. The phase boundaries are real; consider extracting
  Phase 2 (scroll + blink) and Phase 5 (context menu) into free
  functions next to `handle_input` so `show()` reads as five short
  calls plus the recording closure.

## Open questions

- **Behavioural constants on a theme.** `MULTI_CLICK_WINDOW`,
  `MULTI_CLICK_RADIUS`, `BLINK_HALF`, `BLINK_STOP_AFTER_IDLE` are
  hard-coded. No external consumer is asking yet. Move to
  `TextEditTheme`, leave on a future `InputTheme`, or accept as module
  constants?
- **Drag-after-doubleclick selection extension.** macOS extends
  word-by-word on a held drag after a double-click. Today `drag_anchor`
  clears on `click_count == 2`, so the drag is a no-op. Tier 2 nicety
  or out of scope?
- **CJK word boundaries.** `char_kind` is an ASCII-shaped classifier.
  Pinned in design.md as deferred — re-confirm.
- **`Shape::Text` lifetime.** Per item 1 — decision needed: hash+cache
  inside `TextEditState`, or widen `Shape::Text.text` to a non-static
  lifetime.

## Prioritized shortlist

If picking the next 3–5 things to do here:

1. **Item 1** — kill the per-frame `text.clone()`. Easiest local form:
   hash + cache an `Arc<str>` on `TextEditState`. Without this, the
   widget can't honour the "alloc-free in steady state" posture.
2. **Item 2** — fold `handle_input` / `update_scroll` params into a
   `ShapeCtx`. Removes two `clippy::too_many_arguments` allows and
   makes the next addition (line numbering, IME preedit) easier to
   thread.
3. **Item 3** — pull clipboard / undo / select-all into a
   `dispatch_shortcut` step ahead of `apply_key`. Returns the keyboard
   path to one job and makes dispatch reusable.
4. **Item 4 + 9** — drop the redundant return from `update_scroll` and
   rename `clipboard_active`. Both are minutes of work and remove
   small footguns.
5. **Item 6** — collapse `apply_history`'s dead clamps to an
   `assert!`. Tightens the invariant.

Items 5, 7, 8, 10 are cleanups worth doing opportunistically; open
questions wait on real consumers.
