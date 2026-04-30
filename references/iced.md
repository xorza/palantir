# Iced — reference notes for Palantir

Source: `tmp/iced/`. Focus: widget tree, layout protocol, `iced_wgpu` rendering. Iced is a retained Rust GUI with an Elm-style runtime and a wgpu backend. The widget tree is rebuilt every `view()` call, which makes its measure/arrange protocol structurally close to Palantir's "rebuild arena every frame" model — the wgpu backend is therefore the most directly reusable piece.

## 1. Architecture (Elm runtime)

The application defines `state`, a `Message` enum, `update(state, msg)`, and `view(&state) -> Element`. The runtime calls `view()` to obtain a freshly-built `Element<'a, Message, Theme, Renderer>` tree (`core/src/element.rs`); it is owned by an `iced_runtime::UserInterface` (`runtime/src/user_interface.rs`) for one frame. `UserInterface::build` calls `state.diff(root.as_widget())` to reconcile a persistent `widget::Tree` of per-widget `State` (scroll offsets, focus, animation) keyed by widget `tree::Tag`/`Id`, then runs `root.as_widget_mut().layout(state, renderer, &Limits::new(Size::ZERO, bounds))` to produce the root `layout::Node`.

The widgets themselves are throwaway value-types (built fresh by `view()`); only `widget::Tree` and `widget::tree::State` survive between frames. This is the same split Palantir uses: transient tree, persistent `Id → Any` state map. Iced's reconciliation uses `Tag` (a TypeId-ish marker) plus child-position; Palantir's hashed-call-site `WidgetId` is more stable across structural edits.

## 2. `Widget` trait (`core/src/widget.rs`)

```rust
trait Widget<Message, Theme, Renderer> {
    fn size(&self) -> Size<Length>;
    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &Limits) -> Node;
    fn draw(&self, tree: &Tree, renderer: &mut Renderer, theme: &Theme,
            style: &renderer::Style, layout: Layout<'_>, cursor: mouse::Cursor, viewport: &Rectangle);
    fn update(&mut self, tree, event: &Event, layout, cursor, renderer, shell, viewport);
    fn mouse_interaction(&self, tree, layout, cursor, viewport, renderer) -> mouse::Interaction;
    fn children(&self) -> Vec<Tree>;            // initial state for sub-tree
    fn diff(&self, tree: &mut Tree);            // reconcile children
    fn overlay(...) -> Option<overlay::Element>;
}
```

`layout()` is a real measure pass — it returns a `Node` with absolute child positions already resolved (no separate arrange call). `draw()` then walks the `Node` and emits primitives through the `Renderer`. There is no second top-down arrange — the parent in `layout()` is responsible for both sizing children and translating them via `Node::move_to` before returning. This collapses WPF's two passes into one recursive call where the recursion itself acts as both: child returns desired-size + sub-layout, parent positions and aggregates.

## 3. `Limits` — the constraint passed down (`core/src/layout/limits.rs`)

```rust
struct Limits { min: Size, max: Size, compression: Size<bool> }
```

Like WPF's `availableSize`, but two-sided: both a minimum and a maximum, with `Size::INFINITE` meaning unbounded. `Limits::width(Length)`/`.height(Length)` collapse based on the widget's declared `Length`:

- `Length::Fixed(n)` → clamps both `min` and `max` to `n` (forcing exact size).
- `Length::Fill` / `FillPortion` → leaves `max` alone, `compression=false` (resolve picks `max`).
- `Length::Shrink` → sets `compression=true` (resolve picks intrinsic, clamped to `[min,max]`).

`Limits::resolve(width, height, intrinsic) -> Size` is the canonical sizing rule (`limits.rs:149`). Containers shrink the limits via `.shrink(padding)` before recursing. This maps cleanly to Palantir's `Sizing`: `Fixed→Fixed`, `Fill→Fill`, `Shrink→Hug`, but Palantir tracks only an `available: Size` (one-sided) — adopting `Limits`'s `min` would let widgets enforce minimum sizes without a separate field.

## 4. `Node` — layout output (`core/src/layout/node.rs`)

```rust
struct Node { bounds: Rectangle, children: Vec<Node> }
```

A heap-allocated tree mirroring the widget tree. Each `Node` carries its already-positioned `Rectangle` (relative to its parent) and owns its children. `Node::move_to`, `align`, `translate` mutate the bounds. `Layout<'a>` (`layout.rs:14`) is a borrowed cursor: `(position: Point, &Node)` that recursively offsets when descending via `.children()`. `draw()` and `update()` receive a `Layout<'_>` rooted at the widget's bounds and read children as needed.

Versus Palantir: same precomputed-rectangle output, but Iced uses `Vec<Node>` per parent (heap allocation per frame per container) where Palantir's flat arena with linked-list children is cheaper. The tradeoff: Iced's recursion is straightforward; Palantir needs explicit measure/arrange drivers (`HStack`/`VStack`) that walk the linked list twice.

## 5. `iced_wgpu` rendering pipeline

Entry: `Renderer` (`wgpu/src/lib.rs`) holds an `Engine` (pipelines + device) and a `layer::Stack<Layer>`. Each `Layer` (`wgpu/src/layer.rs`) is a clip-bounded bag of typed batches:

```rust
struct Layer {
    bounds: Rectangle,
    quads: quad::Batch,        // rounded-rect instances
    triangles: triangle::Batch,// arbitrary meshes
    primitives: primitive::Batch, // user shader primitives
    images: image::Batch,
    text: text::Batch,
    pending_meshes: Vec<Mesh>, pending_text: Vec<Text>,
}
```

Widget `draw()` calls trait methods like `Renderer::fill_quad`, `fill_text`, `fill_paragraph`, `draw_image`, `draw_mesh`, which append to `layers.current_mut()`. Clip stack: `start_layer(bounds)` pushes a new sub-layer; `end_layer()` pops. Transformations are a parallel stack (`start_transformation` / `end_transformation`) applied at append time, not during render — primitives are stored already in surface space.

Frame: `Renderer::draw` calls `prepare()` then `render()`. `prepare` iterates layers and calls each pipeline's prepare (uploads instance buffers via `wgpu::util::StagingBelt`). `render` opens one `RenderPass` and per-layer sets `scissor_rect` to the layer bounds then dispatches: quads → triangles (which actually break and reopen the render pass since meshes use MSAA resolve) → primitives → images → text. Layer ordering across batches is preserved per-layer; `layer::Stack::merge` combines adjacent layers whose primitive-kind ranges don't overlap (`Layer::start`/`end` return type-indices 1..=5 — see `wgpu/src/layer.rs:306`) so a pure-quad layer and a pure-text layer can fuse into one. No `RenderBundle` use; relies on instanced draws and minimal pass count.

## 6. Quads — SDF rounded rect, instanced (`wgpu/src/quad.rs`, `shader/quad.wgsl`)

A single `Quad` instance struct encodes everything:

```rust
struct Quad { position, size, border_color, border_radius:[f32;4],
              border_width, shadow_color, shadow_offset, shadow_blur_radius, snap }
```

Per-corner radii. Background is split into solid vs gradient batches (`Kind::Solid|Gradient`) with an `order: Vec<(Kind, count)>` to interleave correctly. The fragment shader uses `rounded_box_sdf` (signed distance to a rounded box with per-corner radius selected by quadrant) to produce border, fill, and shadow with one primitive — borders and box shadows do not need separate draw calls. Two pipelines (`solid`, `gradient`) share the same quad geometry. Blending is `PREMULTIPLIED_ALPHA_BLENDING`. **Reusable wholesale by Palantir**: the `Quad` struct, `quad.wgsl`, and the per-batch instance buffer pattern can be lifted with minimal change.

## 7. Text — cryoglyph (glyphon fork) (`wgpu/src/text.rs`, `graphics/src/text/`)

`iced_wgpu` depends on `cryoglyph` (an internal fork of `glyphon`) which is cosmic-text + a glyph atlas + a wgpu text renderer. Measurement happens during `layout()` via `graphics::text::Paragraph` (`graphics/src/text/paragraph.rs`): widgets call `Paragraph::with_text(...)`, which shapes via `cosmic_text::Buffer` and exposes `min_bounds()`/`measure()` for the layout pass. The shaped paragraph is cached on the persistent `widget::Tree` state, so re-layout is cheap unless the string changes.

At paint time, `fill_paragraph` queues a `Text::Paragraph { paragraph: weak_handle, position, color, clip_bounds, ... }` in the layer's `pending_text` (downgraded `Weak` so the cache owns the buffer). `text::Storage` (`wgpu/src/text.rs`) holds per-`cache::Group` `cryoglyph::TextAtlas` instances; `prepare` calls `cryoglyph::TextRenderer::prepare` which rasterizes any missing glyphs into the atlas and builds vertex data; `render` issues one draw per text item (per scissor). For Palantir: glyphon plugs in directly — measure during the measure pass, store the shaped `Buffer` on the persistent state map keyed by `WidgetId`, queue a glyph-draw record during paint.

## 8. Events / hit-testing

`UserInterface::update` walks events through `root.as_widget_mut().update(state, &event, layout, cursor, renderer, shell, viewport)`. Each container forwards events to children whose `Layout::bounds()` contains the cursor — hit-testing is just bounds-containment on the `Node` tree, performed during the same recursive walk that uses up-to-date layout. There is no separate hit-test acceleration structure (no R-tree); this is fine because the tree is small and walked anyway. Captured input is signalled back through `Shell` (sets `event_status`, requests redraw, queues messages). For Palantir's "input lags one frame" plan: identical model, just hit-test against the stored last-frame `Rect`s in arena order.

## 9. What to copy, what to skip

**Copy nearly verbatim into Palantir's wgpu backend:**

- The `Quad` instance layout and `quad.wgsl` SDF (border + fill + shadow in one primitive) — directly maps to `Shape::RoundedRect` with `Stroke`.
- The `Layer { bounds, quads, text, ..., pending_*}` batching pattern — Palantir's paint pass should write into typed batches, not issue draw calls inline. One render pass per surface, one scissor per clip layer, instanced draws per batch.
- Glyphon (`cryoglyph` is just a fork) plumbing: `Paragraph` cached on persistent state, atlas in `Storage` keyed by font group, `prepare` uploads glyph deltas, `render` issues per-scissor draws. Measure during measure pass via `Buffer::set_size` + `min_bounds`.
- `wgpu::util::StagingBelt` for instance/uniform uploads — avoids per-frame buffer recreation.
- Premultiplied-alpha blending and the `pack(Color)` linear-RGB packing in `graphics::color`.

**Don't import:**

- `Limits` two-sided constraint and `compression` flag are nice but heavier than Palantir's `available: Size` + `Sizing` enum. Worth adding `min` only if min-size widgets become important.
- `Element`/dyn `Widget`-trait dispatch is idiomatic for Iced's user-extensible widget set; Palantir's closed `Shape` enum + arena-of-`Node`s is intentionally simpler. Keep it. The Iced `Widget` trait's `layout()`-returns-`Node` shape doesn't fit Palantir's split (where `Shape`s are recorded separately from layout), and trying to retrofit `Widget` would re-introduce the heap-tree allocation Palantir avoids.
- `layer::Stack::merge` adjacency rules are clever but premature; ship one layer per clip region first.
- `RenderBundle` — Iced doesn't use them either; only relevant once dirty-tracking lands.

**Net**: lift the wgpu backend (quad shader, layer/batch types, glyphon integration, staging belt) almost wholesale; keep the layout core (arena tree, linked-list children, measure/arrange split, `Sizing` enum) entirely Palantir's own.
