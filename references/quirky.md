# quirky — reference notes for Palantir

quirky is a small experimental wgpu UI library. The README admits it's a "sketch" — but its renderer is the closest small wgpu IM-ish library to what Palantir's paint pass needs: instanced quads keyed by primitive type, per-primitive WGSL shader, glyphon for text, a pipeline cache keyed by UUID. Architecturally it's *retained* (Arc-of-trait-object widgets, signal-driven), but the layer between widget and GPU is exactly the shape we want.

All paths are under `tmp/quirky/crates/lib/`.

## 1. Architecture

A widget is `Arc<dyn Widget>` (`quirky/src/widget.rs:41`) with five concerns smashed together: `WidgetBase` (id/bbox/dirty/cached primitives, `widget.rs:21`), an async `run` task driven by `futures_signals`, a `prepare` returning `Vec<Box<dyn DrawablePrimitive>>` (`widget.rs:53`), a `size_constraint` signal (`widget.rs:61`, just `MinSize/MaxSize/MaxWidth/MaxHeight/Unconstrained`), and `get_widget_at` for hit-testing (`widget.rs:65`). Each widget carries a per-instance `Uuid` as its identity.

`QuirkyApp` (`quirky/src/lib.rs:46`) owns the wgpu device/queue, a single root widget, a camera UBO + bind group, and two UUID-keyed caches: `pipeline_cache: HashMap<Uuid, RenderPipeline>` and `bind_group_cache: HashMap<Uuid, BindGroup>`. The frame loop is `draw` (`lib.rs:149`):

1. Walk the widget tree pre-order via `next_drawable_list` (`lib.rs:275`). For each widget: if `dirty`, call `prepare(ctx, paint_ctx)` to mint a fresh `Vec<Box<dyn DrawablePrimitive>>` and clear the dirty flag; otherwise reuse `get_cached_primitives()`.
2. Push `(uuid, primitives, widget)` onto a `VecDeque`.
3. Second pass over the deque: call `DrawablePrimitive::prepare` on each primitive (uploads instance buffer, lazily inserts pipeline into the cache).
4. Open one `RenderPass`, call `DrawablePrimitive::draw` on every primitive in tree order.
5. Write cached primitives back onto each widget.

There's no batching across widgets — every `prepare` returns its own `Vec`, every primitive owns its own buffers, and the draw pass switches pipeline + binds + buffers per primitive (`primitives/quad.rs:112`, `primitives/border_box.rs:124`, `primitives/button_primitive.rs:116`). For 100 buttons that's 100 pipeline binds even though they all use the same shader.

Layout is *not* a tree pass. Each container widget independently runs a layout future (`quirky/src/widgets/layout_helper.rs:7`) that consumes a `Signal<LayoutBox>` for its own bbox plus a `SignalVec` of child `SizeConstraint`s, runs a strategy fn, and pushes new bboxes onto children via `set_bounding_box` (which is itself a `Mutable`, so children re-layout reactively). See `BoxLayout::run` (`quirky-widgets/src/layouts/box_layout.rs:93`) for the canonical wiring.

## 2. The wgpu pipeline pattern

Every primitive (quad, border_box, button, image, text) follows the same template, copied verbatim across files. The struct has three buffers — `vertex_buffer` (the unit quad, shared layout `[(0,0),(1,0),(1,1),(0,1)]` from `primitives/vertex.rs:10`), `index_buffer` (`[0,1,2,0,2,3]`, `vertex.rs:29`), `instance_buffer` — plus a `ReadOnlyMutable` of the instance data:

```rust
#[repr(C)]
#[derive(VertexLayout, bytemuck::Pod, bytemuck::Zeroable, Copy, Clone)]
#[layout(Instance)]
pub struct Quad { pub pos: [f32;2], pub size: [f32;2], pub color: [f32;4] }
```

(`primitives/quad.rs:18`). The `wgpu_macros::VertexLayout` proc macro emits `Vertex::LAYOUT`, but the instance layout is hand-written every time (`quad.rs:33`, `border_box.rs:29`, `button_primitive.rs:36`) — the macro doesn't seem to handle instance step mode here. Vertex pulls `@location(0,1)`, instance pulls `@location(2..)`.

`prepare` does two things: lazy-init the pipeline keyed by a hardcoded `const PIPELINE_ID: Uuid` per primitive type (`quad.rs:13`, `button_primitive.rs:14`), and `queue.write_buffer(&self.instance_buffer, 0, ...)` to push fresh instance data. `draw` is six lines: `set_pipeline`, `set_bind_group(0, camera, &[])`, `set_vertex_buffer(0, vert)`, `set_vertex_buffer(1, inst)`, `set_index_buffer`, `draw_indexed(0..6, 0, 0..N)`.

The interesting one is `Quads` (`primitives/quad.rs:58`) — it actually batches: instance data is `ReadOnlyMutable<Arc<[Quad]>>`, draw count is `geometry.lock_ref().len()`. So a single `Quads` primitive with N instances issues one draw call. But `BorderBox` and `ButtonPrimitive` only ever hold a single instance (`borders.rs:92`, `button_primitive.rs:84` — `&[data.get()]`, `0..1`). Because each *widget* mints its own primitive, the batching benefit is lost the moment you have more than one button. There is no cross-widget instance accumulator.

The single shared `camera_bind_group` (group 0) carries a 4x4 transform built from `UiCamera2D` (`quirky/src/ui_camera.rs`). Vertex shaders multiply pixel-space `(pos + vert*size)` by `camera.transform` — pixels-to-clip is a CPU-built matrix, not a viewport push-constant. That's a missed simplification; egui-wgpu uses a `vec2(screen_size)` uniform and divides in the shader.

## 3. "Rounded-rect SDF" — except it isn't

Quirky has *no* real SDF rounded-rect. The closest thing is `button.wgsl`:

```wgsl
let centered = in.quad_pos - vec2<f32>(0.5, 0.5);
let cx = pow(abs(centered.x) * 2.0, 2.0);
let cy = pow(abs(centered.y) * 2.0, 2.0);
let distance = max(cx, cy);
let factor = 1.0 - max(distance - 0.6, 0.0);
return vec4<f32>(r*factor, g*factor, b*factor, 1.0);
```

(`primitives/shaders/button.wgsl:39-51`). That's a vignette inside an axis-aligned quad — fade to black past the 0.6 iso-contour of `max(|x|², |y|²)`. No corner radius parameter, no anti-aliased edge, no border. The button looks "rounded" only because the dark vignette hides the corners against a dark background. `quad_pos` is the **unit-square** vertex position (`button.wgsl:33`), not pixels, so the falloff scales with the button's aspect ratio — wide buttons get squashed vignettes.

`border_box.wgsl` is a 1px outline test in a similar style (`shaders/border_box.wgsl:53-58`): hardcoded `border_thickness = 1.0`, no anti-aliasing, alpha is `select(0.0, 1.0, in_border)` (binary). The `borders: vec4<u32>` and `border_side: u32` instance fields are passed but unused in the fragment shader.

For Palantir's purposes: **quirky's shaders are not a reference for SDF rounded-rects.** They are a reference for the *plumbing* (instance struct, vertex layout, bind group) wrapped around shaders we have to write ourselves. The actual SDF math should come from iced's `quad.wgsl` or vello/lyon — quirky doesn't have it.

## 4. Text path

Plain glyphon. `FontResource { font_system, font_cache: SwashCache, text_atlas: TextAtlas }` is a single resource stuffed into `QuirkyResources` (`quirky-widgets/src/resources/font_resource.rs:1`). `TextLayout::prepare` (`quirky-widgets/src/widgets/text_layout.rs:44`):

1. Take/init a per-widget `Buffer` (`Mutex<Option<glyphon::Buffer>>` field).
2. `set_size(bb.size)`, `set_text(text, Family::SansSerif, Shaping::Advanced)`, `shape_until_scroll`.
3. `TextRenderer::new(&mut text_atlas, &device, ...)` — **a fresh `TextRenderer` every prepare**.
4. `renderer.prepare(...)` with a single `TextArea { buffer, left/top, bounds, default_color }`.
5. Box it as `TextRendererPrimitive(TextRenderer)` (`primitives/text.rs:7`).
6. At draw time: `self.0.render(&font_resource.text_atlas, pass)` (`primitives/text.rs:13`).

The shared atlas is the only thing that batches — every other text widget makes its own renderer. There's no glyph caching across frames beyond what glyphon does internally. Font metrics are hardcoded `Metrics { font_size: 15.0, line_height: 17.0 }` (`text_layout.rs:75`), no DPI scaling, no font weight/style props.

`size_constraint` for `TextLayout` returns `MinSize(uvec2(10, 10))` always (`text_layout.rs:138`) — text never measures itself for layout. The container hands it a box, glyphon clips. This is the biggest layout limitation: there's no way for text width to drive a `Hug` container.

## 5. Layout / widgets

Two layout primitives: `BoxLayout` (h/v stack, `layouts/box_layout.rs`) and `AnchoredContainer` (single child positioned at an `AnchorPoint`, used by `Button` for centering content). Plus `Stack` (all children share parent bbox, `widgets/stack.rs:51`).

`box_layout_strategy` (`layouts/box_layout.rs:190`) is the only real distribution code:
- Sum `MinSize` along main axis as `min_requirements`.
- `remaining = container_size - min_requirements`, split equally as `per_remaining_bonus = remaining / total_items`.
- Each child gets `base + per_remaining_bonus`, clamped by `MaxWidth/MaxHeight` (overflow recycled into the bonus for later children).

So a `MinSize` child grows past its minimum if there's slack — a single `SizeConstraint::MinSize` plays both WPF `Auto` (the floor) and `Fill` (the growth) roles. There's no equivalent of `Fixed`-must-not-grow. Cross axis is always `container.size` (full stretch). This is a thinner contract than Palantir's `Sizing::{Fixed, Hug, Fill}` and conflates intrinsic-min with grow-weight.

The `#[widget]` proc macro (`quirky-macros`) generates a `FooBuilder` plus a generic `Foo<Sig1, Fn1, ...>` parameterised by every signal type. The `async fn run` body is what the user writes by hand — it stitches together prop-poll futures, layout futures, child-run futures, and event-subscription futures into a `FuturesUnordered` and selects forever (`button.rs:102`, `box_layout.rs:93`). It's very verbose and the per-widget async machinery is the dominant complexity in the codebase.

Hit-testing is recursive and uses *current* bboxes (no prior-frame trick): `get_widget_at` walks children in reverse, returns the first containing path (`box_layout.rs:74`, `button.rs:91`). Events dispatch to a target Uuid via `QuirkyAppContext::dispatch_event`, picked up by the widget's own subscription stream (`run_subscribe_to_events` in `widgets/event_subscribe.rs`).

## 6. Lessons for Palantir

**Copy verbatim:**
- The per-primitive instance-struct + WGSL shader template. `#[repr(C)] + bytemuck::Pod + Zeroable` instance type, one `wgpu::VertexBufferLayout`, a const `Uuid` for the pipeline, vertex pulls 0/1, instance pulls 2+. This is exactly Palantir's renderer shape: one pipeline per shape kind (rounded-rect, glyph, line, image), instance buffer per kind, draw_indexed once per kind.
- The shared unit-quad vertex/index buffers in a renderer-wide module — no need to re-upload per primitive.
- `pipeline_cache: HashMap<Key, RenderPipeline>` lazily initialised on first use in `prepare`. Keyed by the Palantir equivalent of "primitive kind" (an enum discriminant beats Uuid).
- glyphon `FontSystem + SwashCache + TextAtlas` as a single shared resource, with one `TextRenderer` for the *whole frame* (not per-widget — see below).
- Two-phase `prepare` → `draw`: the prepare phase has `&mut Device, &mut Queue, &mut Cache`; the draw phase only has `&'a` references. This split lets the borrow checker compile cleanly with one big render pass.

**Avoid:**
- One `DrawablePrimitive` per widget with its own buffers. This is what kills quirky's batching: 100 buttons → 100 `ButtonPrimitive`s → 100 single-instance draws. Palantir's paint pass should accumulate **one instance buffer per shape kind for the whole frame**, indexed by the shape enum, drained once per frame as `draw_indexed(.., 0, 0..N)`. The recording-first design makes this trivial — `Tree.shapes` is already flat; bucket by variant and you're done.
- The button "vignette" shader. Write a real SDF rounded-rect: signed distance to a rounded-rect, smoothstep AA at the edge, optional border in the same fragment. iced's `wgpu/src/shader/quad.wgsl` is the reference. Pass corner radius and border params per-instance.
- glyphon `TextRenderer::new` per widget per frame. Use one `TextRenderer` for the frame and prepare *all* `TextArea`s in one call — that's how glyphon is designed. quirky misses this and pays for it on every text widget.
- `MinSize` overloaded as both intrinsic and grow-weight. Palantir's `Fixed/Hug/Fill` is the right split; don't regress to a single constraint enum.
- The whole `futures_signals` retained-reactive widget machinery. quirky's per-widget `async fn run` selecting on prop signals + bbox signal + child layout signals is a *consequence* of being retained — every cell mutation has to propagate. A frame-rebuilt arena tree (Palantir) eliminates all of this: no `Mutable<LayoutBox>`, no `signal_vec`, no `Arc<dyn Widget>`, no async runtime.
- `Uuid::new_v4()` per widget for identity. Palantir's hashed `WidgetId` is cheaper, deterministic, and survives reordering.
- The ad-hoc `set_bounding_box` mutation in `BoxLayout::run` writing back into children's `Mutable<LayoutBox>`. With a real arrange pass, parents assign child rects in a single recursive walk — no signal plumbing.

**Open question:** quirky's `camera_bind_group` (group 0, a 4x4 ortho matrix in pixel space) is applied uniformly to every pipeline. Palantir could do the same (one global UBO with `screen_size + scale_factor`), or push a `vec4` constant per-pipeline call. Push constants are smaller and avoid the bind-group dance, but require a feature flag. The shared-UBO route is what quirky proves works on stock wgpu — start there.
