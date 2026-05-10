# Mesh shapes (user-supplied colored triangle meshes)

**Status:** research / design. Motivated by workloads that need
non-rectangular geometry (charts, gauges, custom widgets, shape
primitives that aren't rounded-rects or text). Scope: v1 = colored
vertex meshes, no textures, no per-vertex UVs. Texturing is a
follow-up.

## Problem

`Shape` today is `RoundedRect | Line | Text`. A user who wants to
draw a triangle, polygon, gradient fill, ring, or arc has to either:

- approximate with rounded-rects (impossible past axis-aligned),
- ask the framework for new dedicated variants (every new visual is
  one PR + a shader change),
- or stay outside the framework (forfeit clip / damage / cache).

The right primitive is "here's a triangle list, render it." That's
what egui, iced, imgui, and nuklear all expose. The design question
is **storage**: where do the vertices and indices live, and how does
the shape reference them.

## Design constraints

From `CLAUDE.md` posture and the existing pipeline:

1. **Alloc-free in steady state.** No per-frame `Vec::new()`,
   `Box::new()`, `Arc::new()`. Push onto retained scratch.
2. **Shape stays small.** Shapes are pushed into `Tree.shapes:
   Vec<Shape>`, hashed every frame, walked by every pass. Bloating
   the variant penalizes the common (RoundedRect) path. `RoundedRect`
   is ~64 B today; new variants should match or beat that.
3. **Cache identity is content-addressed.** `MeasureCache` and the
   encode cache key on `(WidgetId, subtree_hash, available_q)`.
   Per-frame spans into a freshly-cleared buffer change every frame
   even when the data doesn't — so the shape variant must carry
   stable per-content identity (a hash), not just spans, into the
   per-node rollup.
4. **Cache snapshots own their data.** `MeasureCache` already does
   this for `text_shapes` via `LiveArena<ShapedText>`
   (`src/layout/cache/mod.rs`). On cache hit, the snapshot's payload
   is replayed into the current frame's flat buffer; spans are
   retargeted. Mesh has to plug into the same machinery.

## How other libraries do it

Researched `tmp/{egui,iced,imgui,nuklear,clay,vello,lyon}` —
takeaways:

| lib | shape side | vertex storage | why |
|---|---|---|---|
| **egui** | `Shape::Mesh(Arc<Mesh>)` | per-mesh `Vec<Vertex>` + `Vec<u32>` | shape enum capped at 64 B; `Mesh` (3 Vecs + tex) doesn't fit inline → `Arc` (`tmp/egui/crates/epaint/src/shapes/shape.rs:60`). Atomic refcount per shape per frame. |
| **iced** | `Mesh` collected into `Vec<Mesh>`; cached as `Arc<[Mesh]>` | per-mesh Vecs; renderer concatenates into one GPU buffer per layer at upload time, tracks offsets (`tmp/iced/wgpu/src/triangle.rs:396`) | retained / cache-versioned model; offsets computed at upload, not stored on shape. |
| **imgui / nuklear** | `ImDrawCmd { idx_offset, idx_count, ... }` references one big `ImDrawList { VtxBuffer, IdxBuffer }` (`tmp/imgui/imgui.h`) | flat per-frame draw-list, vertex-offset + index-range per cmd | exactly the "flat buffer + spans" model. C-era allocators, but the layout maps 1:1 to a wgpu indexed draw. |
| **clay / floem / xilem / makepad / slint / quirky** | no user-mesh primitive | n/a | rect+text frameworks; mesh isn't part of their surface. |
| **vello / lyon / kurbo** | upstream tessellators, not consumer APIs | output to user-supplied `VertexBuffers<V, I>` | irrelevant for storage; relevant if we ever add stroke→tris. |

**egui's Arc is forced by their shape-size cap, not chosen for
sharing.** They merge meshes with `Mesh::append_ref` before wrapping
once. **imgui/nuklear's flat-buffer + cmd-with-index-range is the
model that actually fits Palantir's "shape = small handle, content
in arena" philosophy.** Iced shows that batching N meshes into one
GPU buffer at upload time is straightforward.

Full notes are in the research thread; the design below is what
falls out.

## Two enums: public `Shape<'a>`, internal `ShapeRecord`

`ui.add_shape(Shape::*)` is already the canonical authoring path —
keep that. So `Shape` should be the **user-facing input type**, and
the **arena storage type** gets a different name. Existing
`Shape::Text` / `Shape::RoundedRect` / `Shape::Line` call sites
keep working unchanged; the new `Shape::Mesh { mesh: &m, ... }`
variant slots in next to them and reads naturally.

This is a **rename** of today's `Shape` enum (which is actually the
arena form: it lives in `Tree.shapes: Vec<Shape>`, gets walked by
every pipeline pass, and was always going to be incompatible with
holding `&Mesh` because of `Vec<Shape>`'s lifetime). Rename it to
`ShapeRecord` (parallels `NodeRecord`), and introduce a new public
`Shape<'a>` that the user constructs.

```text
                 user code                  │      framework
                                            │
   let mut m = Mesh::new();                 │
   m.vertex(pos, color); …                  │
   m.triangle(0, 1, 2);                     │
                                            │
   ui.add_shape(Shape::Mesh {               │  match arm copies
       mesh: &m,                            │  m.vertices / m.indices into
       local_rect: None,                    │  Tree.mesh_vertices / .mesh_indices,
       tint: Color::WHITE,                  │  hashes the bytes, pushes
   });  ───────────────────────────────────►│  ShapeRecord::Mesh { spans, hash }
                                            │  into Tree.shapes
```

The non-`Mesh` variants are byte-identical between `Shape` and
`ShapeRecord` (same fields), so `add_shape` is a thin match: three
arms pass through unchanged, one arm does the
copy-into-arena + hash. The user never types a span, never computes
a hash. They build a `Mesh`, they hand the framework
`Shape::Mesh { mesh: &m, ... }`.

Per CLAUDE.md "break things freely" posture: the project-wide
rename is a one-shot mechanical change (sd / clippy --fix). Cleaner
than wedging in a parallel `ShapeInput` and then explaining
forever why `add_shape` takes one type but the storage holds
another.

### Public `Shape<'a>` (what users construct)

```rust
// src/shape.rs (replaces today's `Shape` enum, with Mesh added)
pub enum Shape<'a> {
    RoundedRect {
        local_rect: Option<Rect>,
        radius: Corners,
        fill: Color,
        stroke: Stroke,
    },
    Line { a: Vec2, b: Vec2, width: f32, color: Color },
    Text {
        local_rect: Option<Rect>,
        text: Cow<'static, str>,
        color: Color,
        font_size_px: f32,
        line_height_px: f32,
        wrap: TextWrap,
        align: Align,
    },
    Mesh {
        mesh: &'a Mesh,
        local_rect: Option<Rect>,
        tint: Color,
    },
}
```

The `'a` parameter is only used by `Shape::Mesh`. Non-`Mesh`
construction sites infer `Shape<'static>` and behave identically to
today — no churn at the call site:

```rust
// Today (and after):
ui.add_shape(Shape::RoundedRect { local_rect: None, radius, fill, stroke });

// New, with the same surface:
ui.add_shape(Shape::Mesh { mesh: &star, local_rect: None, tint: Color::WHITE });
```

A thin convenience method stays available for the common path:

```rust
impl Ui<'_> {
    pub fn add_shape(&mut self, shape: Shape<'_>);   // primary

    pub fn add_mesh(&mut self, mesh: &Mesh) {
        self.add_shape(Shape::Mesh { mesh, local_rect: None, tint: Color::WHITE });
    }
}
```

### Public `Mesh` (what users construct)

```rust
// src/primitives/mesh.rs (or src/shape/mesh.rs)
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable, PartialEq)]
pub struct MeshVertex {
    pub pos: Vec2,    // 8 B — logical px, owner-local coords
    pub color: Color, // 16 B — linear RGBA, premultiplied (matches Quad)
}
// 24 B, no padding.

#[derive(Default, Clone)]
pub struct Mesh {
    pub vertices: Vec<MeshVertex>,
    pub indices: Vec<u16>,
}

impl Mesh {
    pub fn new() -> Self;
    pub fn with_capacity(verts: usize, indices: usize) -> Self;
    pub fn clear(&mut self);

    /// Push a vertex; returns its u16 index for use in `triangle`.
    pub fn vertex(&mut self, pos: Vec2, color: Color) -> u16;

    /// Push three indices (CCW winding by convention).
    pub fn triangle(&mut self, a: u16, b: u16, c: u16);

    /// Append another mesh, offsetting indices.
    pub fn append(&mut self, other: &Mesh);

    // Convenience builders, added as widgets need them — not load-bearing:
    //   pub fn filled_triangle(a, b, c, color) -> Self
    //   pub fn filled_polygon(points: &[Vec2], color) -> Self
    //   pub fn stroked_polyline(points: &[Vec2], width, color) -> Self
}
```

Format choices:

- **Linear RGBA, premultiplied** to match the rest of the pipeline
  (Quad's `fill`, the wgpu blend state). egui uses sRGB premul — a
  render-time surprise we don't want.
- **No UV** in v1. Adding `uv: Vec2` later widens `MeshVertex` to
  32 B; revisit when the texture story lands.
- **Indices: `u16`.** 65 535 verts per mesh is enormous for a UI
  primitive; if a real workload hits the limit, split or promote to
  `u32` (one bit on the cmd, parallel pipeline / runtime branch,
  same shader).

### Internal `ShapeRecord` (arena storage; not user-facing)

What `Tree.shapes: Vec<ShapeRecord>` holds — what every pipeline
pass after `Ui` walks. Identical to today's `Shape` for the three
existing variants; only `Mesh` differs (span form):

```rust
// Renamed from today's `Shape`. pub(crate) — users never construct it.
pub(crate) enum ShapeRecord {
    RoundedRect { local_rect: Option<Rect>, radius: Corners, fill: Color, stroke: Stroke },
    Line { a: Vec2, b: Vec2, width: f32, color: Color },
    Text {
        local_rect: Option<Rect>,
        text: Cow<'static, str>,
        color: Color,
        font_size_px: f32,
        line_height_px: f32,
        wrap: TextWrap,
        align: Align,
    },
    Mesh {
        local_rect: Option<Rect>,
        vertices: Span,           // into Tree.mesh_vertices
        indices: Span,            // into Tree.mesh_indices
        tint: Color,
        content_hash: u64,        // hash of vertex+index bytes (cache identity)
    },
}
```

- `Span` is `(start: u32, len: u32)` — the project's canonical
  "slice into flat arena" type. **Frame-local: spans into a Vec
  that gets cleared every frame.**
- **Hash:** `ShapeRecord::Mesh` hashes `local_rect`, `tint`, and
  `content_hash`. The spans themselves are **not** hashed — they're
  storage offsets, not identity. Two frames with identical mesh
  content produce the same `content_hash` even though their spans
  differ, and the encode cache hits.
- Size: `Option<Rect>` (20 B) + 2× `Span` (16 B) + `Color` (16 B) +
  `u64` (8 B) + tag/padding ≈ 64 B. Same neighborhood as
  `RoundedRect`. **No `Arc`, no per-shape allocation.**

Visibility: `pub(crate)`. Users can't construct it directly because
they can't reach into `Tree`'s arenas to produce a valid
`(span, content_hash)` pair — that's the correct ergonomic
boundary. The framework is the only thing that can.

### `add_shape` is the conversion layer

```rust
impl Ui<'_> {
    pub fn add_shape(&mut self, shape: Shape<'_>) {
        // Filter no-ops cheaply (matches today's Shape::is_noop on
        // RoundedRect/Line/Text; trivially false for Mesh).
        if shape.is_noop() { return; }

        let tree = self.forest.active_tree_mut();
        let record = match shape {
            Shape::RoundedRect { local_rect, radius, fill, stroke } =>
                ShapeRecord::RoundedRect { local_rect, radius, fill, stroke },

            Shape::Line { a, b, width, color } =>
                ShapeRecord::Line { a, b, width, color },

            Shape::Text { local_rect, text, color, font_size_px, line_height_px, wrap, align } =>
                ShapeRecord::Text { local_rect, text, color, font_size_px, line_height_px, wrap, align },

            Shape::Mesh { mesh, local_rect, tint } => {
                let v_start = tree.mesh_vertices.len() as u32;
                tree.mesh_vertices.extend_from_slice(&mesh.vertices);
                let i_start = tree.mesh_indices.len() as u32;
                tree.mesh_indices.extend_from_slice(&mesh.indices);
                let content_hash = mesh_content_hash(&mesh.vertices, &mesh.indices);
                ShapeRecord::Mesh {
                    local_rect, tint,
                    vertices: Span::new(v_start, mesh.vertices.len() as u32),
                    indices:  Span::new(i_start, mesh.indices.len()  as u32),
                    content_hash,
                }
            }
        };
        tree.shapes.push(record);
    }
}
```

Three arms are mechanical field-for-field copies (the compiler
optimizes them to a `mem::transmute`-equivalent in release for
identical layouts; even if it doesn't, it's three field moves).
The Mesh arm is the only one doing real work — exactly where the
work should be.

### Why not a `Shape<'a>` that *is* the storage type

Brief answer: `Tree.shapes: Vec<Shape<'a>>` would tie the whole
arena's lifetime to the borrowed `Mesh`, infecting every pass that
touches `Tree`. The arena lives for a frame; the `&Mesh` lives for
one statement. Decoupling at `add_shape` is the only sane place.

## Storage on `Tree`

```rust
// src/forest/tree/mod.rs
pub(crate) struct Tree {
    // ... existing fields ...
    pub(crate) mesh_vertices: Vec<MeshVertex>,
    pub(crate) mesh_indices: Vec<u16>,
}
```

Cleared in `begin_frame`, capacity retained — same lifecycle as
`shapes`. `Forest` aggregates these per layer just like `shapes`.

`mesh_content_hash` is one xxhash pass over the two byte slices
(`bytemuck::cast_slice`). Hashing 1 KB of mesh data per
`add_mesh` is cheap and is the price of stable cache identity.

### Big-mesh escape hatch (deferred, not v1)

For workloads with a single very large static mesh (e.g. a
50 000-vertex chart that's identical across frames), the
copy-every-frame cost is real. **Defer the escape hatch until a
profile shows it matters.** When it does:

```rust
Shape::MeshShared {
    local_rect: Option<Rect>,
    data: Arc<MeshData>, // owns vertices + indices + precomputed content_hash
    tint: Color,
}
```

- `Arc::as_ptr()` is the cache identity (or fold its
  `content_hash` field).
- The encoder either:
  (a) resolves `MeshShared` by copying `data.vertices/indices` into
  the cmd buffer's mesh storage (uniform consumption path
  downstream); or
  (b) keeps the Arc alive on the cmd buffer side and references
  `data` directly at render time. (a) is simpler and keeps the
  composer/backend code one path; pick (a) unless profiling
  disagrees.
- Authors opt in only when they have measured the win. The default
  `add_mesh` path stays the simple, fast, alloc-free choice.

## Pipeline integration

### `RenderCmdBuffer` (encode pass)

The cmd buffer must be **self-contained** — cached snapshots get
replayed without reaching back into a `Tree` that's already moved
on. Add mesh storage on the cmd buffer the same way it carries
`data: Vec<u32>`:

```rust
pub(crate) struct RenderCmdBuffer {
    // ... existing fields ...
    pub(crate) mesh_vertices: Vec<MeshVertex>,
    pub(crate) mesh_indices: Vec<u16>,
}

pub(crate) enum CmdKind {
    // ... existing variants ...
    DrawMesh, // payload: DrawMeshPayload
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub(crate) struct DrawMeshPayload {
    pub(crate) origin: Vec2,    // logical px translation (owner_rect.min + local_rect.min)
    pub(crate) tint: Color,
    pub(crate) vertices: Span,  // into RenderCmdBuffer.mesh_vertices
    pub(crate) indices: Span,   // into RenderCmdBuffer.mesh_indices
}
```

Encoder for `Shape::Mesh`:

1. Compute `origin` (translate `local_rect` or `owner_rect` to top-left).
2. `extend_from_slice` from `Tree.mesh_vertices[shape.vertices.range()]`
   into `cmd_buffer.mesh_vertices`.
3. Same for indices.
4. Push `DrawMesh` cmd.

This is one byte-copy per encode, identical to how the cmd buffer
already absorbs payloads. Cache hit replays the snapshot's mesh
bytes the same way it replays the cmd-data bytes.

### `Composer` (compose pass)

Adds a third per-group span alongside `quads` and `texts`:

```rust
pub(crate) struct DrawGroup {
    pub(crate) scissor: Option<URect>,
    pub(crate) rounded_clip: Option<Corners>,
    pub(crate) quads: Span,
    pub(crate) texts: Span,
    pub(crate) meshes: Span,
}

pub(crate) struct RenderBuffer {
    pub(crate) quads: Vec<Quad>,
    pub(crate) texts: Vec<TextRun>,
    pub(crate) meshes: Vec<MeshDraw>,
    pub(crate) mesh_vertices: Vec<MeshVertex>, // physical px, transform baked in
    pub(crate) mesh_indices: Vec<u16>,
    // ...
}

pub(crate) struct MeshDraw {
    pub(crate) vertices: Span,  // into RenderBuffer.mesh_vertices
    pub(crate) indices: Span,   // into RenderBuffer.mesh_indices
    pub(crate) tint: Color,
}
```

Per `DrawMesh` cmd:
1. Apply accumulated transform + DPI scale to each vertex,
   write into `RenderBuffer.mesh_vertices`. (No pixel-snap on
   mesh verts — user controls geometry; snapping arbitrary
   triangles changes shape.)
2. Copy indices through unchanged.
3. Append a `MeshDraw` to `RenderBuffer.meshes` referencing the
   freshly-written vertex/index ranges.
4. Group splitting follows the same rules as quads/texts; meshes
   share scissor with whatever group they land in.

### wgpu backend (`MeshPipeline`)

Mirrors `QuadPipeline`. Separate pipeline because:
- vertex layout differs (per-vertex pos+color vs per-instance Quad),
- draws are indexed (`draw_indexed`) vs `draw(0..6, 0..N)` for quads.

```rust
pub(crate) struct MeshPipeline {
    pipeline: wgpu::RenderPipeline,
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    vertex_capacity: usize,
    index_capacity: usize,
    // stencil variant (rounded-clip): mirror QuadPipeline's lazy stencil pair
}
```

- Vertex layout: `Float32x2` (pos), `Float32x4` (color).
- Single `write_buffer` per frame for verts, one for indices
  (`bytemuck::cast_slice` — both types are Pod).
- Per `DrawGroup`, after the existing quad draw, iterate the
  group's `meshes: Span` and emit
  `draw_indexed(idx_range, base_vertex, 0..1)` per `MeshDraw`. Tint
  goes through a tiny push-constant or a per-draw dynamic offset
  uniform — cheaper than re-uploading verts when only the tint
  changed.
- Rounded-clip stencil: lazy-initialize the stencil-test variant on
  first frame with `has_rounded_clip`, just like `QuadPipeline`.

**Paint order within a group:** quads → text → meshes? Or
preserved record order? Existing composer flushes a group on
text→quad transitions to keep paint order. Add the same rule for
mesh transitions — each draw kind change inside a group flushes.
Simplest correct behavior; profile later if it shows up as a
group-thrash hotspot.

### Cache integration (`MeasureCache`)

Add two `LiveArena`s parallel to `text_shapes_arena`:

```rust
pub(crate) struct MeasureCache {
    // ... existing fields ...
    pub(crate) mesh_vertices_arena: LiveArena<MeshVertex>,
    pub(crate) mesh_indices_arena: LiveArena<u16>,
}

pub(crate) struct MeasureSnapshot {
    // ... existing fields ...
    pub(crate) mesh_vertices: Span, // into mesh_vertices_arena
    pub(crate) mesh_indices: Span,  // into mesh_indices_arena
}
```

On cache hit (replay):
1. Append snapshot's vertex/index slices into the live `Tree.mesh_vertices`
   / `Tree.mesh_indices`.
2. Retarget `Shape::Mesh.vertices` / `.indices` spans to the new
   start offsets.
3. Push the (retargeted) shapes into `Tree.shapes`.

This is the **exact same dance `text_shapes` already does**
(`src/layout/cache/mod.rs:288, 313, 346`). Compaction passes
update snapshot spans on relocate; mirror for the mesh arenas.

The encode cache (which stores already-emitted `RenderCmd`s) needs
no special handling — its cmd buffer's `mesh_vertices` /
`mesh_indices` are already self-contained per the encoder design
above.

## Cross-cutting

- **Layout / hit-test:** meshes don't participate in either —
  they're paint-only, like other shapes today. Hit-test stays
  rect-only (`DESIGN.md`); a mesh-hit story (point-in-tri) is a
  separate project gated on a real workload.
- **Damage:** `Shape::Mesh` content changes propagate via
  `content_hash` into the per-node hash and through subtree rollup
  → `Damage::compute` sees the dirty node. No new wiring.
- **Bounds for damage rect:** use `local_rect.unwrap_or(owner_rect)`
  as the damage rect contribution. Conservative — a mesh can
  exceed its `local_rect` if vertices fall outside (user error).
  Document; don't try to compute mesh AABB unless damage reports
  drop visible pixels.
- **Showcase:** new "Mesh" tab — a filled triangle, a polygon star,
  a vertex-color gradient quad, and a stress test with 5 000
  vertices to verify the alloc-free claim under a real load.

## Implementation order

Each step lands as its own commit; tests + showcase moves with the
final step.

1. **Rename today's `Shape` → `ShapeRecord`, mechanical.**
   `sd 'Shape' 'ShapeRecord' src/shape.rs` then fix the imports
   site-wide via `cargo clippy --fix --all-targets` (it'll surface
   all the `Shape::*` constructions and the `Vec<Shape>` /
   `&Shape` signatures). No new variant yet, no behavior change.
   Pinning tests stay green. **Lands as its own commit** — every
   subsequent change is additive on top of a clean rename.
2. **Public `MeshVertex` + `Mesh` types, `Tree` arena storage.**
   `MeshVertex` and `Mesh` in `src/primitives/mesh.rs`. `Mesh`
   with `vertices: Vec<MeshVertex>` + `indices: Vec<u16>` plus
   ergonomic builders (`new`, `vertex`, `triangle`, `append`).
   `mesh_vertices` / `mesh_indices` columns on `Tree`, cleared in
   `begin_frame`. Tests: `MeshVertex` `Pod` round-trip; alloc-free
   push after warmup via a `tree_mesh_capacity` reach-in on
   `support::internals`.
3. **Add `ShapeRecord::Mesh` variant + content hash.**
   Stable discriminant tag. Hash test pinning `(local_rect, tint,
   content_hash)` as identity; spans deliberately excluded.
4. **Introduce public `Shape<'a>` + rewrite `Ui::add_shape`.**
   New `pub enum Shape<'a> { RoundedRect, Line, Text, Mesh { mesh: &'a Mesh, .. } }`
   in `src/shape.rs`. `Ui::add_shape` becomes the conversion
   layer (three pass-through arms + one copy-into-arena arm).
   Add `Ui::add_mesh(&Mesh)` thin wrapper. Tests: identical mesh
   across frames → identical `content_hash`; reordered indices →
   different hash; warm-arena steady-state alloc count = 0;
   non-`Mesh` call sites unchanged.
5. **Encoder dispatch + `DrawMesh` cmd.**
   `RenderCmdBuffer.mesh_vertices` / `.mesh_indices`, Pod payload,
   `emit_one_shape` arm. Cleared per frame.
6. **Composer dispatch + `RenderBuffer` mesh arrays.**
   Transform-bake into physical-px vertex copy; `MeshDraw` per
   group. Group-flush rule on draw-kind transition.
7. **`MeshPipeline` in wgpu backend.**
   Vertex + index buffer growth (next-pow2), `write_buffer` upload,
   `draw_indexed` per `MeshDraw`. Stencil variant lazy-built like
   `QuadPipeline`.
8. **Cache integration.**
   `LiveArena<MeshVertex>` + `LiveArena<u16>` on `MeasureCache`,
   snapshot ownership, replay path, compaction. Pinning test:
   identical-content mesh across frames → encode-cache hit;
   changed content → miss.
9. **Showcase tab + golden image regression.**
   Triangle / polygon / gradient quad / 5 k-vert stress. Damage
   debug overlay verifies bounding rect.
10. **(Deferred) `Shape::MeshShared(Arc<MeshData>)`.**
    Land only when a profile shows the v1 `&Mesh`-copy path losing
    on big-mesh throughput. Code mostly mirrors v1 with one
    `extend_from_slice` moved earlier in the encoder.

## Open questions

- **Group-flush granularity.** Always flush on draw-kind transition
  vs interleave within one group via cmd-list replay? Start with
  always-flush (simpler), profile after the showcase lands.
- **u32 indices.** Defer until a workload hits 65 k verts / mesh.
  When it lands: one bit on `DrawMesh` cmd, second `MeshPipeline`
  variant or a runtime branch on the index buffer.
- **Mesh AABB for damage.** Conservative `local_rect` works until
  it doesn't. If it does: cache an AABB on `Shape::Mesh` (16 B
  more) computed at `add_mesh` time.
- **Tint vs vertex-color-only.** `tint` is cheap (one shader
  multiply, no extra upload) and unlocks shape reuse. Cost is
  16 B on `DrawMesh`. Keep unless it shows up as overhead.
- **Mesh + rounded clip stencil interaction.** Probably fine —
  stencil mask is geometry-agnostic — but confirm with a test
  case (mesh inside a rounded-clip frame).

## References

- Shape pipeline anchors: `src/shape.rs`, `src/forest/tree/mod.rs`,
  `src/renderer/frontend/{encoder,cmd_buffer,composer}/mod.rs`,
  `src/renderer/backend/quad_pipeline.rs`,
  `src/renderer/quad.rs`.
- Cache snapshot pattern with flat-arena spans:
  `src/layout/cache/mod.rs` (`text_shapes_arena`,
  `text_shapes` snapshot field, replay at `:288/313/346`).
- egui mesh: `tmp/egui/crates/epaint/src/{shape.rs:60,mesh.rs}`.
- iced batching: `tmp/iced/wgpu/src/triangle.rs:396-545`.
- imgui flat draw-list (the design we're closest to):
  `tmp/imgui/imgui.h` (`ImDrawCmd`, `ImDrawVert`, `ImDrawList`).
