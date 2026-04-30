# lyon — reference notes for Palantir

lyon is a CPU path tessellator: arbitrary 2D vector paths (with Béziers, arcs, fills, strokes) in, indexed triangle meshes out, ready for any GPU pipeline. It is the de-facto Rust answer to "I have an SVG-style `<path d="...">` and need triangles." iced uses it for shapes and primitives; resvg uses it indirectly via tiny-skia comparisons; many one-off renderers fall back to it. This file pins what lyon does, how it costs, and exactly when Palantir would and would not pull it in.

All paths are under `tmp/lyon/crates/`.

## 1. FillTessellator: monotone decomposition via sweep

`FillTessellator` (`tessellation/src/fill.rs:523`) implements a Bentley-Ottmann-style sweep-line that simultaneously **decomposes the polygon into y-monotone pieces and triangulates them** in a single pass. The high-level pipeline is: flatten curves to line segments at `options.tolerance` (default 0.1, `lib.rs:478`) → push segments into an `EventQueue` of `(position, edge_data)` sorted by sweep direction (`tessellation/src/event_queue.rs:40`) → walk events, maintaining `ActiveEdges` (`fill.rs:200`, the edges currently crossing the sweep line, sorted left-to-right by their x at the current y) → at each event classify as `Start`, `End`, `Split`, `Merge`, `Left`, `Right` based on local edge topology and immediately emit triangles via `BasicMonotoneTessellator` (`tessellation/src/monotone.rs:9`), the standard stack-based monotone triangulator.

Winding is tracked on `ActiveEdge.winding: i16` (`fill.rs:157`). `WindingState::update` (`fill.rs:106`) walks edges left-to-right summing winding numbers; `FillRule::is_in(number)` decides which spans are inside (`EvenOdd` vs `NonZero`, `lib.rs:458`). Spans are the inside regions between active-edge pairs and each maintains its own `BasicMonotoneTessellator` so triangles are emitted progressively rather than after a full decomposition pass.

**Robust intersection handling.** The "hard" part of any sweep tessellator is detecting and resolving edge–edge intersections that weren't in the input. lyon does it: `FillOptions.handle_intersections` (default true, `lib.rs:473`) tells the scan to look for active-edge intersections below the current event and inject new events into the queue mid-sweep. There's a fast-path `assume_no_intersection` (`fill.rs:534`) that skips the check — strictly faster but UB-on-violation. The author (Nicolas Silva, `nical`) has written multiple posts on the precision pitfalls; in practice lyon snaps near-coincident events together (`compare_positions` in `fill.rs`, used pervasively) and uses `float_next_after` to nudge degenerate cases. Known historical issues live around near-tangent self-intersections and curves that flatten to coincident segments — the bug tracker is the source of truth, not a list to memorize.

## 2. StrokeTessellator: strip along the path

`StrokeTessellator` (`tessellation/src/stroke.rs:108`) is conceptually simpler: walk the flattened path emitting a triangle strip offset ±width/2 along the normal at each vertex. No sweep. Joins (`LineJoin::Miter | MiterClip | Round | Bevel`, `path/src/lib.rs:193`) and caps (`LineCap::Butt | Square | Round`, `path/src/lib.rs:170`) are emitted as extra geometry where adjacent segments meet or where a sub-path begins/ends. Miter limit (`StrokeOptions.miter_limit`, `lib.rs:346`, default 4.0) clips the miter to a bevel when the join angle gets too sharp — same semantics as SVG.

`StrokeOptions.variable_line_width: Option<AttributeIndex>` (`lib.rs:340`) supports tapering: the width at each endpoint comes from a custom per-vertex attribute, linearly interpolated along segments. This requires the `tessellate_with_ids` path (`stroke.rs:140`) so attributes can be looked up by `EndpointId`. The doc comment on `StrokeTessellator` (`stroke.rs:42-48`) is honest about the limitation: a self-intersecting path produces overlapping triangles and SVG-correct rendering of a semi-transparent self-intersecting stroke needs a separate pass — lyon does not solve it.

## 3. Path / PathBuilder API

`Path` (`path/src/path.rs:75`) is a flat `(Vec<Point>, Vec<Verb>)` representation; `PathSlice` is a borrowed view. The mutable builder is the `PathBuilder` trait (`path/src/builder.rs:451`) with two layers: `NoAttributes<B>` (`builder.rs:138`) for the common case (`begin / line_to / quadratic_bezier_to / cubic_bezier_to / end(close: bool) / close`, see `builder.rs:170-214`) and the raw trait if you need per-endpoint attributes for variable stroke width or gradient sampling. Sub-paths must be wrapped `begin/end`; the debug builder validator panics otherwise.

`PathEvent = Event<Point, Point>` (`path/src/events.rs:35`) is the iterator currency: `Begin { at } | Line { from, to } | Quadratic { from, ctrl, to } | Cubic { from, ctrl1, ctrl2, to } | End { last, first, close }`. Tessellators take `impl IntoIterator<Item = PathEvent>` (`fill.rs:578`, `stroke.rs:122`) — the path data structure is not load-bearing, you can stream events from anywhere. `BorderRadii` (`builder.rs:100`) and the `add_rounded_rectangle` convenience methods on `PathBuilder` give you the rounded-rect shape without manual Bézier math.

## 4. Output via `GeometryBuilder` trait → typed vertex types

The tessellators don't allocate the output; they push into a user trait. `GeometryBuilder` (`tessellation/src/geometry_builder.rs:210`) has `add_triangle(a, b, c: VertexId)` plus `begin_geometry / end_geometry / abort_geometry`. For fills you also implement `FillGeometryBuilder::add_fill_vertex(FillVertex) -> Result<VertexId>` (`geometry_builder.rs:237`); for strokes `StrokeGeometryBuilder::add_stroke_vertex(StrokeVertex)` (`:248`). `FillVertex` (`fill.rs:2159`) and `StrokeVertex` (`stroke.rs:2668`) carry `position()` plus `sources() -> VertexSourceIterator` and `interpolated_attributes()` so your constructor can reach back to per-endpoint user data and produce whatever vertex layout the GPU pipeline wants.

The provided `BuffersBuilder<'l, OutputVertex, OutputIndex, Ctor>` (`geometry_builder.rs:300`) wraps a `VertexBuffers<V, I>` (a `(Vec<V>, Vec<I>)`) and a `FillVertexConstructor<V>` / `StrokeVertexConstructor<V>` (`:393`/`:398`) that maps the tessellator-side vertex to the user vertex. `MaxIndex` (`:562`) protects against `u16` overflow with `TooManyVertices`. `simple_builder` (`:` — emits `Point` vertices) is the toy default. This decoupling is genuinely good design: tessellation algorithm, geometry storage, and vertex layout are three orthogonal axes.

## 5. Performance characteristics

Cost is dominated by the event queue and active-edge maintenance, not triangle emission. For a path of `n` flattened segments the fill is roughly `O((n + k) log n)` where `k` is the number of intersection events introduced during the sweep; for a stroke it is plain `O(n)`. Curve flattening is set by `tolerance` (`FillOptions::DEFAULT_TOLERANCE = 0.1`, `lib.rs:478`); halving tolerance roughly doubles segment count for a given curve, so tessellating a glyph-style path at 0.01 vs 0.25 is a factor of ~5x in both time and output triangles.

Concrete budget: a tessellator instance is reusable and reuses internal `Vec`s across calls (`FillTessellator` keeps `events`, `scan`, `active`, `attrib_buffer` as fields, `fill.rs:524-538`); the `tessellate(&path, &options, &mut builder)` call is allocation-light after the first frame. Lyon publishes microbenchmarks in `tmp/lyon/bench/`; a reasonable rule is **single-digit microseconds for icon-sized paths, hundreds of microseconds for full glyph runs, milliseconds for "tessellate this entire SVG every frame"**. It is not free at UI volumes if you re-tessellate the world per frame — caching is mandatory.

## 6. Limitations

- **Anti-aliasing is the caller's problem.** lyon emits opaque triangles; smoothing the boundary is up to whoever consumes the mesh. The two real options downstream are (a) MSAA on the swapchain, (b) an extra triangle ring along the boundary with a "distance" varying for analytical AA, which lyon does **not** generate. iced bolts on its own AA strategy; egui sidesteps lyon entirely and pre-tessellates with feathered edges in `epaint`.
- **No GPU compute path.** All work is on the CPU; the output is just buffers. This is by design and is the principal contrast with Vello (which moves the entire pipeline to wgpu compute, see `references/vello.md`) and pathfinder.
- **Self-intersecting strokes overlap.** `stroke.rs:42-48` documents this. SVG-correct semi-transparent self-overlapping strokes are out of scope.
- **Numerical precision under heavy intersection.** The sweep is robust in the common case but pathological geometry (near-tangent curves, many edges within `compare_positions` epsilon) can still misbehave; the `LYON_FORCE_LOGGING` env var (`fill.rs:551`) exists precisely because debugging these is painful.
- **No fonts, no GPU-side curves.** lyon does Béziers via flattening only; there is no Loop-Blinn-style on-GPU curve evaluation. If a glyph atlas already exists, lyon adds nothing for text.

## 7. Lessons for Palantir

**When lyon is the right answer:** an honest-to-goodness path. Vector icons supplied as SVG. User-drawn ink. A `Canvas` widget that exposes Bézier primitives. A waveform / chart polyline that isn't worth a custom shader. For these cases lyon is a one-shot dependency that produces buffers Palantir's wgpu paint pass can hand to a generic `position + color` pipeline, and the tessellation cache keys on the (`Path`, `transform`, `tolerance`) tuple so you only pay on change.

**When lyon is the wrong answer — and this is most of a UI:**

- **Rounded rectangles.** Palantir's `Shape::RoundedRect` already targets an instanced SDF quad: one quad per rect, four corners' AA falls out of `length(p) - r` in a fragment shader. Tessellating a rounded rect into ~24 triangles per corner just to lose AA and pay CPU per frame is a strict regression. Do not route `RoundedRect` through lyon.
- **Lines / strokes between two points.** A 1-quad axis-aligned rect with an optional rounded-cap SDF beats `StrokeTessellator` for the common 1px-to-4px UI line; lyon's stroke tessellator is built for SVG-shaped paths with joins, not `Shape::Line`.
- **Text.** Glyphs go through glyphon's atlas; never tessellate text outlines. The "Status" board in `CLAUDE.md` already commits to this.
- **Backgrounds, frames, dividers, separators.** Same SDF/instanced-quad story.

**The shape of the decision.** Carrying lyon adds three crates (`lyon_geom`, `lyon_path`, `lyon_tessellation`) and a few hundred KB to compiled size; it does not infect the rest of the renderer because its output is just `(Vec<V>, Vec<u32>)`. So the cost is small. The benefit is small too — only paid when an actual path widget exists. The right policy is therefore: **don't pre-emptively add lyon; add it the first frame Palantir gains a `Path` shape variant, and keep `RoundedRect`, `Line`, `Text` on their dedicated SDF / atlas paths forever.** Keep the `Shape` enum decoupled from the tessellator (paint-pass dispatches per variant) so a `Shape::Path { mesh: TessellatedPath }` can be added as a leaf without touching layout or any other shape code, exactly the discipline `CLAUDE.md` already pins down.

**One copyable design point regardless of dependency choice:** lyon's `GeometryBuilder` trait — algorithm pushes vertices/triangles into a user-owned, vertex-typed sink — is the same shape Palantir's paint pass should expose. Whatever produces the per-frame draw lists (rounded-rect SDF instancer, glyph quad batcher, eventual lyon path adapter) all want a single `&mut dyn DrawSink<Vertex = ...>` to write into. Stealing that interface costs nothing and decouples shape-emission from buffer-management cleanly.
