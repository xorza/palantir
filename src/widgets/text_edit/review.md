# `src/widgets/text_edit/` — review

Scope: `mod.rs`, `tests/` (sharded), `design.md`.
Posture-cross-checked against `CLAUDE.md` / `DESIGN.md`. Code is well
factored overall — most findings are small. The biggest ones are (1) a
steady-state allocation per frame, (2) the test file violating the
"split fat-test files" rule, and (3) some stale docstrings after the
recent grapheme migration.

## Status — items resolved in the follow-up pass

- ✅ **Item 1** (per-frame buffer clone) — open, see "Still outstanding"
  below. Needs a `Shape::Text.text` lifetime decision; out of scope for
  a TextEdit-only fix.
- ✅ **Item 2** — `tests.rs` (2172 lines) split into
  `tests/{mod,apply_key,selection,undo,grapheme,word_nav,multi_click,blink,scroll,click,multiline,theme,context_menu}.rs`,
  each well under 500 lines.
- ⚠️ **Item 3** — partially. `apply_key` docstring updated, but the
  function itself wasn't restructured; the "pure" framing in design.md
  was softened in the docstring.
- ✅ **Item 4** — `show()` now holds a single `state_mut` borrow
  covering the post-input compare, `update_scroll`, and the blink reset
  / `last_caret_change` snapshot. Three fewer hashmap probes.
- 🟡 **Item 5** — first-frame `wrap_target = None` is still documented
  in design.md / known-gotchas; tracking via `request_discard` slice in
  `docs/roadmap/invalidation.md`.
- ✅ **Item 6** — `word_range_at` dead-store arm flattened to a single
  `if … && let …`.
- ✅ **Item 8** — dropped the 1×1 placeholder rect. `Shape::Text::is_noop`
  no longer ANDs on `local_rect_paint_empty`; TextEdit passes a 0×0
  anchor.
- ✅ **Item 9** — `TextEditState` and `apply_key` docstrings refreshed
  to reflect grapheme walks and the function's actual parameters.

Behavioural additions during the follow-up:

- `BLINK_STOP_AFTER_IDLE = 30s` — focused-but-idle editors stop
  scheduling blink wakes after 30 s and the caret stays solid, so an
  unattended host doesn't keep repainting at 2 Hz forever. Pinned by
  the appended assertion in `caret_blinks_on_and_off_while_focused`.

## Architectural issues

### 1. Per-frame buffer clone violates the alloc-free posture

Every `show()` clones the entire host buffer into a new `String` and
hands it to `Shape::Text` as `Cow::Owned`:

`mod.rs:564-568` — the `text_ptr.clone()` runs unconditionally on every
recorded frame for a focused, non-empty editor. With a 10 KB buffer
that's a 10 KB allocation per frame just to keep the shape alive past
the `show()` borrow.

The root cause is upstream: `Shape::Text.text: Cow<'static, str>`
(`src/shape.rs:87`) — `'static` rules out borrowing from the host's
`&mut String`. So this isn't a TextEdit-only fix, but it's where the
per-frame allocation actually lands.

Suggestion: track the text-bytes hash in `TextEditState`. When the host
hasn't mutated the buffer (hash unchanged) re-use last frame's cloned
`Arc<str>` / `Cow::Owned`. Or — bigger change — widen `Shape::Text` to
`Cow<'frame, str>` so recording can borrow the host buffer directly and
the shapes arena copies once at the recording boundary. Either keeps
typing alloc-free in steady state.

### 2. `tests.rs` violates the "split fat-test files" rule

`tests.rs` is 2172 lines (vs. `mod.rs` at 1235) — by CLAUDE.md's >150
lines / >40% threshold, this file should be sharded into
`tests/{keyboard, multi_click, scroll, blink, word_nav, grapheme,
clipboard, context_menu, focus, theme, multiline}.rs`. The current
arrangement makes future migrations on a single feature touch a
2-thousand-line file.

Concrete shape:

```
tests/
  mod.rs          // shared helpers: editor_at, body, frame_at, press, shift, …
  apply_key.rs    // pure-fn table tests
  selection.rs    // selection_state_transitions
  undo.rs         // undo/redo cases
  grapheme.rs     // grapheme boundary + backspace cluster tests
  word_nav.rs     // word boundary, word_range_at, apply_key_word_nav_cases
  multi_click.rs  // double-/triple-click selection
  blink.rs        // caret_blinks_on_and_off + wake
  scroll.rs       // scroll_keeps_caret + click hit-test compensation
  click.rs        // click_lands_caret + padding + dual-editor focus
  ime.rs          // text_event_inserts + paste_strips_newlines
  multiline.rs    // multiline_*
  theme.rs        // each_text_widget_reads_its_own_theme_path_*
  context_menu.rs // secondary_click + open + menu interactions
```

Helpers like `body`, `frame_at`, `editor_id` are inlined in multiple
tests today (`tests.rs:1738-1750`, `1825-1841`, `2070-2085`) — folding
them into the new `tests/mod.rs` removes the duplication.

### 3. `apply_key`'s pure-function contract leaks padding-of-features

`apply_key` (`mod.rs:930-1073`) is "pure on `(text, state, kp)`" except
it now takes `multiline` and `clipboard_active` as in-band flags and
emits a vertical-motion side-channel via `out_vertical`. With overflow
handling + multi-click + blink layered on top, the line between
`apply_key` (pure) and `handle_input` (impure) has gotten blurry.

The actual split that matters now is keyboard-vs-pointer, not pure-vs-
impure: pointer / multi-click / scroll / blink all live in
`handle_input`, and keyboard nav lives in `apply_key`. The "pure" framing
is still useful for testability but stretched. Worth either:

- renaming `apply_key` → `apply_keypress` and dropping the "pure"
  language in design.md, or
- pulling clipboard / undo dispatch out of `apply_key` into a separate
  `apply_shortcut` step run before the key-by-key dispatch, leaving
  `apply_key` strictly for editing keys (chars, backspace/delete,
  arrows, home/end, escape). That'd shrink the function from ~150 lines
  to ~80 and make the shortcut dispatch easy to reuse for a future
  command palette.

### 4. `state_mut::<TextEditState>(id)` is called 6× in one `show()`

`mod.rs:437, 457, 475, 492, 636, 648, 667, 678` — every call is a
`HashMap::entry` lookup. Borrow choreography forces this (we can't hold
`&mut state` across `ui.text` / `ui.node` calls), but at least 3 of
them can be folded: the `caret_before / sel_before` snapshot (437), the
`caret_changed` recompare (457), and the `update_scroll`-then-blink
pair (475 / 492) could share one borrow.

The fix is a small helper, e.g. `with_state<T>(ui, id, |s| -> T)`, or
just collapsing the three reads at the top of `show()` into one block.
Not perf-critical (single-digit lookups), but it removes ambiguity
about which `state` mutations are seen by which downstream stage.

### 5. First-frame `wrap_target = None` is now a known correctness gap

`mod.rs:422-426` reads `wrap_target` from `response.rect` which is
`None` on the first recorded frame. Multi-line editors lay out
unwrapped on that first frame, then re-layout once cascade catches up.
Documented in `design.md` and roadmap "Known gotchas", but the symptom
is that `update_scroll` runs against the wrong layout for one frame too
(caret_pos.y is computed at unwrapped layout). Most editors don't
notice; a tall first-frame multi-line editor with long content might
see a one-frame scroll flash.

If `request_discard` (per `docs/roadmap/invalidation.md`) lands, plumb
it here.

## Simplifications

### 6. `word_range_at` has a dead-store branch

`mod.rs:1207-1214`:

```rust
let mut start = byte;
if forward_char.is_some_and(|c| char_kind(c) == anchor_kind) {
    // Caret is at the start of the anchor char — don't step back …
} else if let Some(c) = backward_char {
    start = byte - c.len_utf8();
}
```

The first `if` arm is a no-op with an explanatory comment. The comment
is doing all the work; the arm doesn't have to exist. Replace with:

```rust
let mut start = byte;
if !forward_char.is_some_and(|c| char_kind(c) == anchor_kind)
    && let Some(c) = backward_char
{
    start = byte - c.len_utf8();
}
```

Two fewer levels of nesting and the intent is clearer.

### 7. `next_word_boundary` / `prev_word_boundary`: phase-1 vs phase-2 duplication

`mod.rs:1137-1182` — `next_word_boundary` and `prev_word_boundary` have
the exact same structure (skip whitespace, then skip same-kind run),
just mirrored. The mirroring is small but it duplicates the `target_kind`
loop + the same-kind loop in both directions.

Either accept the duplication (current state, fine) or factor through
a generic `walk_word(text, from, dir: Step)`. Probably not worth
introducing a helper for two callers, but flag in case word-boundary
semantics ever change — the two functions must change together.

### 8. The 1×1 placeholder `local_rect` for text is a workaround comment

`mod.rs:571-579` — `local_rect: Some(Rect::new(padding.left - scroll.x,
padding.top - scroll.y, 1.0, 1.0))` with a comment "Size is unused under
`Align::Auto`; pick something positive so `is_paint_empty` doesn't
reject the shape."

This is a fragile coupling: TextEdit reaches into encoder semantics
(text positions at `leaf.min` under `Auto`, ignoring size) and
`is_paint_empty` semantics (positive w/h). If either changes, this
silently breaks.

Two cleaner options:

- Encode "positioned text with shaped-size bbox" as a first-class
  `Shape::Text` variant that doesn't go through `is_paint_empty`.
- Pass the shaped measurement back out of the shaper and put it into
  `local_rect.size` (still ignored by Auto, but at least correct).

The simplest pragmatic fix: skip the `is_paint_empty` check for
`Shape::Text` (`src/shape.rs:391`) since text emptiness already gates
on `text.is_empty()`. That removes the workaround entirely.

### 9. Stale docstring on `TextEditState`

`mod.rs:40-43`:

```
/// v1 mutates byte boundaries that always coincide with
/// codepoint boundaries (insert at caret, remove one codepoint at a
/// time on backspace/delete) so a malformed offset shouldn't be
/// reachable from inside the widget.
```

Codepoint-at-a-time was true before grapheme-aware boundary walks
shipped. Backspace / Delete now remove whole grapheme clusters, which
can span 2-18+ bytes. Either widen the comment to "grapheme cluster"
or just drop the parenthetical — the invariant ("malformed offset
unreachable") is what matters, not the mechanism.

### 10. `MULTI_CLICK_WINDOW` / `MULTI_CLICK_RADIUS` constants vs. theme

`mod.rs:28-32` — both are crate-local `f32` consts. Standard OS
behavior varies (Windows: 500 ms, macOS: ~500 ms but configurable, GTK:
250 ms). Long-term these probably belong on `TextEditTheme` or a
top-level `InputTheme` so a host app can match the OS's actual setting.
Not urgent, but the comment "Standard OS default" overpromises.

## Smaller improvements

- `mod.rs:38-44`: docstring talks about "malformed offset shouldn't be
  reachable" — but `byte_at_xy` returning a non-grapheme-boundary in
  combining-mark text could in principle land caret mid-grapheme. Cosmic
  is grapheme-aware, so it doesn't, but the invariant is now upheld by
  cosmic, not by widget code. Worth noting.
- `mod.rs:457`: `sel_before != ui.state_mut::<TextEditState>(id).selection`
  — re-borrows just to re-read the selection. Could be folded into the
  snapshot block above.
- `mod.rs:497-500`: blink-phase computation uses `f32::floor` and casts
  to `u64`. A long-running editor's `elapsed` could grow into the
  hundreds (focused for an hour with no input). At `elapsed = 1e6`,
  `phase as u64` is fine but the f32 step loses precision near
  `BLINK_HALF`. Cap or clamp at, say, 60 phases (30 s of unchanged
  caret) and treat "definitely visible" beyond that.
- `mod.rs:781`: `ui.time.saturating_sub(state.last_press_time)` — if
  the editor is re-shown after a host time-jump backward (rare), this
  underflows to 0 and could chain-trigger a multi-click. Harmless but
  surprising.
- `tests.rs` defines `body` and `frame_at` inline in three places
  (1738, 1825, 2068) — extract once in `tests/mod.rs`.
- `mod.rs:923-929`: the long docstring on `apply_key` still calls it
  "pure on `(text, state, key)`" — now also takes `multiline` and
  `clipboard_active` and emits `out_vertical`. Update.
- `design.md:236-241` and `design.md:267-308` describe overflow + blink
  + word nav inside the "Edit set (`apply_key`)" section, but the
  rendering side (clip mode, shape offset, repaint scheduling) lives in
  `show()`, not `apply_key`. Re-organize so each subsystem's home is
  contiguous (state → input → scroll → blink → rendering).

## Open questions

- **Theme vs. configuration**: should `MULTI_CLICK_WINDOW` /
  `BLINK_HALF` / `MULTI_CLICK_RADIUS` move to `TextEditTheme`? They're
  more behavioural than visual, so maybe a separate `InputTheme`?
- **Shape::Text alloc**: is the per-frame buffer clone acceptable given
  CLAUDE.md's posture? If so, document the exception. If not, item 1
  needs a real plan (Arc<str> on `Shape::Text`? `Cow<'frame, str>`?).
- **Multi-click drag-after-doubleclick**: macOS extends selection
  word-by-word on a held drag after a double-click. Today we clear
  `drag_anchor` on double-click, so drag is no-op. Add to Tier 2
  roadmap or accept the gap?
- **`prev_grapheme_boundary` from a non-grapheme-boundary offset**: the
  current implementation handles it (returns the previous boundary),
  but the comment in `TextEditState` claims this is unreachable. Either
  prove it (assert at the call site) or weaken the comment.

## Still outstanding

1. **Per-frame buffer clone (item 1)** — `Cow::Owned(text_ptr.clone())`
   still allocates every frame. Needs a `Shape::Text.text: Cow<'frame,
   str>` or `Arc<str>` rework upstream. Out of scope for a
   TextEdit-only patch.
2. **`apply_key` restructure (item 3)** — clipboard / undo dispatch
   still mixed into `apply_key`. Could split into `apply_shortcut`
   (clipboard, undo, redo, select-all) ahead of `apply_keypress`
   (chars, backspace/delete, arrows, home/end, escape). Defer until a
   command palette / external shortcut consumer needs the dispatch.
3. **First-frame `wrap_target = None` (item 5)** — depends on the
   `request_discard` slice on the invalidation roadmap.
4. **`MULTI_CLICK_WINDOW` / `BLINK_HALF` on a theme (item 10)** —
   behavioural, not visual; probably belongs on an `InputTheme`.
   Skipped — no consumer asking yet.
5. **CJK / non-Latin word-break iterator** — `CharKind` is still ASCII
   classifier territory. Noted in design.md; defer to a Unicode
   word-break upgrade.
