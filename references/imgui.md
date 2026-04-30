# Dear ImGui — Reference Notes

Notes from reading `tmp/imgui/{imgui.h,imgui.cpp,imgui_internal.h,imgui_widgets.cpp,imgui_draw.cpp}`. Focus: what ImGui's immediate-mode model nails, and what it gives up by *not* having a real measure pass.

## 1. Frame lifecycle

Three top-level entry points in `imgui.cpp`:

- `ImGui::NewFrame()` (5498) — advances `g.FrameCount`, processes input, runs `UpdateMouseMovingWindowNewFrame`, decays `HoveredIdPreviousFrame`/`ActiveIdPreviousFrame`, garbage-collects unused windows, opens an implicit "Debug" window. Resets all per-window `DrawList`s.
- User code — `Begin("...")` … widgets … `End()`. Widgets *paint as they're called*, into the current window's `DrawList`. There is no separate "build the tree" phase.
- `ImGui::EndFrame()` (5978) — finalizes hovered/active state, closes any windows the user forgot to `End()`, runs settings I/O.
- `ImGui::Render()` (6083) — sorts windows, walks them and concatenates their `DrawList`s into `ImDrawData` (vertex/index buffers + `ImDrawCmd` array). The renderer backend (e.g. `imgui_impl_wgpu.cpp`) consumes that.

So: **no "record then layout then paint."** Painting is interleaved with widget submission, using whatever cursor position is current.

## 2. Widget pattern: ItemSize + ItemAdd

Canonical body (`ButtonEx`, `imgui_widgets.cpp:784`):

```cpp
const ImGuiID id = window->GetID(label);
const ImVec2 label_size = CalcTextSize(label, ...);
ImVec2 pos  = window->DC.CursorPos;
ImVec2 size = CalcItemSize(size_arg, label_size.x + padX*2, label_size.y + padY*2);
const ImRect bb(pos, pos + size);
ItemSize(size, style.FramePadding.y);
if (!ItemAdd(bb, id)) return false;
bool pressed = ButtonBehavior(bb, id, &hovered, &held, flags);
RenderFrame(bb.Min, bb.Max, col, true, style.FrameRounding);
RenderTextClipped(...);
return pressed;
```

`ItemSize` (`imgui.cpp:11396`) advances `window->DC.CursorPos` by `size + ItemSpacing`, updates `CurrLineSize`/`PrevLineSize`, and bumps `CursorMaxPos` (used at `End()` to compute window content size for autofit/scrollbars). `ItemAdd` (`imgui.cpp:11258`) registers the bounding box and ID for hit-testing/nav, runs the clip-rect cull, and stores `g.LastItemData` for `IsItemHovered/Clicked/Active`. The widget then *immediately* draws into `window->DrawList`.

This is single-pass top-to-bottom. There is no measure step where children report sizes back up. A widget computes its size from its own intrinsic content (text width, fixed user `size_arg`) plus padding — never from siblings or from a future-known parent width.

## 3. Layout limitations and tricks

Children are positioned by the *cursor advance* invariant in `ItemSize`: after every item, `CursorPos.y` moves down by item height + `ItemSpacing.y`; `SameLine` rewinds Y and advances X. Width that "fills the parent" works because the *parent's* width is known when the child is submitted (window was already opened with a size), but the parent's *height* and total content width can only be known after children submit — too late to feed back.

Consequences:

- **Auto-sized windows take 2 frames to settle.** `BeginChild`/auto-fit windows initialize `AutoFitFramesX = AutoFitFramesY = 2` (`imgui.cpp:6650`); first frame uses a guess, observes `CursorMaxPos`, second frame applies the measured size. Same trick for `BeginChild` with `ImGuiChildFlags_AutoResizeX/Y`.
- **Horizontal "fill, sharing leftover equally"** is not really expressible. You hand-compute widths, or use `Tables` which has its own constraint solver.
- **Right/center alignment** of a row requires knowing the row width, which you don't, so you call `CalcTextSize`/`GetItemRectSize` ahead and subtract — caller-side measurement.
- `BeginGroup`/`EndGroup` (`imgui.cpp:11694`/`11724`) is the closest thing to a layout primitive: backs up `CursorPos`/`CursorMaxPos`, lets children submit, then on `EndGroup` computes `group_bb` from the observed `CursorMaxPos` and emits a synthetic `ItemSize`/`ItemAdd` so `IsItemHovered` etc. work on the whole group. It's *post-hoc bbox aggregation*, not a measure pass — the group still can't influence its children's sizes.

## 4. ID stack

`ImGuiID` is a 32-bit `ImHashStr`/`ImHashData` of input bytes, **seeded by `IDStack.back()`** (`imgui.cpp:9185`). `PushID(...)` hashes its argument with the current top and pushes the result; `PopID` pops. So the ID for a widget is the hash chain of every `PushID` from window root down to the widget's label. This is what makes call-site identity work: identical labels under different `PushID` parents get distinct IDs, identical-label siblings without `PushID` collide (the FAQ's `"##suffix"` trick).

Persistent state lives in `ImGuiStorage` (`imgui.h:2794`) — a sorted `ImVector<ImGuiStoragePair>` keyed by `ImGuiID`, with `GetInt/Bool/Float/VoidPtr` and `*Ref` variants. Each window owns one `StateStorage`. Tree nodes, collapse state, scroll, slider edit buffers all key into it by ID.

## 5. Draw lists

`ImDrawList` (`imgui.h:3273`) holds `VtxBuffer`/`IdxBuffer` and a `CmdBuffer` of `ImDrawCmd`s (`{ClipRect, TexRef, VtxOffset, IdxOffset, ElemCount, UserCallback}`, line 3175). Vertex format is `{ImVec2 pos, ImVec2 uv, ImU32 col}` — 20 bytes. Each high-level call (`AddRectFilled`, `AddText`) appends triangles and *coalesces* into the current `ImDrawCmd` as long as `ClipRect`/`TexRef` are unchanged; a change pushes a new command via `AddDrawCmd`. `PushClipRect` (`imgui.h:3309`, render-level) and `ImGui::PushClipRect` (input-level, line 992) split the buffer and intersect rects. `ImDrawListSplitter` (line 3227) is used by Tables/Columns to interleave layers and merge them in submission order at the end. Backends (`imgui_impl_wgpu`) just upload the two big buffers, then iterate `CmdBuffer` issuing one indexed draw per `ImDrawCmd` with the matching scissor and texture bind.

## 6. Hit-testing and input

Hit-testing happens *during* widget submission against the bb just computed. `ButtonBehavior` (`imgui_widgets.cpp:543`) calls `ItemHoverable(bb, id, ...)` which checks mouse pos vs the active clip rect and the topmost-window/popup gates, then sets `g.HoveredId = id`. Click activates: `SetActiveID(id, window)` writes `g.ActiveId`. A widget is "the active one" if `g.ActiveId == id`; release on hover → "pressed." Because submission is in order, the *last* hovered widget under the cursor wins, matching paint order (top-most). Two ID slots — `HoveredIdPreviousFrame` / `ActiveIdPreviousFrame` — preserve state across the frame boundary so widgets that disappear can still resolve `IsItemDeactivated`.

## 7. Windows, docking, viewports

`ImGuiWindow` (`imgui_internal.h`) owns: `IDStack`, `DC` (drawing context: `CursorPos`, `CursorMaxPos`, `Indent`, `LayoutType`, `GroupOffset`), `DrawList`, `StateStorage`, `ClipRect`, `Pos`/`Size`, `Scroll`, `ChildWindows`. Docking adds `DockNode`s (a tree of split rects + tab bars) overlaying the window list. Viewports (multi-OS-window) attach each top-level window to a `ImGuiViewportP` whose backend creates a real platform window; `ImDrawData` is then emitted per-viewport. Both are layered on top of the same single-pass widget engine — they manage *where* a window's local cursor coordinate space gets rendered, not how its contents lay out.

## 8. State storage

Per-window `ImGuiStorage`. Widgets call `window->StateStorage.GetIntRef(id, default)` to lazily allocate keyed slots: tree-node open/closed, slider scratch, child-window scroll. Sorted-vector lookup is O(log N), insertion amortized once per ID's lifetime. The pattern is "ID is hashed call site, storage is a flat sorted map" — exactly what Palantir's `Id → Any` will do.

## 9. Lessons for Palantir

**Keep:**

- Call-site IDs via a hashed stack (Palantir's `WidgetId`). Stable across frames, no parent has to allocate child slots.
- Builder-style widget API (`Button::new(id).label("x").show(&mut ui)`) mirroring `ImGui::Button(label)` ergonomics.
- ID-keyed external state map for scroll/focus/animation. Tree is throwaway, state is not.
- `ImDrawList`-style flat vertex+index+cmd buffers with auto-coalescing on `(clip_rect, texture)` changes; one wgpu draw per command. Use a splitter analog (or just a stable z-list) for popups/overlays.
- `BeginGroup`/`EndGroup`-style "treat this subtree as one item for hit-testing" — useful even with real layout.
- Hit-test against last frame's geometry (DESIGN.md §5 already plans this; ImGui actually hit-tests the *current* frame because it has bb at submission time, but the one-frame-stale variant is what you need when layout is deferred).

**Avoid (these are exactly what Palantir's record→measure→arrange fixes):**

- Painting during user code. ImGui can't run a real measure pass because pixels are already on the draw list by the time the parent could ask "how big are you?". Palantir records `Node`/`Shape` only — paint runs after arrange, so two-pass is free.
- Cursor-advance layout. It bakes *flow direction and spacing* into every widget body. A widget "knows" it's stacking vertically. Palantir's measure/arrange + container drivers (`HStack`/`VStack`) keep widgets layout-agnostic.
- Two-frame autofit jitter. `AutoFitFrames=2` is the visible scar of single-pass: first frame is a guess, second is the correction. Palantir measures bottom-up *within the same frame* on the recorded tree, so a window that hugs its content is correct on frame 1.
- Manual right/center alignment. With real `Sizing::Fill` siblings and `available` propagated downward in measure, "two buttons share the row equally" is a one-liner, not a `CalcTextSize` dance.
- Caller-side `CalcItemSize`/`SetNextItemWidth`. In Palantir, sizing is declared on the node (`Sizing::{Fixed, Hug, Fill}`) and resolved by the layout engine — widget authors don't poke `CursorPos`.

The mental model: ImGui = *immediate submit, immediate paint, cursor cursors itself forward*. Palantir = *immediate submit into a tree, deferred WPF measure/arrange, deferred batched paint*. Same call-site ergonomics; proper layout underneath.
