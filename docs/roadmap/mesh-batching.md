# Mesh batching & generalized overlap-aware reorder

Cross-group mesh batching that mirrors `TextBatch`, folded into a
generalized overlap-aware reorder that unifies the per-kind state
machines in the composer.

Status: **Phase 1 landed** (commit `26cff73` + `e9faa9e` —
per-instance GPU state for mesh transform/tint, content-stable
vertex arena). Phase 2 + 3 pending.

## Best practices survey

Distilled from external sources and Palantir's existing
`references/` notes:

- **Group by identical pipeline state** (shader, blend, scissor,
  bind group). Each state break = a drawcall. ARM mobile guide flags
  ~100 drawcalls as the stutter threshold on low-end GPUs.
- **Order preservation is the hard constraint.** UI is alpha-blended
  back-to-front, so unlike opaque 3D you cannot freely sort by state
  — record order matters wherever two draws overlap.
- **Reorder is allowed only between non-overlapping draws.** Sokol-GP
  looks back N draw commands (uses 8) and merges any pair with matching
  state iff no draw between them overlaps either of them. Godot's 2D
  batcher uses the same idea with an `item_reordering_lookahead`
  window. Iced's `layer::Stack::merge` fuses adjacent layers only when
  their kind-spans don't conflict.
- **Higher kinds get a free pass.** Topmost kinds (text in many
  engines) are always safe to defer past lower kinds — enables
  ImGui/Iced-style text coalescing across regions.
- **Index-rebase for one-drawcall merge.** ImGui concatenates per-list
  vertex/index data into one global buffer and rebases indices
  (needed because WebGL/GLES2 lack `base_vertex`). On wgpu `base_vertex`
  is free — Palantir's shared `meshes.arena` already exploits this.
- **MultiDrawIndirect** on native wgpu collapses N draws differing only
  in index range. Behind `MULTI_DRAW_INDIRECT` feature; falls back to
  a loop on web.
- **Vello / piet-gpu** is a different model (compute path renderer);
  not applicable to Palantir's instanced-quad architecture.

Sources:
- https://nical.github.io/drafts/gui-gpu-notes.html
- https://developer.arm.com/documentation/101897/latest/Optimizing-application-logic/Draw-call-batching-best-practices
- https://toji.dev/webgpu-best-practices/indirect-draws.html
- https://deepwiki.com/ocornut/imgui/9.4-performance-optimization
- https://gist.github.com/floooh/10388a0afbe08fce9e617d8aefa7d302
- https://github.com/edubart/sokol_gp
- https://github.com/godotengine/godot/issues/38004
- https://jorenjoestar.github.io/post/modern_sprite_batch/

## Current shape — concrete inventory

Four GPU paths, each with its own batching story:

| Path | File | Per-frame draws | Batching state | Tint/transform |
|---|---|---|---|---|
| Quad (instanced SDF) | `backend/quad_pipeline.rs` | **1 per group** | instanced array of `Quad` per group | per-instance attr |
| Text (glyphon) | `backend/text/*` | **1 per `TextBatch`** | cross-group coalesce via `TextBatch.last_group` | per-glyph in glyphon atlas |
| Mesh (per-vert color) | `backend/mesh_pipeline.rs` | **1 per `MeshDraw`** | none beyond pipeline-bind amortization within a group | per-instance vertex buffer (`MeshInstance`) |
| Mask quad (rounded clip) | `quad_pipeline` mask variant | 0–2 per group | stencil pre-stamp / post-clear | n/a |

**Composer state** (`src/renderer/frontend/composer/mod.rs`):
- 3 cursors: `quads_start`, `texts_start`, `meshes_start`.
- 2 overlap scratches: `mesh_rects` (group-scoped, cleared on flush)
  and `batch_text_rects` (batch-scoped, cleared on `close_batch`).
- 1 open batch state: `open_batch: Option<OpenBatch>` (text only).

**Within-group kind order is fixed: quad → text → mesh**, enforced by
the composer flushing on:
- quad recorded → check overlap vs. `mesh_rects` + `batch_text_rects`
  (lower kind, would re-order under).
- mesh recorded → unconditionally `close_batch` (text emit must land
  before this group's meshes, since mesh is highest).
- text recorded → checked vs. mesh implicitly via the kind hierarchy
  (text-after-mesh in same group means text paints under mesh —
  forces flush, which actually happens at the mesh boundary).

**Scheduler** (`backend/schedule.rs`) walks groups linearly, drains
text batches anchored at `last_group <= i`, emits steps. Mesh draws
are emitted **per-`MeshDraw`** inside the `Meshes` step
(`backend/mod.rs:646-654`) — pipeline bound once per `Meshes` step,
then a loop over `buffer.meshes.draws[start..end]` calling
`draw_indexed`.

**The asymmetry that remains:** quads and text already have their
natural batching. Mesh now has per-instance GPU state (Phase 1) so
two meshes with different tints/transforms are merge-eligible, but
still lacks the cross-group `MeshBatch` infrastructure parallel to
`TextBatch`.

## Where Palantir lands vs. best practices

| Practice | Palantir today | Status |
|---|---|---|
| Pipeline-state batching within a group | quads: instanced · text: glyphon batched · meshes: bind-only | partial |
| Overlap-aware reorder | quad↔text, quad↔mesh, text↔mesh checks in composer | done — same algorithm as Sokol-GP / Godot, just kind-typed |
| Cross-group coalescing | text only (`TextBatch`) | partial — meshes missing |
| Atlas / state-key sharing | gradient LUT, glyph atlas | done |
| Per-instance GPU state (no CPU tint/transform bake) | meshes use `MeshInstance` vertex buffer | done |
| `base_vertex` use | yes (`meshes.arena` is shared) | done |

## Phase 2 — Generalize `OpenBatch` and add `MeshBatch`

Mirror `TextBatch` for meshes; collapse the three per-kind state
machines into one indexed structure.

`src/renderer/frontend/composer/mod.rs`:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
enum Kind { Quad = 0, Text = 1, Mesh = 2 }

struct OpenBatch {
    start: u32,
    last_group: u32,
    union_aabb: URect,
}

struct Composer {
    batches: [Option<OpenBatch>; 3],
    rects:   [Vec<URect>; 3],
    // remove: open_batch, batch_text_rects, mesh_rects (group-scoped)
}
```

Unified rule on every draw cmd at kind `K_new` with AABB `r`:

```rust
// 1. For each open batch at kind K > K_new, if r overlaps its
//    union → close it.
for k in [Kind::Text, Kind::Mesh] {
    if k as u8 <= K_new as u8 { continue; }
    if let Some(b) = &self.batches[k as usize]
        && b.union_aabb.intersect(r).is_some()
        && any_overlap(&self.rects[k as usize], r)
    {
        self.close_batch(k, out);
    }
}
// 2. On scissor/clip change in set_clip: close ALL batches.
// 3. Append to K_new's batch, opening if absent.
```

`src/renderer/render_buffer.rs` adds `MeshBatch` mirroring `TextBatch`:

```rust
pub(crate) struct MeshBatch {
    pub(crate) meshes: Span,
    pub(crate) last_group: u32,
}
pub(crate) mesh_batches: Vec<MeshBatch>,
```

`src/renderer/backend/schedule.rs` — the existing text-drain loop
generalizes to "drain any batch whose `last_group < i` before
emitting group i". `RenderStep::Meshes { group, range }` becomes
`RenderStep::MeshBatch { batch: usize }`.

**Draw-call shape within a MeshBatch — two options:**

- **(A) N `draw_indexed` calls per batch, bind once.** Same drawcall
  count as today, but pipeline + buffers bound once across the whole
  batch. Minimal disruption to vertex layout.
- **(B) One `draw_indexed` per batch, with per-vertex `draw_id: u16`
  indexing per-instance state in an SSBO.** ImGui shape. Costs +2 B
  per vertex; one drawcall per batch.

**Recommendation:** (A) first. Mesh drawcall count is rarely the
bottleneck in typical UI workloads (<10 meshes/frame). Defer (B)
until profiling shows it matters.

## Phase 3 — cleanup

- Delete `Composer.mesh_rects`, `Composer.batch_text_rects`,
  `Composer.open_batch`; collapse to the unified arrays.
- Delete the bespoke `OpenBatch` (text-specific) struct.
- Rename `text_batches` / `mesh_batches` to `batches: [Vec<Batch>; 3]`
  (quad slot stays empty — quads don't cross-group coalesce).
- Update `for_each_step` to iterate the unified batch list keyed by
  `(kind, last_group)`.

## Phase 4 (deferred, separate work) — retained mesh arenas

Phase 1's content-stable vertex stream makes this tractable: hash a
mesh by `(content_hash, owner WidgetId)` and reuse last frame's arena
slice when unchanged — like `MeasureCache` does for layout. Big win
for static meshes (icon glyphs, decorative paths). Don't build this
speculatively; pin a workload first.

## Sizing / risk

- **Phase 2** is ~300 LOC, mostly composer + schedule + a fresh
  `MeshBatch` parallel to `TextBatch`.
- **Phase 3** is mechanical cleanup (~200 LOC, mostly deletions).

Land Phase 2 standalone; Phase 3 as a follow-up — refactoring
working text-batch code in the same PR adds risk for no perf gain.

## Lookback / out-of-order merge — not proposed

Sokol-GP-style lookback (peek back N cmds and merge non-adjacent
same-kind draws if no intervening overlap) is the obvious next
step but **not worth doing for Palantir** until profiling shows
>100 drawcalls/frame. Real UI scenes are kind-clustered already
because widgets paint their own quads/text/meshes contiguously.

## Overlap-test data structure — not proposed

Both rect scratches today are linear scans guarded by a union-AABB
reject. Godot and Iced stay with linear scan — they cap batch size
(Iced flushes at ~4k quads). When batches are tiny (<32 rects),
linear-scan wins by a mile. Only worth revisiting if profiling
shows the overlap scan hot.
