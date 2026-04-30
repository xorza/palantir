# egui — reference notes for Palantir

egui is the closest existing prior art for an immediate-mode Rust GUI. Unlike Palantir, it does **not** record a tree first; user code paints directly into a `PaintList` while a cursor-based `Placer` advances. Two-pass layout is bolted on as an opt-in "sizing pass" plus the `Context::request_discard` multi-pass loop. This file pins down exactly how that works and what to copy/avoid.

All paths are under `tmp/egui/crates/`.

## 1. Single-pass-with-prior-frame-cache

The core primitive is `Ui::allocate_space(desired: Vec2) -> (Id, Rect)` in `egui/src/ui.rs:1185`. It does no measuring — the caller supplies a `desired_size` and `Placer::next_space` (`egui/src/placer.rs:113` → `Layout::next_frame` in `layout.rs:499`) hands back the next slot from a moving `Region { min_rect, max_rect, cursor }`. The widget then `painter.add(shape)` directly. There is no "measure phase".

Widgets that need a child's size before placing themselves cheat in two ways:

- **Read last frame's rect.** `Context` keeps `viewport.prev_pass.widgets: WidgetRects` (`context.rs:216`, swapped with `this_pass` at `context.rs:2589`). `Ui::ctx().read_response(id)` and `Response.rect` fall back to `prev_pass.widgets.get(id)` (`context.rs:1298`, `response.rs:163`). Containers like `ScrollArea` and `Window` size themselves from this.
- **Reserve a `ShapeIdx` and back-patch.** `PaintList::add(rect, Shape::Noop)` returns a `ShapeIdx` (`layers.rs:108`); the widget paints children, then `PaintList::set(idx, frame_shape)` overwrites the placeholder with the now-known frame. This is how `Frame` and `Button` paint a background sized to inner content (`layers.rs:141-156`).

**First-frame jitter.** A `Grid`, `ScrollArea`, or any widget that needs prior-frame data has nothing to read on frame 0. egui's answer is `UiBuilder::sizing_pass()` (`ui_builder.rs:140`): a Ui where `cross_justify` is forced off and centered alignment falls back to `Min` (`ui.rs:235-242`), so widgets shrink to intrinsic. The `is_sizing_pass()` flag (`ui.rs:319`) leaks into widgets — e.g. `widgets/separator.rs:111` returns near-zero size when set. `Grid::show` (`grid.rs:439-459`) demonstrates the full pattern: load prior `State`; if absent, call `ui.request_discard("new Grid")` and run inside a `sizing_pass().invisible()` builder.

`Context::request_discard(reason)` (`context.rs:1869`) marks the current pass to be thrown away; `will_discard()` checks that `num_completed_passes + 1 < max_passes` (default 2, see `context.rs:1888`). The runner loop in `Context::run` reruns the user closure up to `max_passes` times (`context.rs:836-861`). It emits a perf warning if discard fires multiple frames in a row (`context.rs:2489-2495`).

## 2. `Id` system

`Id` (`id.rs:34`) is a `NonZeroU64` aHash digest. `Id::new(x)` hashes any `Hash` source; `Id::with(child)` mixes parent hash + child key. Niche-optimized so `Option<Id>` is 8 bytes. `IdMap<V>` is `nohash_hasher::IntMap` keyed on the pre-hashed value (`id.rs:139`).

`Ui` carries two ids: `id` (stable: parent's id `with` a user/positional salt) and `unique_id` (stable id `with` `next_auto_id_salt`, which is a counter incremented on every allocation; `ui.rs:245-255`). Anonymous widgets get the per-call counter id from `allocate_space` (`ui.rs:1228`); stateful widgets must take an `Id` from the user so it survives reordering — same idea as Palantir's `WidgetId`.

## 3. Memory: persistent state

`Memory::data: IdTypeMap` (`memory/mod.rs:47`) is the global `Id → Any` store. Lookup is `(Id, TypeId) → Box<dyn Any>`; values clone-on-read (the doc explicitly says wrap large state in `Arc<Mutex<…>>`). This is exactly Palantir's planned persistent state map. It survives across passes/frames, can be `serde`-persisted (`persistence` feature). `Memory::caches: CacheStorage` is a separate per-frame computation cache evicted by frame age.

State lives in `Context` (single `Arc<RwLock<ContextImpl>>`, `context.rs`). The transient per-pass state is on `viewport.this_pass: PassState` and is rotated to `prev_pass` at end of pass.

## 4. Layout: `Layout` + `Placer`, no measure pass

`Layout` (`layout.rs:102`) is `{ main_dir: Direction, main_wrap, main_align, main_justify, cross_align, cross_justify }`. It has zero measure logic — it's a *cursor-walking strategy*. `Placer` (`placer.rs:7`) wraps `{ layout, region: Region, grid: Option<GridLayout> }`. `Region.cursor` is a `Rect` whose finite side is "where the next widget starts" and whose infinite side is "the rest is free" (see `layout.rs:582-`).

`horizontal`/`vertical` are just `with_layout(Layout::left_to_right(...))` etc. The "decision" for child position is purely `Layout::next_frame(region, child_size, spacing)` — given desired size, advance cursor along main axis, justify on cross. Wrapping is a special case in the same function (`layout.rs:506-580`). Children are placed in declaration order at the size they ask for; there is no fairness/distribution.

`Grid` is the exception — it's the one container that does a real two-pass behavior. `GridLayout` stores `prev_state: State` of column widths from last frame (`grid.rs:440`), uses those for placement this frame, and saves `curr_state` at the end. First frame: the discard + sizing pass described above.

There is **no `Sizing::Fill`-style space distribution** in core egui. `cross_justify` stretches to fill cross-axis but doesn't share leftover space among siblings. That whole concept (WPF `*`, flex `grow`) is absent — which is why `egui_taffy` exists as an external crate (it intercepts `allocate_space` and runs Taffy on a tree it builds itself, then plays the result back into egui rects).

## 5. Painting model

`Shape` (`epaint/src/shapes/shape.rs:27`) is the leaf enum: `Rect`, `Circle`, `Path`, `Text(Arc<Galley>)`, `Mesh`, `Callback`, `Noop`, etc. All shapes carry **screen-space** coordinates — they are not relative to any owner. `Painter` (`egui/src/painter.rs:21`) wraps a `LayerId` and pushes into the appropriate `PaintList` via `Context::graphics_mut` (`context.rs:977`).

`PaintList(Vec<ClippedShape>)` (`layers.rs:113`) is per-layer, append-only with the `set`/`mutate_shape` back-patch escape hatch. `GraphicLayers([IdMap<PaintList>; Order::COUNT])` groups by `Order` (Background/Middle/Foreground/Tooltip/Debug). At end of pass, `GraphicLayers::drain` (`layers.rs:213`) flattens in `area_order` (z-order tracked per-frame) into `Vec<ClippedShape>`. `Context::tessellate` (`context.rs:2728`) then runs `epaint::Tessellator::tessellate_shapes` to produce `Vec<ClippedPrimitive>` of CPU-built `Mesh { vertices: Vec<Vertex>, indices: Vec<u32>, texture_id }`.

`egui-wgpu/src/renderer.rs` consumes those primitives. It uploads vertex/index buffers, manages a `texture_id → BindGroup` map for the font atlas + user textures, and runs a single `egui.wgsl` pipeline. Custom rendering goes through `Callback` (`renderer.rs:28`) with a three-phase `prepare`/`finish_prepare`/`paint` lifecycle so user wgpu work batches alongside.

Notable: the shapes go through CPU tessellation every frame. There is no instancing of rounded-rects — every quad is real triangles in a vertex buffer. This is fine at egui's volumes but is exactly what Palantir avoids by emitting typed batches (rounded-rect SDF, glyph quads) directly.

## 6. Input: hit-test lags one frame

`hit_test::hit_test` (`hit_test.rs:42`) runs against `WidgetRects` from **the previous pass** (`context.rs:469-488`, using `viewport.prev_pass.widgets`). The current pass registers each widget's rect via `Context::create_widget` while the user code runs (`ui.rs:291`); those rects are only consulted next frame. `Response.clicked()` etc. are computed at `interact()` time from the prior-frame hit result + this-frame input events. This is the standard immediate-mode trick and what Palantir's `DESIGN.md §5` already commits to.

## 7. Multi-pass and intrinsic-size queries

- `Context::request_discard(reason)` + `max_passes` (default 2) is the whole multi-pass mechanism. `Context::run` loops the user closure.
- `Response::intrinsic_size_or_nan: Vec2` (`response.rs:71`, set at `ui.rs:1139` via `set_intrinsic_size`) lets a parent read the size a child *asked for* (pre-justify) so it can do its own layout math.
- `egui_taffy` (external) builds a Taffy node tree during user code, calls Taffy's solver, then `allocate_rect`s the resulting positions. It uses egui's measurement of intrinsic sizes (text via `WidgetText::into_galley`, `widget_text.rs`) as `MeasureFunc`. This is essentially Palantir-style record-then-layout, retrofitted as a layer on top of egui.

## 8. Lessons for Palantir

**Copy:**
- `Id` design: 64-bit aHash, `IdMap = nohash IntMap`, `Id::with` for hierarchical mixing, niche-optimized. Cheap, fast, zero-alloc.
- `IdTypeMap` for persistent state — one map keyed by `(Id, TypeId)`, clone-on-read, optional serde. Matches `DESIGN.md §4` exactly.
- `Response::intrinsic_size` as part of the public widget contract — useful even with two-pass, for "ask child what it would prefer" without forcing a re-measure.
- The `Order`-then-`area_order` drain pattern in `GraphicLayers` for z-ordering popups/tooltips — applies to Palantir's paint pass.
- `request_discard`-style discard-and-rerun escape hatch for the rare case where measure-arrange's one-shot model is genuinely insufficient (e.g. content that resizes based on its own arrangement).

**Avoid (this is what recording-first buys us):**
- The `sizing_pass` flag plumbed through every widget. egui needs it because widgets paint during recording; if you record then measure, "use small intrinsic size" is just the natural measure result on `Sizing::Hug`.
- The `prev_pass.widgets` fallback for sizing. With a tree, every node sees a real `Measure(available)` result *this* frame. No first-frame jitter, no `request_discard` for normal layouts, no Grid special case.
- Back-patching `ShapeIdx` placeholders. Palantir's `ShapeRect::Full` sentinel + paint-pass resolution against owner `Rect` solves the same "I want a frame around content I haven't placed yet" problem without mutation of the paint list.
- Cursor-only layout. egui has no concept of `Sizing::Fill` distribution; users hack around it with `with_layout` tricks. Palantir's WPF `Fixed/Hug/Fill` lets `HStack`/`VStack` do honest leftover-space sharing in a real arrange pass.
- CPU tessellation of every rounded-rect into triangles. Palantir can render rounded-rects as instanced SDF quads and only fall back to tessellation for paths. egui's choice is a consequence of the `Shape` enum being shared with non-wgpu backends (`egui_glow`, software).
- `egui_taffy`'s existence is a tell: serious users want a real layout engine and have to bolt one on. Palantir bakes that in.

**Open question worth resolving:** egui's `Frame`/`Button` paint background then content; the paint order matches the immediate-mode `add(Shape)` order. Palantir's paint pass walks the tree pre-order and emits each node's shapes in slice order. Confirm that "background shape declared on parent before recursing into children" is the recorded contract — otherwise `Frame` semantics break. (Cross-check `src/ui.rs::container` and `src/widgets/button.rs` once Frame lands.)
