# bevy_ui — reference notes for Palantir

`bevy_ui` is Bevy's built-in UI system. It's *retained mode* in the strictest sense: every UI element is an ECS entity with components (`Node`, `ComputedNode`, `UiTransform`, `Children`, …), and every frame is a sequence of ECS systems that read those components, run Taffy for layout, and submit a render phase. There is no per-frame tree rebuild and no widget object — widgets are bundles of components. Reading it is useful for two reasons: it shows what *full Taffy integration* costs, and it shows the price of trying to fit GUI into a pure ECS model.

All paths under `tmp/bevy/crates/bevy_ui/`.

## 1. Architecture

A UI element is an `Entity` with at minimum a `Node` (`src/ui_node.rs:489`) — the user-facing style component (flexbox/grid properties, sizing, padding, …) — plus `ComputedNode` (`src/ui_node.rs:29`), the output of layout (size, content insets, scrollbar rects, …). A button is `Button` (a marker `Component`, `src/widget/button.rs`) that `#[require(Node, FocusPolicy::Block, Interaction)]`s the components needed for hit-testing. There is no `Button::new(...)` builder; you `commands.spawn((Button, Node { ... }, BackgroundColor(...)))`.

Per frame, bevy_ui runs a chain of systems (declared in `src/lib.rs:33-49` and `src/update.rs`):

1. `detect_text_needs_rerender` — invalidate `ContentSize` for changed text.
2. `ui_layout_system` (`src/layout/mod.rs:77-180`) — sync `Node` → Taffy nodes, run Taffy, write back into `ComputedNode` + `UiGlobalTransform`.
3. `ui_focus_system` (`src/focus.rs`) — hit-test cursor against `ComputedNode` rects + `UiGlobalTransform`, set `Interaction::{None, Hovered, Pressed}` on each entity.
4. `ui_stack_system` (`src/stack.rs`) — produce `UiStack { uinodes: Vec<Entity> }` in z-order via `ZIndex`/`GlobalZIndex` plus tree depth.
5. Render extraction (in sibling crate `bevy_ui_render`) reads the stack and emits draw commands.

There is no "user code that runs each frame to declare UI." UI is mutated by spawning/despawning entities and changing component values; change detection (`Ref<Node>::is_changed()`, `src/layout/mod.rs:110`) decides what to re-sync.

## 2. Layout: Taffy via `UiSurface`

bevy_ui doesn't implement layout — it owns a `TaffyTree` and translates to/from it. `UiSurface` (`src/layout/ui_surface.rs:60+`) is a `Resource` holding `entity_to_taffy: EntityHashMap<LayoutNode>` + the `TaffyTree<NodeMeasure>` itself. `LayoutNode` (`src/layout/ui_surface.rs:20`) wraps a `taffy::NodeId` and an optional `viewport_id` for root nodes (a synthetic Taffy parent with the screen rect).

Each frame `ui_layout_system` (`src/layout/mod.rs:77`):

1. For every `Entity` whose `Node` or `ComputedUiRenderTargetInfo` changed, call `ui_surface.upsert_node(...)` which converts the bevy `Node` style to `taffy::Style` (`src/layout/convert.rs`) and writes it into the Taffy tree.
2. Sync `Children` ordering into Taffy parents.
3. Call `taffy_tree.compute_layout_with_measure(...)` for each viewport root.
4. Walk results: read `taffy_tree.layout(node_id)`, fill in `ComputedNode { size, content_box, padding_box, border, ... }` and `UiGlobalTransform`.

The full flexbox + CSS-grid feature set comes "free" because Taffy implements it — at the cost of an `Entity ↔ taffy::NodeId` map, double bookkeeping (Bevy's `Children` vs Taffy's parent slot), and a conversion layer (`src/layout/convert.rs`) for every style property.

Text intrinsic sizes flow through `ContentSize` + `Measure` (`src/measurement.rs`), which Taffy invokes as `MeasureFunc` callbacks during the solver pass — same pattern as `egui_taffy` and `iced`.

`GhostNode` (feature `ghost_nodes`) lets logical entities exist in the bevy hierarchy without being in Taffy — useful for "this entity holds metadata but doesn't participate in layout." The `experimental` module (`src/experimental/`) has the supporting `UiChildren`/`UiRootNodes` query helpers that skip ghosts.

## 3. Renderer: separate crate, queue-and-batch

Rendering lives in the sibling crate `bevy_ui_render` (not `bevy_ui` itself). `pipeline.rs` defines a single `UiPipeline` with a view uniform + image bind group (`bevy_ui_render/src/pipeline.rs:14-42`). `render_pass.rs:25` (`ui_pass`) walks `transparent_render_phases` and issues draw calls.

Per frame, an extraction system reads `UiStack.uinodes` in order, looks up each entity's `ComputedNode` + `BackgroundColor` + `BorderColor` + `BorderRadius`, and pushes a `UiBatch` quad into a `TransparentUi` render phase sorted by `FloatOrd(stack_index as f32)`. Specialized pipelines exist for box-shadows (`box_shadow.rs`), gradients (`gradient.rs`), images with 9-slice (`ui_texture_slice_pipeline.rs`), text (`text.rs`), and user materials (`ui_material_pipeline.rs`). Each pushes onto the same `TransparentUi` phase with its own pipeline id.

Vertex format is per-instance quads with corner radius/border baked in (the rounded-rect SDF is in the shader), then drawn as one indexed call per batch. This is essentially what Palantir is going to write, just embedded in Bevy's general render-graph machinery.

## 4. State model

ECS components *are* the state. Persistent widget state is just regular components on the entity:

- `Interaction` (`src/focus.rs`) — `None | Hovered | Pressed`. Updated by `ui_focus_system`.
- `Pressed`, `Checked`, `InteractionDisabled` — marker components from `interaction_states.rs`, added/removed by widget logic.
- `ScrollPosition`, `ComputedStackIndex`, `UiTransform`, `BackgroundColor`, `BorderRadius`, `Outline` — all Bevy `Component`s, queryable, change-tracked.

There is no `Id → Any` map, no hashed call site, no `WidgetId`. Identity is `Entity` (a 64-bit generational index). User code that wants "the count for *this* button" puts `Counter(u32)` on the entity and queries it. Cross-system communication happens through `Event`s (`bevy_ecs::event`) or by mutating components.

This is the polar opposite of immediate-mode: every widget has a stable ECS identity *forever*, until despawned. Reordering is `Children` mutation. State migration on identity change is moot — identity doesn't change.

## 5. Lessons for Palantir

**Useful contrasts (not direct copies):**

- **Taffy as a pluggable layout backend, not as the architecture.** Bevy demonstrates the conversion cost: every `Node` field has a `Into<taffy::Style>` shim (`src/layout/convert.rs`), every layout pass is `bevy → taffy → bevy`, change detection has to be threaded through both. Palantir's hand-written `measure`/`arrange` in `src/layout.rs` skips all of that. If we ever want flexbox, integrating Taffy is doable, but Bevy is the proof that it's not free — keep our own engine for the WPF-aligned subset and only bolt Taffy on if a user genuinely needs `flex-grow` semantics that `Sizing::Fill` can't express.
- **Stack-index z-order separated from layout.** `ComputedStackIndex` (`src/stack.rs:16`) and `UiStack.uinodes: Vec<Entity>` are the *render order*, computed in a separate pass from layout, driven by `ZIndex`/`GlobalZIndex` + tree depth. This decoupling is exactly what Palantir's paint pass needs once popups/tooltips/modals exist — z is not tree depth.
- **Per-feature pipelines on a shared phase.** `bevy_ui_render` has separate pipelines for solid quads, gradients, shadows, sliced images, text, user materials — but they all push into one `TransparentUi` phase that draws back-to-front. That's the right structure for Palantir: one ordered command stream, multiple pipeline ids, batched per `(pipeline, texture, scissor)`.
- **`ContentSize`/`Measure` callback for intrinsic text size.** Taffy's `MeasureFunc` hook, threaded through bevy's `NodeMeasure` (`src/measurement.rs`), is the same shape as what we'll need when glyphon-backed text measurement plugs into our `Measure` pass. Keep the API: `fn measure(&self, available: Size) -> Size`, called bottom-up.

**Avoid:**

- **ECS as the UI authoring surface.** Spawning entities + `#[require(Node, ...)]` is verbose, fragile across versions, and forecloses the immediate-mode `Button::new(id).label("x").show(&ui)` ergonomic. Bevy's choice is forced by the engine's identity; Palantir is a standalone GUI lib and shouldn't inherit it.
- **Double bookkeeping.** Maintaining `Children` *and* a parallel `TaffyTree` parent/child topology, syncing every frame via change detection, is a recurring source of "why is this not laying out" bugs in Bevy. Our linked-list children + flat `Tree.nodes` is one source of truth — the layout engine reads the same arena the recorder writes.
- **Style conversion layer.** Every Bevy UI release has a `convert.rs` PR fixing some Taffy/Bevy semantic drift. By owning measure/arrange we own the contract — no version-skew with an external solver.
- **Retained-mode "spawn once, mutate forever."** Useful for game UI where HUD elements live for the level. Painful for tool UIs where panels/tabs/lists come and go and identity has to follow. Palantir's "rebuild tree, persist state by ID" handles dynamic UI naturally; emulating it on top of ECS would mean despawn/respawn every frame, which defeats ECS.

The high-order takeaway: bevy_ui is "what if a UI was just an ECS world." It works, the renderer is good, the Taffy integration is a real reference for *how to integrate* a layout engine — but the authoring model is the wrong shape for Palantir's IM goals. Take the render-phase pattern and the stack-index decoupling; leave the rest.
