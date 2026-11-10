# Renderer audit: second pass

A proposal for one piece of work. When an item lands, its lasting decision folds into the doc
comment or the `AGENTS.md` entry it names, and the item leaves this file; when every item has
landed or been refused, the file is deleted.

Read 2026-11-10 at `16f63cb17`, after the first symmetry plan landed (B1 to B10, D1). Scope: the
frontend (`renderer/`: encoder, composer, payloads, render buffer), the backend's draw path
(`gpu/pipeline`, `gpu/wgpu_backend`, `gpu/frame/schedule`, `gpu/raster`) and the WGSL they share.
The backdrop passes, the cutout bake and the surfaces were read for their seams alone.

## What the renderer is now

| Family | Record → payload | Row | Vertex stage | Pipelines a format builds | Built | Batch unit |
| --- | --- | --- | --- | --- | --- | --- |
| Fill | `Closed`/`ClosedPaint::Fill` → `DrawClosedPayload<ClosedFill>` | `QuadRow { key: QuadKey, instance: Quad }` | template (`vs_template`) | 4 `FillForm` × 2 clip halves | on census | quad lane, record order, runs per `QuadDraw`, look-back 10 |
| Shadow | box's `LoweredShadow` → `DrawShadowPayload` | `QuadRow` (`ShadowQuad` in a `Quad`'s bytes) | 3 × 3 grid around a hole (`vs_shadow`) | 3 `ShadowEntry` × 2 | on census | quad lane |
| Image | `ClosedPaint::Image` → `DrawClosedPayload<ClosedImage>` | `ImageDrawRow { id, form, instance }` | 4-vertex strip (`Rect`) or template | 3 `ShapeForm` × 2 | on census | tier `Image`, per group, runs per (form, texture) |
| Effect, material | `ClosedPaint::Effect` / a box's material → `DrawClosedPayload<ClosedEffect>` | `EffectDrawRow { key: EffectKey, images, instance }` | template (`palantir_vs`) | per effect: 2 template forms × 2 | at want or first draw, **both clip halves at once** | tier `Effect`, per group, runs per (key, images) |
| Stroke | `ShapeRecord::Stroke` → `DrawStrokePayload` | `Vec<CurveInstance>` | chord strips by `CurveKind` | 1 × 2 | on census | tier `Curve`, per group |
| Mesh | `ShapeRecord::Mesh` → `DrawMeshPayload` | `MeshDrawRow` | app vertices | 1 × 2 | on census | tier `Mesh`, per group |
| Text | `ShapeRecord::Text` → `DrawTextPayload` | `TextDrawRow` | raster quads | raster flat + spun, × 2 | on census | **text lane, batches across groups** |
| Icon | `ClosedPaint::Icon` → `DrawIconPayload` | `IconDrawRow` | raster quads | the text's raster pipelines | on census | **tier `Icon`, per group** |

Replay within a group: quads in record order, then the text batch whose last group it is, then the
tiers in `PaintTier` order. Draw counts per showcase page are in
`.notes/research/2026-11-10-draw-calls.md`: 5 to 22 draws after the text clip and the quad run
join, from 7 to 38 before.

## Findings

Each finding states what the code does, what it should do by the project's own rules, and the
source of that rule.

### Inconsistencies

- **[fixed in item 1] Effect pipelines skip the census and build both clip halves at once.** Every other family
  builds the half a frame binds on the first frame whose census asks for it (`ClipVariant`'s doc:
  "Refused: both built up front"); `effect_pipeline::build` calls `ensure` for `Off` and `On`
  together, and `WgpuEffectStore` builds per format on its own schedule. An app with no rounded
  clip compiles a second program per effect and form that it never binds, at load and at each
  device recovery.
- **[fixed in item 2] Effects are drawn through a different door.** `render_groups` hands each family its pipelines
  a different way: the quads `PassPipelines`, an image a closure over `ShapeForm`, the mesh and
  the curve one `&Pipeline`, the raster tenants a `RasterPipelines` closure, and the effects only
  the `Clipping`, as `EffectPipeline` keeps its own copy of the store's pipelines
  (`EffectPipeline::pipelines`, refreshed at each store revision). `PassPipelines` is the one
  place that picks a pipeline for a frame's clipping; the effects pick theirs a second way.
- **The effect module re-declares what the prelude declares.** `prelude.wgsl` holds
  `Immediates`/`imm`, `clip_from_px`, `frag_px`, `turned` and `LUMA`; the effect module holds
  `PalantirImmediates`/`palantir_imm`, an inline NDC in `palantir_vs`, the frame origin added by
  hand in `palantir_local`, `palantir_turned` and `PALANTIR_LUMA` (`effect/material.wgsl`). Same
  layout and arithmetic, two copies, which `AGENTS.md` refuses ("Shared WGSL has one copy, under
  the `palantir_` prefix"). The prelude stays out of an effect module because its names carry no
  prefix.
- **The template's fragment tail is written four times.** "The interior is the paint; elsewhere
  the template's distance fades it" is spelled out in `template_color` (quad), `fs_shaped`
  (image), `palantir_material_shade` (material) and `palantir_fs` (effect), two with the
  composite and two with a bare coverage. The vertex half already has one copy,
  `palantir_template_vertex`.
- **A template's vertex count is asked three ways.** `QuadPipeline::form_vertices`,
  `ImagePipeline::form_vertices` and the effect's inline `shape_template::vertices(form.corner_count())`
  each derive it from a `ShapeForm`.
- **Row names and key fields differ by family.** `QuadRow { key }`, `EffectDrawRow { key }`,
  `ImageDrawRow { form }` beside its texture id, `MeshDrawRow`, `IconDrawRow`, `TextDrawRow`, and
  the curves a bare `Vec<CurveInstance>`. Two names for the stream row (`*Row`, `*DrawRow`), and
  the image's pipeline key is a field named for its type, not its role.
- **Stale docs.** `RenderStep::TierBatch` says an image draws "one `draw` per row" (runs per form
  and texture since B6) and a mesh "one `draw_indexed` per `MeshDraw`"; `TextDrawRow::bounds`
  says "the pixel clip is the batch scissor", which `RasterPass::clip_since` replaced for an
  unturned run.

### Unnecessary abstractions

- **`VariantRecipe` is a second spelling of `PipelineRecipe`.** It carries five of
  `PipelineRecipe`'s fields for `InstancedPipeline::ensure`, which copies them into a
  `PipelineRecipe::color(..)`; the quad and effect families write the `PipelineRecipe` directly
  with struct update. "One way to do each thing" (user rules).
- **`InstancedPipeline` holds three of the six instanced families.** Mesh, image and curve share
  its buffer, shader and layout; the quad keeps its own (two layouts, an extras tail uploaded with
  `upload_parts`), the effect its own (a shader per effect), the raster its `RasterPass`. It is a
  helper for the simple case, not the family abstraction its doc names ("what the mesh, image and
  curve families share" is accurate; nothing reads it as more). Kept, with the recipe folded in.

### Batching

- **A shadow's entry splits the runs the composer joins.** The composer joins a group's quads by
  `QuadKey`, look-back 10 (`quad_runs`); the cutout plan picks each shadow's `ShadowEntry` after
  it, in the backend, and `QuadPipeline::runs` splits where the entry differs. Measured: the
  Shadows page draws 15 quad runs where its keys allow 11 (`2026-11-10-draw-calls.md`, step 9d).
- **Icons batch per group; text batches across groups.** Both are raster tenants of one
  `RasterPass` program. Text clips an unturned run's quads to its bounds on the CPU
  (`clip_since`), so a batch crosses scissor groups and closes only at a rounded clip, a barrier
  or a draw over it; an icon is a `PaintTier`, one batch per group, cut by the group's scissor. A
  toolbar whose icons sit in several clipped groups draws one icon batch per group.
- **Only a solid box occludes.** The occlusion prune (`record_opaque_cover`) drops quads under an
  opaque solid box; an opaque image over quads prunes none, though the image store learns at
  upload whether an image is opaque (it skips the premultiply for one,
  `wgpu_image_store.rs`). Quads are raster-bound on the AMD iGPU, 4.1 M fragments over 1.12
  screens (`PERF.md`, "The main pass on the AMD iGPU"). No showcase page puts an opaque image over
  quads it would drop, so the gain is unmeasured.

### Pipeline consolidation

The pipelines that remain apart are apart for a measured reason, and none should merge:

| Kept apart | Reason | Source |
| --- | --- | --- |
| Fill solid / gradient | VideoCore: 54 temporaries for every brush, 22 for a solid | `AGENTS.md`, "Coverage is geometry" |
| Shadow `General` / `Tables` / `TablesWide` | an entry holds only the forms it reaches, so a tiler runs it on more threads | `shadow.wgsl`, `fs_shadow_tables` |
| Raster flat / spun | the unturned glyph, the hot case, reads no turn | `raster_pipeline` |
| Image rect / template | 4 vertices against 138 for a plain image | `AGENTS.md`, "A plain image keeps its one quad" |
| Shadow / effect / outside box | coverage of a blur is no distance; investigated this session | `ShadowQuad`'s doc |
| Every pipeline × clip half | the clip's code cost a clipless frame 16 to 23 % | `AGENTS.md`, "Rounded clip" |
| Text lane / tiers (D2) | structural, no workload asks | `PERF.md`, "Open" |
| Sharp fill rect form (D3) | waits for the V3D measurement | `PERF.md`, "Open" |

## Proposed changes

In order of value per cost. Each refactor moves no golden; the visual suite and the composer's
captured streams prove it.

1. [x] **Effect pipelines on the census**, done. A want builds a form's clipless half; the first
   frame whose draws name a rounded clip builds the clipped half of every wanted form at once
   (`WgpuEffectStore::prepare` takes the frame's `Clipping`), so a press inside a clip reaches a
   built pipeline after that frame. `EffectDevice::ensure` builds one half; the decision lives in
   `WgpuEffectStore`'s doc. Test: `the_clipped_half_waits_for_a_frame_with_a_rounded_clip`.
2. [x] **Effects drawn through `PassPipelines`**, done. `PassPipelines::effect(key)` reads the
   format's effect pipelines from the store's borrow, which `submit` holds across the frame's
   passes, and the effect family takes them as the image family does, a pipeline per run key;
   `EffectPipeline` drops its copy, its revision check and its kinds, and the store its revision.
   Mesh, curve and raster keep the one pipeline they take, as `PassPipelines` picks it; refused:
   `PassPipelines` handed to every family, which would tie the raster pass and its bench to the
   format's whole set.
3. **One shared WGSL copy of the frame's coordinates.** A `primitives::wgsl` include declares
   `PalantirImmediates`, `palantir_clip_from_px`, `palantir_frag_px`, `palantir_turned` and
   `PALANTIR_LUMA`; the prelude and the effect module include it, and the prelude's unprefixed
   `Immediates`, `clip_from_px`, `frag_px`, `turned` and `LUMA` go. Refused: the prelude in the
   effect module, whose unprefixed names an app's text could collide with.
4. **One template fragment tail.** `palantir_template_coverage(region, geo, ring_r)` and
   `palantir_template_fill(region, geo, ring_r, fill, border, width)` in `shape_template.wgsl`;
   the four stages call them. Refused: one function with a border of zero for the image and the
   effect, which would pay the composite where no border is.
5. **One template vertex count.** `shape_template::form_vertices(ShapeForm)` (4 for `Rect`, the
   template's count for the others); the three callers go through it.
6. **`VariantRecipe` goes.** `InstancedPipeline::ensure` takes a `PipelineRecipe` built from
   `InstancedPipeline::recipe(label, format)`, as the quad and the effect build theirs.
7. **Join quad runs on the draw.** The backend re-runs the look-back join on `QuadDraw` after the
   cutout plan picks the entries (the composer's join on `QuadKey` stays for the occlusion prune
   and the CPU bench). Gate: the Shadows page's 15 runs drop toward 11, and
   `atomic/composer/cpu/quads/*` and `integration/frame/gpu/*` hold. Refused: the entry decided in
   the composer, which would move the cutout plan, a backend's knowledge of its atlas, into the
   frontend.
8. **Icons in the raster lane.** An unturned icon's quad clips to its visible rect on the CPU as a
   glyph does, and icons batch across groups as text does, each tenant its own batch stream, so
   `PaintTier::Icon` goes. Gate: draws on the Icons and Dock pages, and a toolbar fixture of icons
   in clipped groups. Cost: icon and text batches order against each other, which the lane's one
   batch state must track. As WebRender batches its image and text primitives in one list with a
   look-back.
9. **Opaque images occlude quads.** `ImageHandle` keeps whether its texels are opaque, which the
   upload learns, and a plain opaque image (sharp box, `Fit` covering its rect, no tint alpha)
   records its rect as an opaque cover. Gate: a fixture of a full-bleed photo over cards; the AMD
   iGPU's main pass. Refused until measured: an overdraw saving no page shows.
10. **Rows named alike.** `QuadRow` and the `*DrawRow`s take one suffix, and the image's
    pipeline field is named `key` like the quad's and the effect's.
11. **The stale docs** of `RenderStep::TierBatch` and `TextDrawRow::bounds` say what the code
    does.

Items 1 to 6, 10 and 11 are refactors with no visible change; 7 and 8 change draw counts and need
their gates; 9 waits for a workload.
