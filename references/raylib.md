# raygui — reference notes for Palantir

raygui is the immediate-mode UI shipped alongside raylib. It's a single C header — `raygui.h`, ~6000 lines — and is as bare as immediate-mode gets. No layout engine, no automatic sizing, no widget tree, no allocations after init. The user passes in a `Rectangle bounds` for every control and raygui draws it and returns interaction state. It's worth reading specifically *because* it's the floor: anything below this is just `DrawRectangle` calls.

Source for these notes: `tmp/raylib/examples/core/raygui.h` (vendored copy of raygui v5.0-dev, the canonical implementation).

## 1. Architecture

There is no architecture. Each control is a single function with a fixed signature:

- `int GuiButton(Rectangle bounds, const char *text)` (`raygui.h:781, 2038`)
- `int GuiCheckBox(Rectangle bounds, const char *text, bool *checked)`
- `int GuiSlider(Rectangle bounds, const char *textLeft, const char *textRight, float *value, float minValue, float maxValue)`

State that needs to persist across frames (`bool *active`, `int *value`, `Vector2 *scroll`) is passed by pointer — **the user owns it**. raygui itself holds only global style state, no widget instances. The library's persistent footprint is a fixed-size `guiStyle[]` array (`raygui.h:1465`, ~1.5 KB) plus an embedded icon atlas (`guiIcons[]`, `raygui.h:1152`, ~8 KB), all in `.data`. No heap.

Each control follows a strict three-step body, visible cleanly in `GuiButton` (`raygui.h:2038-2069`):

1. **Update**: hit-test mouse vs `bounds`, set local `state` to `STATE_NORMAL/FOCUSED/PRESSED/DISABLED`, return `1` on release-over.
2. **Draw**: call `GuiDrawRectangle(bounds, ...)` and `GuiDrawText(text, ...)` with style colors indexed by `state*3` — the four states are interleaved into the property table so `BORDER + state*3` picks the right color.
3. **Tooltip**: if focused and `guiTooltip` set, queue tooltip text.

Rendering goes straight through raylib's `DrawRectangle`/`DrawTexturePro` (`raygui.h:1501, 1551`). In standalone mode (`RAYGUI_STANDALONE`) the user provides those primitives.

## 2. Layout: there isn't one

The header is explicit (`raygui.h:113-117`): "raygui currently does not provide an auto-layout mechanism … layouts must be defined manually." Every widget call hard-codes its `Rectangle`. The companion tool `rGuiLayout` is a *visual editor* that emits hard-coded `Rectangle` constants into your source.

Consequence: nothing in raygui ever measures content. `GuiButton` does not look at the text width — it draws into whatever box you gave it and clips. `GuiLabelButton` is the one exception: it expands `bounds.width` to fit `GuiGetTextWidth(text)` (`raygui.h:2079`), but that's caller-side, not a measure pass.

Containers are the same: `GuiPanel`, `GuiGroupBox`, `GuiScrollPanel` (`raygui.h:1754`, `1697`, etc.) just draw decoration into a user-supplied rect. They don't lay out children — the user calls more `Gui*` functions with offset rects inside.

## 3. State model

Five module-level statics (`raygui.h:1435-1467`):

- `guiState: GuiState` — global override (`STATE_NORMAL/FOCUSED/PRESSED/DISABLED`). `GuiDisable()` flips this and forces every subsequent control into disabled appearance until `GuiEnable()`.
- `guiLocked: bool` — short-circuits all input.
- `guiAlpha: float` — applied inside `GuiDrawRectangle`/`GuiDrawText`.
- `guiControlExclusiveMode: bool` + `guiControlExclusiveRec: Rectangle` (`raygui.h:1446-1447`) — when a slider/textbox is being dragged/edited, it claims exclusive input by setting these; other controls' update step early-outs on `guiControlExclusiveMode`. The `Rectangle` doubles as the identity of which control owns the lock — there is no widget ID system.
- `guiStyle[]` — flat property table indexed by `(control * stride) + property` (`raygui.h:1621`).

That last point is worth dwelling on. raygui has **no `Id` system** — no hashed call sites, no parent stack. "Which slider is being dragged" is identified by `Rectangle` equality of its `bounds`. If two sliders share a bounds rect (impossible if you've laid them out at different positions, which you have, because there's no auto-layout), they would collide. This works precisely because every widget has a hand-picked screen rect.

## 4. Drawing model

`GuiDrawRectangle(rec, borderWidth, borderColor, color)` (`raygui.h:1551`) is the single shape primitive — every control body composes from it. `GuiDrawText` (`raygui.h:1550`) handles alignment, multi-line, word-wrap (added in 4.0). Both apply `guiAlpha` at draw time.

There is no draw list, no batching, no z-order. Calls go directly to raylib's immediate `DrawRectangle`, which itself batches into raylib's internal `rlgl` vertex buffer per-frame. raygui doesn't even know that. The "renderer" is raylib's whole 2D pipeline; raygui sits entirely on top of it.

This means **paint order = call order**, period. No overlay layer for tooltips except by drawing them last. `GuiTooltip` (`raygui.h:2065`) just records a pointer; the tooltip itself is drawn at end of frame by an explicit `GuiDrawTooltip` call the user can place wherever.

## 5. Lessons for Palantir

**The `RAYGUI_STANDALONE` mode is the take-home.** raygui is structured so the entire library compiles against four function pointers: `DrawRectangle`, `DrawTriangle`, `DrawTextEx`, `MeasureTextEx`, plus input getters. Anything that can produce those four operations runs raygui. Palantir's `Shape` enum + paint pass is the equivalent abstraction — keep it that small and the renderer stays swappable (wgpu now, software/D3D/Metal later if ever).

**Properties as flat arrays.** `guiStyle[control*stride + prop]` is ugly C, but it's a single allocation, cache-line dense, trivial to memcpy/serialize, and indexable by `(control_kind, property)` enums. Worth considering for Palantir's eventual theme system instead of a tree of `HashMap<Property, Value>`.

**Exclusive-mode by rect.** When dragging a slider or editing a textbox, raygui claims input with `guiControlExclusiveMode` + `Rectangle`. Palantir will want the same concept — "this widget owns input until release" — but keyed by `WidgetId`, not rect. The pattern of *one global "active widget" slot* generalizes; rect-as-identity does not.

**Avoid:**

- **No layout.** raygui is unusable for any non-trivial app without `rGuiLayout` baking screen positions into source. This is exactly the gap WPF-style measure/arrange fills. Palantir already rejects this; reading raygui is a reminder of why.
- **No widget IDs.** Identifying "the slider being dragged" by `Rectangle` value works only because every widget has a hand-placed unique rect. Add layout, and rects become computed and possibly equal across frames during a relayout — you need stable IDs. Palantir's `WidgetId` is non-negotiable.
- **Global mutable style with `guiState` override.** `GuiDisable()` setting a process-wide "everything is disabled" flag is convenient for raygui's scope but breaks for nested forms ("disable this panel only"). Palantir's per-node `Style` plus tree-walked inheritance is the right way; don't ship a global override flag.
- **Caller-supplied `Rectangle` per control.** It's the API equivalent of saying "the user's job is to be the layout engine." Builder pattern + `Sizing::{Fixed, Hug, Fill}` removes that burden entirely.

The mental model: raygui is `void DrawWidget(Rectangle, state*)`, called N times per frame. It's the *minimum viable immediate-mode UI*. Reading it pins down what Palantir is buying with everything above the `Shape` enum.
