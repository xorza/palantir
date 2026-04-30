# Clay

Single-header C UI layout library by Nic Barker. ~5000 lines of `clay.h`. Deferred immediate-mode: user code records a tree, Clay runs multi-pass layout, emits backend-agnostic render commands. Structurally the closest existing design to Palantir.

## 1. Arena memory model

One bump allocator (`Clay_Arena` at clay.h:219) holding `nextAllocation`, `capacity`, `char *memory`. The user supplies the backing buffer (`Clay_CreateArenaWithCapacityAndMemory`, clay.h:938) — Clay never calls `malloc`/`free`. Total budget is set up front from `Clay_MinMemorySize()`.

Memory is split into two regions, partitioned via `arenaResetOffset`:

- **Persistent** (`Clay__InitializePersistentMemory`, clay.h:2245): hash maps for element IDs and text measurement, scroll/transition state, free-lists, measured-word list. Allocated once. The arena's `nextAllocation` after this becomes `arenaResetOffset`.
- **Ephemeral** (`Clay__InitializeEphemeralMemory`, clay.h:2220): `layoutElements`, `renderCommands`, `openLayoutElementStack`, `layoutElementChildren`, `layoutElementChildrenBuffer`, `treeNodeVisited`, `wrappedTextLines`, etc. Reset every frame by `arena->nextAllocation = arenaResetOffset`.

All "arrays" are `{ capacity, length, internalArray* }` slices into the arena (`CLAY__ARRAY_DEFINE`). Same arena holds tree, scratch BFS/DFS buffers, and final render command list.

## 2. Tree representation

```c
typedef struct Clay_LayoutElement {
    Clay__LayoutElementChildren children;   // { int32_t *elements; uint16_t length; }
    Clay_Dimensions dimensions;
    Clay_Dimensions minDimensions;
    union { Clay_ElementDeclaration config;
            struct { Clay_TextElementConfig textConfig; Clay__TextElementData textElementData; }; };
    uint32_t id; ...
} Clay_LayoutElement;
```

Elements live in `context->layoutElements` (clay.h:1212, flat array). Children are **index slices**, not linked lists: `children.elements` points into the shared `context->layoutElementChildren` int array. During recording, child indices accumulate in a scratch ring (`layoutElementChildrenBuffer`); on `Clay__CloseElement` they are copied into `layoutElementChildren` and the parent's `children.elements` is set to that slice's start (clay.h:1884). Open/close discipline is enforced by an explicit `openLayoutElementStack` of indices pushed in `Clay__OpenElement` (clay.h:2041) and popped in `Clay__CloseElement` (clay.h:1867).

## 3. Macro DSL

```c
CLAY({ .id = CLAY_ID("X"), .backgroundColor = ... }) {
    CLAY({...}) { ... }
    CLAY_TEXT(str, ...);
}
```

Desugars (clay.h:139-151) to a `for` loop whose init runs `Clay__OpenElement()` + `Clay__ConfigureOpenElement(...)`, body runs once (controlled by the latch sentinel `CLAY__ELEMENT_DEFINITION_LATCH`), increment runs `Clay__CloseElement()`. The `for`-with-single-iteration trick guarantees close even with `break`/`continue` inside, and lets `{ }` after `CLAY(...)` look syntactically like a block. Works in C because configuration is a struct passed by value through compound literal `(Clay_ElementDeclaration){...}` (`CLAY__INIT`).

## 4. Layout passes

`Clay_BeginLayout` (clay.h:4355) opens the root. User code records the tree. `Clay_EndLayout` (clay.h:4448) closes the root and calls `Clay__CalculateFinalLayout` (clay.h:2573). During recording, `Clay__CloseElement` already computes intrinsic `dimensions` and `minDimensions` per axis (sum along layout axis, max on cross axis, plus padding/childGap — clay.h:1880-1930). That's pass zero, post-order, baked into recording.

Then `Clay__CalculateFinalLayout` runs:

1. **X-axis sizing** — `Clay__SizeContainersAlongAxis(true, ...)` (clay.h:2281). BFS top-down; for each parent, expand `PERCENT` children, then if content overflows shrink in two-largest steps, if underflows distribute slack to `GROW` children in two-smallest steps (clay.h:2402-2491). This is the iterative "give to the smallest until it matches the next-smallest" loop — yields equal final widths and is independent of declaration order.
2. **Text wrap** — for each text leaf collected during pass 1, line-break against the now-known container width using cached measured words; updates the leaf's height (clay.h:2584-2636).
3. **Aspect ratio** — set heights from widths.
4. **Height propagation** — DFS post-order, propagate text-induced height changes back up to parents (clay.h:2646-2691).
5. **Y-axis sizing** — `Clay__SizeContainersAlongAxis(false, ...)`.
6. **Aspect ratio** width pass.
7. **Position + render commands** — DFS pre-order; assign each child a `boundingBox`, emit `Clay_RenderCommand`s in draw order (clay.h:2716+).

So three "size" passes (initial close-time bottom-up, then top-down X, then top-down Y) plus a height-propagation DFS in between, plus position+emit. Multi-pass is required because text wrap height depends on assigned width.

## 5. Sizing model

`Clay_SizingAxis` is a tagged union (`CLAY__SIZING_TYPE_FIT|GROW|PERCENT|FIXED`) with `{minMax}` or `{percent}`. Macros at clay.h:70-76.

- `FIT` (default, ≈ WPF `Auto` / Palantir `Hug`) — desired = content + padding, clamped to `minMax`.
- `GROW` (≈ WPF `*` / Palantir `Fill`) — start at `FIT`, then expand to consume slack on the layout axis, capped at `max`. Multiple `GROW` siblings split slack via the two-smallest iterative algorithm: each round, pick the smallest, raise it to match the second-smallest (or absorb all remaining slack divided among them, whichever is less). Equal in steady state, but respects per-child `max` caps gracefully.
- `PERCENT` — `(parentSize - paddingAndGaps) * percent`, computed before `GROW` distribution.
- `FIXED` — `min == max == n`.

Vs WPF: `FIT == Auto`, `GROW == Star (single weight)`, `FIXED == Fixed`. `PERCENT` has no direct WPF analogue (closer to CSS `%`). On the **cross axis**, `GROW` simply stretches to parent inner size (clay.h:2493-2510), matching WPF's `HorizontalAlignment::Stretch`.

## 6. Rendering

Clay does not draw. `Clay_EndLayout` returns `Clay_RenderCommandArray` (clay.h:786-818): a flat array of `{boundingBox, renderData, id, zIndex, commandType}` already sorted by draw order. Types: `RECTANGLE`, `BORDER`, `TEXT`, `IMAGE`, `SCISSOR_START/END`, `CUSTOM`. Backends (raylib, SDL, web canvas, etc., in `renderers/`) walk the array. This is the cleanest possible decoupling — the layout core has zero rendering dependencies.

## 7. Text measurement callback

`Clay_SetMeasureTextFunction(fn, userData)` (clay.h:1004) installs a function pointer. During X-axis sizing Clay calls `Clay__MeasureText(slice, config, userData)` per word and caches the result in `measureTextHashMapInternal` keyed by hash of (string content + config) — `Clay__MeasureTextCached` (clay.h:1639). Cache survives across frames via `generation` counter; entries not touched this frame are evicted. Words are stored as a linked list (`Clay__MeasuredWord.next`) for wrapping.

## 8. ID system

`Clay_ElementId = { id, offset, baseId, stringId }` (clay.h:245). Built by `Clay__HashString` (FNV-1a, clay.h:1436). `CLAY_ID("Label")` = global hash; `CLAY_ID_LOCAL("Label")` seeds with the parent's id (`Clay_GetOpenElementId()`) so the same string under different parents yields different ids; `CLAY_IDI(label, i)` adds a numeric offset for collections. IDs are persisted in `layoutElementsHashMap` → `Clay_LayoutElementHashMapItem` (clay.h:1269), which carries `boundingBox` and `generation`. Hit-testing (`Clay_PointerOver(id)`) and scroll/transition state look up by id, so identity persists across frames even though the tree itself is rebuilt.

## 9. Lessons for Palantir

Direct copies:

- **Single arena, two zones**, ephemeral reset by offset rewind. Palantir already has `Tree.nodes: Vec<Node>` and `Tree.shapes: Vec<Shape>` — same idea, less manual. Persistent state map stays separate (Palantir already plans this).
- **Index-slice children, not per-node `Vec<NodeId>`.** Palantir's CLAUDE.md mentions linked-list `first_child`/`next_sibling` à la `indextree`. Clay's *contiguous index range per parent* is even better for cache and BFS; the trick is filling it on close from a scratch buffer. Worth considering.
- **Iterative "two-smallest" GROW distribution** (clay.h:2455-2490). Beats naive equal-split because it respects per-child `max` caps without a separate fixup. Translate verbatim into `resolve_axis`.
- **Stable IDs from parent-id-seeded FNV hash** of a user string — gives `CLAY_ID_LOCAL` semantics for free, supports collections via numeric offset. Maps onto `WidgetId`.
- **Render commands as flat sorted array**, layout core unaware of wgpu. Palantir's Paint pass should consume an analogous `Vec<DrawCmd>` rather than walking the tree directly — preserves the Node/Shape decoupling and makes a `RenderBundle` cache trivial later.
- **Text measurement as a cached callback keyed by (text + config) hash, with generation-based eviction.** Drop straight into glyphon integration.

Simpler in Rust:

- No `CLAY({...}) { ... }` macro hack — `ui.stack(|ui| { ... })` closures already give scoped open/close with `Drop`-based safety.
- No manual arena sizing/`Clay_MinMemorySize`. `Vec` grows; reuse with `clear()` keeps capacity.
- No `union` of layout/text data — use enum.
- No `void *userData` for measure-text callback — `dyn FnMut` or generic over a `TextMeasurer` trait.
- No `nextIndex` free-list hash maps — `HashMap<WidgetId, _>`.

What Clay does that Palantir's current sketch should adopt:

- The **height-propagation DFS** between X-sizing and Y-sizing. Without it, parents whose intrinsic height was computed pre-wrap will be wrong. Palantir's current single measure/single arrange will fail multi-line text; plan for this third pass before text wrapping lands.
- Cache the close-time intrinsic `dimensions` so the X-pass starts from FIT sizes — avoids re-walking.
- Emit `SCISSOR_START` / `SCISSOR_END` commands rather than carrying clip stacks into the renderer.
