# Makepad — reference notes for Palantir

Source: `tmp/makepad/` (Rik Arends, 2024–2026). Three top-level crates matter here: `platform/` (windowing, GPU abstraction, shader compiler, live-reload), `draw/` (2D draw context, turtle layout, primitive shaders, text), `widgets/` (button, view, dock, etc.). Studio (`studio/`) is the live IDE that drives hot reload over a JSON-line bridge (see `tmp/makepad/AGENTS.md`).

## 1. Architecture and the DSL

Every widget is described twice: as a Rust `#[derive(Script, ScriptHook)] #[repr(C)] struct` whose `#[live]` fields lay out the GPU instance, and as a block in the Makepad DSL. Historically this was the `live_design!{ ... }` macro (still present in `draw/src/lib.rs`, `platform/src/app_main.rs`); the current tree has migrated most code to `script_mod! { ... }` blocks evaluated at runtime by `platform/script/` (an embedded VM — see `platform/script/src/{lib,opcodes,native,mod_shader}.rs`). Either way, the DSL is the single source of truth for theme tokens, default field values, and shader source. Rust just defines the struct/ABI; the DSL fills in styling and `pixel:`/`vertex:` GLSL-like functions. `widgets/src/button.rs` lines 12–200 show the pattern: `ButtonFlat = ... do mod.widgets.ButtonBase{ draw_bg +: { border_radius: uniform(...), pixel: fn(){ let sdf = Sdf2d.viewport(...); ... } } }`.

## 2. Shader-per-primitive

`draw/src/shader/` defines one Rust struct per draw kind: `DrawQuad`, `DrawColor` (extends DrawQuad), `DrawText`/`DrawTextSlug`, `DrawGlyph`, `DrawSvg`, `DrawCube`, `DrawPbr`, `DrawVector`. Each holds `#[deref] draw_vars: DrawVars` plus `#[live]`/`#[instance]` fields like `rect_pos: Vec2f`, `rect_size: Vec2f`, `color: Vec4f`. `#[repr(C)]` means the struct *is* the per-instance vertex layout — `DrawVars::as_slice()` reinterprets it as `&[f32]` and pushes those floats straight into the draw call's instance buffer. Nothing copies field by field.

The DSL block declares uniforms, varyings, the geometry buffer (`geom: vertex_buffer(geom.QuadVertex, geom.QuadGeom)` — a unit quad), and `vertex`/`fragment`/`pixel` functions. `DrawQuad`'s vertex shader does `clip_and_transform_vertex(rect_pos, rect_size)` with view transform and clip rect; the user only writes `pixel: fn(){ ... }`. Subclasses override `pixel`. Rounded rect / shadow / blur live in `draw/src/shader/sdf.rs` as a `Sdf2d` library: `Sdf2d.viewport(self.pos * self.rect_size); sdf.box(...); sdf.fill(color)`. There is no monolithic shader — every visual primitive is a distinct shader program because each widget's `pixel:` body is unique.

## 3. Batching: `ManyInstances` and `add_aligned_instance`

`draw/src/draw_list_2d.rs` (lines 280–410) holds the batcher. A `DrawList` contains ordered `DrawItem`s; each `DrawItem` owns a draw-call uniform set plus a `Vec<f32>` instance buffer. `Cx2d::append_to_draw_call(draw_vars)` looks up the current draw list, then `find_appendable_drawcall(sh, draw_vars)` decides whether the new instance can extend the previous draw item (same shader id, same uniform values, no `draw_call_always` flag) or must start a new one. If extendable, the f32 slice is `extend_from_slice`'d in. `add_aligned_instance` does the same thing but also registers the area in `align_list` so post-layout alignment can shift it. `begin_many_instances`/`end_many_instances` are the bulk variant — take ownership of the buffer, append directly, hand it back. Net effect: identical-shape rounded-rect buttons collapse into one instanced draw call automatically, gated only by uniform equality.

## 4. The `Cx` context

`platform/src/cx.rs` holds the global `Cx`: `windows`, `passes`, `draw_lists` (`IdPool`), `textures`, `fonts`, `redraw_id`, the event queue, the live registry. `Cx2d<'a,'b>` (`draw/src/cx_2d.rs`) wraps `&mut Cx` with the 2D drawing state — turtle stack, draw-list stack, align list. Every widget method takes `cx: &mut Cx2d` and threads it through `begin`/`end` calls. There is no implicit thread-local context; the explicit `&mut Cx` is the moral equivalent of egui's `Context` but mutable and non-Clone.

## 5. Cross-platform rendering

`platform/src/os/`: `apple/metal.rs` (Metal), `windows/d3d11.rs` (DX11), `linux/{opengl,vulkan,vulkan_naga}.rs`, `linux/android`, `web/` (WebGL/WebGPU via JS shim). There is no wgpu — Makepad ships its own per-backend GPU layer because it needs a custom shader compiler (DSL → MSL/HLSL/GLSL/SPIR-V via `platform/src/draw_shader.rs`, ~1000 LOC, plus naga for Vulkan). `vulkan_naga.rs` is the closest to what wgpu does internally. The trade is more code in exchange for runtime shader compilation that matches the live-edit story.

## 6. Text

Entirely in-house, no cosmic-text/glyphon. `draw/src/text/`: `loader.rs` loads TTF/OTF, `shaper.rs` does HarfBuzz-style shaping, `rasterizer.rs` rasterizes via `sdfer.rs`/`msdfer.rs` (signed/multi-channel distance fields) into `font_atlas.rs` (a packed GPU texture). `slug_atlas.rs` adds curve-texture / band-texture rendering ("slug" algorithm — vector glyphs in shader, no resolution loss; see `DrawTextSlug` in `draw/src/shader/draw_text.rs`). `layouter.rs` produces `LaidoutText` → `LaidoutRow` → `LaidoutGlyph`s, which the `DrawText`/`DrawGlyph` shader consumes as instances.

## 7. Hot reload

`platform/src/live_reload.rs` (~1100 LOC) watches DSL files; on change it re-parses, diffs the live tree, and patches values into the running app via `LiveChange` events. Since shader source lives in the DSL, changing a `pixel: fn(){}` recompiles that shader at runtime and rebinds it — no Rust recompile. The Studio bridge (`AGENTS.md`) sends `LiveChange` over the wire so the IDE can scrub a color picker and watch the running app update.

## 8. Turtle layout

`draw/src/turtle.rs`. A turtle is a cursor positioned at the top-left of a region; widgets `walk()` it forward by their measured size. The two driving types:

```rust
pub struct Walk { abs_pos: Option<Vec2d>, margin: Inset, width: Size, height: Size, metrics: Metrics }
pub struct Layout { /* padding, flow (Right/Down/Overlay), spacing, align, scroll, clip */ }
```

`Size` is `Fixed(f64) | Fit | Fill | All`. `Walk` is what a child asks for; `Layout` is what a parent imposes on its children. Each widget implements `fn draw_walk(&mut self, cx: &mut Cx2d, scope, walk: Walk) -> DrawStep` (see `widgets/src/button.rs:589`) which calls `self.draw_bg.begin(cx, walk, layout)` — that pushes a turtle, draws children inside it, and `end()` pops the turtle, computes the final rect, and back-patches the bg instance's `rect_pos`/`rect_size`.

The trick that matters: `Fit` content can't be sized until children are drawn, so the bg instance is added with placeholder coordinates and *fixed up* when the turtle ends, via `align_list` entries that record the InstanceArea. There is no explicit Measure pass — instead Makepad does **single-pass deferred-placement**: emit instances eagerly, patch positions when the turtle closes. This works because a widget's drawn output is just N floats in an instance buffer, trivially mutable after the fact.

## 9. Events and animation

Events flow via `platform/src/event/event.rs` — `Event::FingerDown/Up/Move/Hover`, `KeyDown`, `Tick`, etc. Each widget implements `fn handle_event(&mut self, cx: &mut Cx, event: &Event, scope: &mut Scope)` (button line 494) which hit-tests against its stored `Area` (a handle into the `draw_lists` pool) and emits `Action`s into the scope. `widgets/src/animator.rs` is a state-machine animator driven by `Animate::on/off` triggers; the animator interpolates `#[live]` fields (e.g. `hover: f32`) into instance memory each `Tick`, which the next frame's draw picks up automatically because instances are just structs.

## 10. Lessons for Palantir

- **Per-primitive shader vs über-shader.** Makepad's per-primitive split is justified because the DSL lets users write arbitrary `pixel:` bodies — they *need* arbitrary shaders. We don't. For `Shape::RoundedRect | Text | Stroke`, an über-shader keyed by `prim_kind: u32` in the instance is simpler, batches everything into one draw, and avoids state thrash. Steal the *data layout* (`#[repr(C)]` instance struct → `&[f32]` → wgpu vertex buffer) but skip the per-shape program proliferation. Look at `Sdf2d` in `draw/src/shader/sdf.rs` — that body (rounded box, shadow, stroke) is exactly the math to port into a single fragment shader.
- **Instance buffer batching.** `find_appendable_drawcall` keying on `(shader_id, uniform_hash)` is the right gate. With one über-shader, only uniforms (clip, transform) split batches, so most frames collapse to one draw per draw-list/clip stack. Use a `Vec<f32>` plus `extend_from_slice` exactly like `draw_list_2d.rs` — bytemuck-cast the instance struct.
- **Turtle vs WPF measure/arrange.** Makepad's turtle is single-pass with back-patching, not two-pass. It works for them because (a) `Fit` parents back-patch their own bg rect after children draw, (b) `Fill` siblings can't compete the way WPF `*` columns do — Makepad's flow is more like CSS flexbox-with-greedy-layout than WPF Grid. **Keep Palantir's two-pass model.** The WPF contract (`available → desired` then `final → arrange`) handles `Fill` distribution, multi-`*` columns, and constrained re-measure cleanly; back-patching doesn't. The turtle is worth studying for its `Walk`/`Layout` *type split* — child-side request vs parent-side imposition is a clean API factoring we can mirror in `Sizing` + `Spacing`.
- **Don't write your own GPU layer.** Makepad does it for live shader recompile. We don't need that — wgpu is the right choice.
- **Text.** Makepad's slug/SDF stack is impressive but huge. Use glyphon/cosmic-text as planned; revisit only if vector zoom matters.
- **Hot reload.** Out of scope for v1. If we ever want it, `live_reload.rs` and the Studio bridge are the model — but it requires the DSL-as-source-of-truth split, which is a big architectural commitment.

Key files to revisit while implementing the renderer: `draw/src/shader/sdf.rs` (SDF math), `draw/src/shader/draw_quad.rs` (instance struct ↔ shader binding), `draw/src/draw_list_2d.rs:280–410` (batcher), `draw/src/text/font_atlas.rs` (atlas packing reference).
