# Review: renderer and GPU usage, with the backdrop integration (2026-10-29)

Scope: `src/renderer/`, `src/gpu/`, and the code that drives them from `src/host/window_driver/`,
`src/damage/pane_walk.rs` and `src/cascade/pane_row.rs`. Four reviews ran in parallel: backdrop
integration end to end, backend orchestration, pipelines and atlases, and the CPU frontend. Every
bug below was checked against the code. The test suite runs in 4.6 s (2133 tests) with no slow
test.

## Proposal

### Root causes

1. **The backdrop capture set is decided last.** `PaneCaptures::settle` runs after compose, but
   every stage before it needs the answer. So each stage approximates it:
   - the damage walk marks the stale panes;
   - the encoder keeps the footprint of every pane whose rect meets the damage, held or not;
   - the composer closes a group and a text batch for every pane, held or not;
   - the host mutates the renderer's output to finish each row;
   - the GPU side keeps a second cache of the same plan and region, and computes the region
     again.

   The pane's plan, key and reach are also derived in more than one place, from σ values that are
   rounded differently.
2. **There is no per-window render stream on the GPU side.** Without it:
   - the host holds GPU attachments (backbuffer, stencil, freshness, cutout plan) and makes GPU
     capability calls;
   - the backend keeps parallel owner-keyed maps (timings, backdrop cache, backdrop target
     groups);
   - CPU planning types (`RepaintScissors`) live in `gpu`;
   - the present path is decided twice.
3. **Paint-order arbitration has no single owner.** Each draw kind hand-writes cull → order →
   register → pane break in its own shape. One of these paths misses a check (bug B1), and the
   reach rect of a turned quad is derived in two ways (bug B3).
4. **Shared mechanics live inside one consumer or in a hub.**
   - `ShelfPacker` sits in `raster_atlas`, and the backdrop atlas reaches into it.
   - `ShaderBody` holds every shader's path and constants, which gives `pipeline ⇄ raster` and
     `pipeline ⇄ shader_body` module cycles.
   - Each atlas has its own growth and "full" contract, and the backdrop atlas panics.
   - Each pipeline hand-rolls the same scaffolding with a different bind and upload shape.
5. **Rust and WGSL mirrors are not pinned.** `BackdropSlot`, the shadow spread rules and the shadow
   box are written twice, and no test compares the two copies.

### Target design

- **The pane ledger (renderer, per window).** It replaces the split between `PaneCaptures.held`
  and `Backdrops.cache`.
  - It is the one record of what each pane holds: plan, surface and region.
  - Before encode, it takes the cascade's `PaneRow`s, the dirty set and the frame kind, and gives
    the capture candidates: full ∪ dirty ∪ held does not serve.
  - The cascade computes key, plan and footprint once per row, with the f32 σ and one scale
    source.
  - The encoder adds exactly the candidates' footprints.
  - The composer opens a barrier only for a candidate and writes the row's final source:
    `Capture { region }`, `Clear` or `Held { region }`. No placeholder is written, and the host
    does not mutate the buffer.
  - The capture budget is checked once, before the build.
  - The present path (Direct or through the backbuffer) is sealed in the CPU half, right after
    the ledger settles, from `captured_area` and the target's copy capability.
  - `RepaintScissors` moves beside the ledger in `renderer`.
  - **Constraint to check first:** the composer's footprint starts from the clipped quad rect.
    The cascade row must carry the same clipped footprint, or else the ledger's "serves" test
    disagrees with the region that the composer captures.
- **`Backdrops` (gpu) becomes a slot allocator: `(owner, key) → Kept`.**
  - It reads the region from the row and does not compute it again.
  - It is made on the first frame that has panes, not at backend start (8.4 MB atlas, 5 blur
    pipelines).
  - Its atlas is a `PackedAtlas`. A full atlas degrades and does not panic.
  - Its work unit is the break (`BlurBreak`): timing, copies and blurs go one break at a time.
  - `BackdropBlur` borrows the jobs and does not copy them. `BlurJob` carries its own footprint.
- **`RenderStream` (gpu, per owner).** It holds the backbuffer with its blur-source group, the
  stencil, the backbuffer freshness, the timings, the cutout plan, and the cached sampled groups
  of the swapchain images. It retires as one unit. The host passes `submit(owner, target,
  &FramePlan)`, and the backend builds a pure, testable `FramePasses` plan and runs it through one
  `DrawCtx`.
- **`PaintOrder` (composer).** It owns the batch, the higher-kind rects, the group cursors, the
  occlusion pruner and the pane break, with one `admit(lane, bounds)` over the total order
  Quad < Text < Mesh < Image < Icon < Curve. Each draw kind's lowering hands it the one reach rect
  that its instance type defines, the same rect the pruner uses.
- **`gpu::resource::{shelf_packer, packed_atlas}`.** These are the one growable atlas, with one
  `AtlasFull` result, for the raster sides and the backdrop results.
- **Pipeline families.**
  - Each family owns its shader source, its constants, and an instance layout derived from
    `offset_of!`.
  - They share an `InstancedPipeline<T>` with one upload, bind and draw shape.
  - `RasterProgram` moves into `gpu::pipeline`.
  - Quad draws are keyed once per instance (`QuadDraw::{Form, Shadow}`), and the pipeline knows
    which groups each key binds. The backend does not branch on `QuadForm::Backdrop`.

### Decisions that are yours

- **naga as a dev-dependency:** for a reflection test that pins every Rust/WGSL struct and vertex
  location, and that replaces the hand-written WGSL call-graph scanner in `shader_body/tests.rs`.
  Without it, batch 1 pins `BackdropSlot` with hand `offset_of!` asserts.
- **`DriverError` variants (public API):** they box `RequestAdapterError`, `RequestDeviceError`
  and `CreateSurfaceError` as `dyn Error`. The guide keeps a foreign error as its own type, and
  the crate already does `pub use wgpu`.
- **Device loss (public behavior):** no `set_device_lost_callback` or `on_uncaptured_error` is
  set. A GPU reset panics in `Gpu::wait`, in `Capture::take`, or in wgpu's default handler.
  Choose an error case that the host surfaces, or a documented crash contract.
- **Benches and the layering rule (AGENTS.md):** `gpu/bench.rs`, `curve_pipeline/bench.rs`,
  `image_pipeline/bench.rs` and `frontend/bench.rs` import `host`, `ui`, `widgets` and
  `widget_core`, against "a top-level module imports only modules to its left". AGENTS.md also
  puts each `bench.rs` beside the code it measures. Choose: write a bench exemption into the
  layering rule, or move the end-to-end benches to `src/bench/`.

## Plan

### Batch 1: bugs

Each item is small and independent, and each gets a test.

- [ ] `src/renderer/frontend/composer/session.rs:1170-1185` `quad_forces_flush`.
  - Defect: on an open-batch hit whose batch's last group is already closed, it closes the batch
    and returns without the `higher_kinds.any_overlap` / `closed_hit` test. A quad after an
    overlapping image, mesh, icon or curve in the open group stays in that group and is painted
    below it.
  - Repro: `text(0,0,50,20); image(100,0,40,40); draw(100,0,40,40); image(150,0,40,40);
    draw(0,0,200,40)`.
  - Target: flush when `renders_after || closed_hit(q) || higher_kinds.any_overlap(q)`, whatever
    the batch-close branch did.
- [ ] `src/renderer/frontend/composer/occlusion_pruner.rs:86-89`.
  - Defect: the pruner exempts only shadows. A pane quad covered by a later opaque quad in its
    group is dropped, but its `BackdropRow` stays, so the frame still copies and blurs it: about 5
    passes, 48 µs each on the M2.
  - Target: never prune an `is_backdrop()` quad, or drop its row with it.
- [ ] `src/renderer/frontend/composer/session.rs:194-206` `turned_rect` against
  `src/renderer/quad.rs:37-41` `Quad::shaded_rect`.
  - Defect: for a turned quad, cull, order and the pane break use a rect one pixel row short of
    the shaded AA fringe. A draw that touches only the fringe neither flushes nor orders, and a
    later pane can share a break that misses it.
  - Target: one reach function per instance type, used by cull, order, pane break and prune.
    Check turned images (`session.rs:411-420`) the same way.
- [ ] `src/renderer/frontend/composer/session.rs:942` against `src/cascade/pane_row.rs:24-35`.
  - Defect: the composer builds the `BlurPlan` from the σ lane rounded to f16. The cascade builds
    it from the f32 σ. Near a level threshold the composer's reach can pass the cascade footprint
    plus its 1 px margin: on a partial frame the capture then reads pixels that were not encoded.
  - Target: the composer takes `pane.backdrop.sigma() * scale` in f32. Batch 2 removes the second
    derivation.
- [ ] `src/gpu/wgpu_backend/mod.rs` `run_main_pass` against
  `src/gpu/frame/gpu_timings/query_tracer/mod.rs:32,354-361`.
  - Defect: timing slots are per pane (`MAX_TIMED_PANES = 8`, 3 slots each), but blurs run per
    break. A break whose last job index is 8 or more drops its `BlurEnd`, so the whole break's blur
    time is lost. 12 panes in one break publish `blurs: Some(0)`.
  - Target: three marks per break (capture start, blur start, blur end), capped by breaks.
- [ ] `src/gpu/pipeline/quad_pipeline/shader.wgsl:647`, `src/gpu/pipeline/curve_pipeline/shader.wgsl:426`.
  - Defect: implicit-LOD `textureSample` under branches on flat varyings. Naga accepts it only
    because it relaxes fragment uniformity; Tint rejects it.
  - Target: `textureSampleLevel(…, 0.0)`, as the image shader does. The LUT has one mip, so the
    pixels do not change.
- [ ] `src/gpu/pipeline/quad_pipeline/quad_form/mod.rs:494-501`, `shader.wgsl:582-593,636-640`.
  - Defect: an unknown fill tag silently becomes Gradient, then Solid, and an unknown spread
    becomes Pad. The crate mints these tags, so such a value is a logic error painted as a
    plausible quad.
  - Target: `debug_assert!` / `unreachable!` in `QuadForm::of`. The WGSL fallback is the last
    known tag.
- [ ] `src/gpu/backdrop/backdrops/mod.rs:28-35` against `quad_pipeline/shader.wgsl:98-115`.
  - Defect: `BackdropSlot` (48 B, `clear: vec3` at 32) is not pinned. A field reorder would
    misread silently, because a Rust `[f32; 3]` aligns to 4 and a WGSL `vec3` to 16.
  - Target: `size_of` / `offset_of!` asserts now, or the naga reflection test (your decision).

### Batch 2: the pane ledger decides the capture set once

Files: `src/renderer/pane_captures/`, `src/renderer/render_buffer/backdrop_row.rs`,
`src/renderer/frontend/` (encoder `mod.rs`, composer `session.rs`, `pane_break.rs`),
`src/cascade/pane_row.rs` and `engine.rs`, `src/damage/pane_walk.rs`,
`src/gpu/backdrop/backdrops/`, `src/gpu/surface/viewport.rs`, `src/host/window_driver/mod.rs`.

- [ ] `encoder/mod.rs:52-73`.
  - Defect: the encoder adds the footprint of every pane whose rect meets the damage. A caret
    blink inside a 4K frosted modal then encodes and composes almost the whole frame, and the
    scissors drop it on the GPU.
  - Target: the ledger gives the candidates before encode, and the encoder adds only their
    footprints.
- [ ] `session.rs:954-964` `open_barrier`.
  - Defect: a held pane still closes the batch and the group, for a break the backend never takes:
    one more group and one more text draw per held pane on each partial frame.
  - Target: a barrier only for a candidate. A held pane joins the open group.
- [ ] `backdrop_row.rs:27`, `session.rs:977`, `window_driver/mod.rs:383,395`.
  - Defect: the composer writes `PaneSource::Held` as a placeholder, and the host's `settle`
    mutates the renderer's output. On overflow, `settle` runs twice and the frontend builds twice.
  - Target: the composer writes the final source from the ledger's input to `Frontend::build`.
    The budget is checked before the build, and `Settled` is the host's only read.
- [ ] `pane_captures/mod.rs:65,87-114` against `backdrops/mod.rs:42-57,190-248`.
  - Defect: two caches hold the same plan, surface and region, synced by hand.
    `plan.region(footprint, surface)` is computed twice per row per frame, and each cache evicts
    on its own. The only guard is the Held branch's `expect` / `debug_assert`, which panics in
    release if a frame settles but never prepares.
  - Target: `PaneSource::{Capture { region }, Held { region }, Clear}`. `Backdrops.cache` holds
    `Kept` only.
- [ ] `cascade/pane_row.rs`, `session.rs:942`, `encoder/plane_ctx.rs:127-138,460-466`,
  `cascade/engine.rs:553-556`, `scene/tree/mod.rs:509`.
  - Defect: the plan, the reach and the scale are derived in the cascade and again in the
    composer, and `PaneKey` is built in three places, with the chrome row `0` hard-coded twice.
  - Target: `PaneRow` carries key, plan and the clipped footprint. The composer and the encoder
    read it. Check the clip constraint (Proposal) first.
- [ ] `pane_walk.rs:41-43,67-76` against `frontend/mod.rs:64-72`.
  - Defect: "this frame's pane keys in paint order" is built twice (`PaneWalk.keys` and
    `RenderBuffer.live_panes`). Both evictions run a linear `live.contains`, O(P²) per frame per
    window.
  - Target: one key list, exported by damage. Rename `PaneWalk.held` to `listed`, since "held"
    means three things today.
- [ ] `window_driver/mod.rs:462-477` (merges two review reports).
  - Defect: `render_to_texture` decides Direct → ViaBackbuffer again from `captured_area` and
    `gives_copy`, although `PresentPath` says it is sealed in `cpu_frame`.
  - Target: seal it after the ledger settles. `TargetKey` carries the copy capability, so
    `note_target` also invalidates when the target's usage changes.
- [ ] `src/gpu/surface/viewport.rs:3-81` (merges two review reports).
  - Defect: `RepaintScissors` is CPU planning in `gpu::surface`, and it imports damage,
    `pane_captures` and `backdrop_row`. `ViewportPush` shares the file against one major struct
    per file.
  - Target: move `RepaintScissors` to `renderer` beside the ledger. Move `ViewportPush` beside
    `IMMEDIATES_BYTES` in `gpu/pipeline`.
- [ ] `damage/dirty_panes.rs:233`, `pane_captures/mod.rs:70`.
  - Defect: one budget (8 captures in a partial frame) is enforced twice.
  - Target: one check in the ledger.
- [ ] `scene/tree/pane_entry.rs:7` against `backdrop_row.rs:32`.
  - Defect: two crate types are both named `PaneSource` (`{Chrome, Shape}` and
    `{Held, Capture, Clear}`). `PaneKey`'s doc says "ordinal among the owner's shapes", but
    `pane_walk` says "its row among its owner's".
  - Target: rename the scene one to `PaneOrigin`, and give the key one wording.
- [ ] Tests:
  - one gated `BackdropRow` fixture in `render_buffer` internals, which replaces the copies in
    `backdrops/tests.rs:23-60`, `pane_captures/tests.rs:15-42` and the `viewport.rs` tests;
  - one shared `CLEAR` (`backdrops/tests.rs:21`, `tests/clear.rs:21`);
  - `tests/mod.rs:296-300` uses `window()`.
  - Add tests: a held pane on a partial frame neither breaks nor adds its footprint; the
    Direct → backbuffer switch (`captured ≥ surface`, `!gives_copy`, also with a test target
    without `COPY_SRC`); the cascade and composer footprints agree.

### Batch 3: one packed atlas

Files: `src/gpu/raster/raster_atlas/` (`shelf_packer.rs`, `side.rs`, `mod.rs`, `atlas_slot.rs`,
`free_slots.rs`), `src/gpu/backdrop/backdrops/`, new `src/gpu/resource/{shelf_packer,
packed_atlas}`.

- [ ] `backdrops/mod.rs:6`, `raster_atlas/side.rs:133-182`, `backdrops/mod.rs:350-399`.
  - Defect: one structure (packer, texture, view, grow by blit, rebind) is written twice. The
    backdrop atlas reaches into raster internals for `ShelfPacker`.
  - Target: `PackedAtlas { packer, texture, view, size, ceiling, pending_grow }` with
    `allocate -> Result<_, AtlasFull>`, `grow_to(edge)` and `deallocate`, used by the raster sides
    and `Backdrops`. The cutout plan keeps its own packer, and its doc says why.
- [ ] `backdrops/mod.rs:383-388`.
  - Defect: past `min(max_texture_dimension_2d, u16::MAX)` a release `assert!` panics on
    data-driven content. A level-0 plan stores results at full resolution: a 4K full-screen pane
    takes 66 MB, so about eight reach the limit on an 8192 device.
  - Target: on `AtlasFull`, evict the results that no pane of this frame uses. If the frame's own
    results do not fit, store a result one level down. Do not panic.
- [ ] `raster_atlas/mod.rs:413-430`, `side.rs:133-156`, `backdrops/mod.rs` `grow`.
  - Defect: a burst that needs several doublings creates a texture at each step (for example a
    4 MiB mask texture that is never sampled). The backdrop atlas copies at each step.
  - Target: compute the needed edge, then make one `grow_to`.
- [ ] `shelf_packer.rs:91-96,136-144`.
  - Defect: empty shelves in the middle never merge, and the fallback takes the first empty shelf,
    not the best fit. So an insert can return `AtlasFull` while room can be evicted. Each eviction
    round scans the whole slab twice.
  - Target: merge adjacent empty shelves on deallocate, and pick the best-fit empty shelf, as
    étagère does.
- [ ] `shelf_packer.rs:84-88,134`, `backdrops/mod.rs` `slot_for`.
  - Defect: a new shelf allocates `vec![row]`, so a raster miss allocates after warmup, and a pane
    that animates its size reallocates its slot every frame.
  - Target: one flat span `Vec` with per-shelf ranges, and slots rounded up with slack.
- [ ] `atlas_slot.rs:279-286`, `free_slots.rs:14-31`.
  - Defect: slot state is split over `placement: Option` and `free: bool`, where `(Some, true)`
    cannot occur. `FreeSlots::release` mutates the slab.
  - Target: `enum SlotState { Placed, Unplaced, Free }`, and `release` on `Slab`.
- [ ] Tests:
  - read back an atlas after a grow;
  - two grows in one frame;
  - backdrop atlas exhaustion.
  - The eviction policy becomes device-free unit tests on the GPU-free core
    (`raster_atlas/tests.rs:285-618`).

### Batch 4: backdrop GPU resources

Files: `src/gpu/backdrop/`. Needs batch 3.

- [ ] `backdrops/mod.rs:37,137-139`, `backdrop_blur/mod.rs:160-264`.
  - Defect: every backend allocates the 1024² `Rgba16Float` results atlas (8.4 MB) and builds 5
    blur pipelines with their groups, though no pane may ever appear.
  - Target: make them on the first frame that has panes.
- [ ] `backdrop_blur/mod.rs:266-360`.
  - Defect: the capture is the whole surface in the surface format (33 MB Bgra8 or 66 MB f16 at
    4K), and the levels are a full-surface mip chain (about 22 MB). Both are grow-only, and one
    capture is kept per format. Each growing frame of a resize drag makes them again.
  - Target: a capture and levels local to the footprint, at a lattice-aligned origin
    (`BlurParams.source_origin` already takes an offset source), grown with slack.
- [ ] `backdrop_blur/mod.rs:153-154,458-459`, `backdrops/mod.rs:91-94`.
  - Defect: `Backdrops.jobs` is copied into `BackdropBlur.jobs` every frame. `footprints` and
    `first_slots` are arrays parallel to `jobs`.
  - Target: `BlurJob` carries its footprint. `BackdropBlur` borrows the jobs in `encode` and
    `copy_capture`, and derives the slots.
- [ ] `wgpu_backend/mod.rs` `run_main_pass`.
  - Defect: the backend walks the breaks, calls `copy_capture` per job, and checks
    `reads_target()`, which is state the last `prepare` left on the `Backdrops` that all windows
    share.
  - Target: `Backdrops::encode_break(encoder, break, color, timings)`. The backend knows only
    "break at barrier b".
- [ ] `backdrops/mod.rs:69,103`.
  - Defect: tuples `Option<(&Texture, &TextureView)>` and
    `Vec<(RenderOwnerId, Texture, TargetSource)>`.
  - Target: named types. Batch 5 removes `targets`.

### Batch 5: a per-window render stream on the GPU side

Files: `src/gpu/wgpu_backend/`, `src/gpu/frame/submission.rs`, `src/gpu/surface/`
(`backbuffer.rs`, `stencil.rs`), `src/gpu/pipeline/quad_pipeline/cutout_plan/`,
`src/host/window_driver/mod.rs`.

- [ ] `window_driver/mod.rs:54-72,466-488,512-523`, `wgpu_backend/mod.rs:145,899-914`,
  `surface/stencil.rs:92-105`.
  - Defect: per-window GPU state has two owners. The host holds backbuffer, stencil,
    `backbuffer_fresh`, scissors and cutouts. The backend holds owner-keyed maps. The two
    `ensure`s have different shapes, and each submit threads 13 fields.
  - Target: a `RenderStream` per owner in `gpu`, retired as one unit, and
    `submit(owner, target, &FramePlan)`.
- [ ] `backdrops/mod.rs:103,307-325`, `backdrop_blur/mod.rs:156`.
  - Defect: `Backdrops.targets` holds a clone of each window's backbuffer. After a resize, the old
    backbuffer stays alive until that window's next frame that blurs from the backbuffer, which is
    never on the Direct path.
  - Target: the stream's backbuffer owns its blur-source group, made and dropped with it.
- [ ] `window_driver/mod.rs:16,69,388`, `quad_pipeline/cutout_tables.rs:31-35`.
  - Defect: the host owns a quad pipeline's `CutoutPlan`. `CutoutMode` is kept per device and per
    window with no check that the two agree, and it is chosen by `backend == Backend::Gl` inside a
    pipeline.
  - Target: the stream owns the plan and its mode comes from the tables. The tables decide by
    capability (whether `R32Float` allows `RENDER_ATTACHMENT`).
- [ ] `frame/submission.rs:38-64`, `wgpu_backend/mod.rs:429,475`.
  - Defect: `Submission` carries `plan` and `scissors`, which both say Full or Partial, kept in
    step by a `debug_assert`.
  - Target: scissors and clear, partial derived from the scissors, and the overlay's region on its
    own.
- [ ] `wgpu_backend/mod.rs:250-268,311,940-943`.
  - Defect: `ensure_format` compiles the whole `FormatPipelines` set inside the first submit of
    each format, so the first frame of each window stalls. Also, `disable_shadow_grid` clears the
    pipelines, and then `copy_backbuffer_to_surface`'s `expect` panics on a SkipCopy to a target
    that gives no copy.
  - Target: build the set at `note_target` or window open.
- [ ] `wgpu_backend/mod.rs:189,223,297,314,340,398,405,426-427`.
  - Defect: `new` takes the `Gpu` apart and builds it again. `Submission::clear()` and
    `dim_undamaged()` run twice per submit, `gpu_timings` is looked up three times, and `device()`
    repeats `gpu().device()`. `collect_gpu_stats` has three carriers, one of them the one-field
    `BackendConfig` in its own file.
  - Target: compute each once, and keep one carrier.
- [ ] `surface/surface_manager/mod.rs:23-25,219`, `.notes/PERF.md` Open.
  - Defect: PERF.md rejects a sampled swapchain image because it "turns off Metal's
    framebuffer-only path". But wgpu-hal 30.0.1 `metal/surface.rs:260` keeps framebuffer-only
    only when the usage is exactly `COLOR_TARGET`, and the surface already takes
    `COPY_DST | COPY_SRC`. The premise is false today.
  - Target: correct PERF.md. Spike a `TEXTURE_BINDING` surface whose stream caches one blur group
    per swapchain image, then measure against the 5 to 12 % that PERF.md records for skipping the
    copies on the M2.

### Batch 6: the backend's pass plan and one draw context

Files: `src/gpu/wgpu_backend/`, `src/gpu/pipeline/quad_pipeline/`, `src/gpu/frame/schedule/`.
Needs batch 5.

- [ ] `wgpu_backend/mod.rs:277-411,562-687`.
  - Defect: the pass plan is decided inline while encoding: the dim pre-pass, each segment's
    LoadOp, the breaks, copy-out against `draw_onto`, the overlay pass. The backend has no unit
    test.
  - Target: a pure `FramePasses::plan(..) -> [PassStep]`, tested like `Schedule`, and a thin
    executor.
- [ ] `wgpu_backend/mod.rs:614-860` `render_groups`.
  - Defect: each tenant's binding is built by hand:
    - `fmt.X.select(stencil)` appears 8 times;
    - the mark and debug-group triad appears 8 times;
    - `render_groups` has 6 parameters besides `self` and `run_main_segment` has 5;
    - `(from, barrier): (u32, Option<u32>)` is a tuple parameter, also in `Walk::run_between`.
  - Target: a `DrawCtx`, one wrapper for the mark and debug group, and a named `GroupSpan`.
- [ ] `wgpu_backend/mod.rs:783-791`, `quad_form/mod.rs:16-30`, `quad_pipeline/mod.rs:287-307,357-371`,
  `schedule/mod.rs:302-316`.
  - Defect: the backend branches on `run.key == QuadForm::Backdrop` to choose the bind method. The
    backdrop form is chosen by entry point, not by the `QUAD_FORM` override. One quad stream is
    split three times: shadows, forms, shadow entries.
  - Target: `QuadDraw { Form(QuadForm) | Shadow(ShadowEntry) }`, computed once per instance, and
    `QuadPipeline::bind_draw(key, groups)`.
- [ ] `wgpu_backend/mod.rs:10-12`.
  - Defect: the module doc says every pipeline "spells the same six steps"; text and icon do not.
  - Target: a doc that matches, or nothing.

### Batch 7: one owner of paint order in the composer

Files: `src/renderer/frontend/composer/` (`session.rs`, `mod.rs`, `rect_grid/`, `tile_index.rs`,
`occlusion_pruner.rs`, `higher_kind_rects.rs`, `pane_break.rs`), `src/renderer/frontend/paint_sink/`,
`src/renderer/frontend/payload/draw_quad_payload/`, `src/renderer/frontend/encoder/plane_ctx.rs`.
Batch 1 already fixes the bugs; this batch removes their cause.

- [ ] `session.rs` (1281 lines).
  - Defect: per-kind lowering is mixed with paint-order arbitration. Quad (806-815), text
    (760-793) and the tiers (`admit_higher_kind`, 1147-1162) each hand-write cull, order and
    register: `pane_break.paint` is called three times, and polyline clamps twice.
  - Target: `PaintOrder` with one `admit(lane, bounds)`. Text's cross-group batch stays the one
    documented exception. Per-kind lowering goes in its own files.
- [ ] `draw_quad_payload/mod.rs:118-123,270-272`, `session.rs:288,296,799,858-866`,
  `plane_ctx.rs:85-91`.
  - Defect: backdrop branches leak into the generic quad code. `DrawPanePayload` holds the
    backdrop twice (the `fill_axis` lanes and `backdrop`), and `faded` updates only the lanes.
    `push_quad(_, Option<Pane>)` and `paint_rect(.., pane: Option<(LoweredBackdrop, PaneKey)>, ..)`
    are `Option` parameters that select behavior, the second also a five-parameter fn with a tuple.
  - Target: the pane is its own payload, lowered to a `Quad` once.
- [ ] `tile_index.rs:110-131`, `rect_grid/mod.rs:43-51`.
  - Defect: `RectGrid` inserts a rect into every tile it spans. A viewport-wide curve bbox at 4K
    is 2040 inserts, and `OcclusionPruner` already bypasses large entries over the same
    `TileIndex`.
  - Target: the large-entry lane moves into `TileIndex` and serves both.
- [ ] `composer/mod.rs:166-174`.
  - Defect: four `BatchState` fields are reset one by one, and "`open_grid` non-empty ⇔
    `open.is_some()`" is not encoded anywhere.
  - Target: `BatchState::start_frame` and `close`.
- [ ] `paint_sink/mod.rs:107-180`.
  - Defect: eight identical fade → `is_noop` → forward gates. Polyline instances rely on a zeroed
    `fill_kind` that matches `FillKind::SOLID` (`session.rs:692-705,719-730`).
  - Target: one gate over a payload trait. Set `fill_kind` explicitly.
- [ ] `session.rs:660-677,691,712-713`.
  - Defect: `seg_colors` runs three times per segment (about 30k fades for a 10k-point polyline).
  - Target: once per segment, into `PolylineScratch`.
- [ ] Tests:
  - order and cull of turned draws;
  - a pane under an opaque quad;
  - the AA-fringe overlap.
  - `composer/bench.rs` gets quad, clip, rounded-clip, prune, polyline and pane cases.

### Batch 8: shader ownership and pipeline families

Files: `src/gpu/pipeline/` (all families, `shader_body/`, `format_pipelines.rs`),
`src/gpu/raster/raster_program.rs`, `raster_atlas/shader.wgsl`, `raster_quad.rs`,
`src/gpu/backdrop/backdrop_blur/`. Needs batch 6.

- [ ] `shader_body/mod.rs:3-18,36-129`.
  - Defect: one hub holds every shader's path and constants, which gives the cycles
    `quad_pipeline ⇄ shader_body`, `raster ⇄ shader_body` and `backdrop ⇄ shader_body`.
  - Target: each family owns `const SHADER` and `constants()`. `shader_body` keeps the prelude
    and `specialize_source`, and the list that the tests walk is one slice in
    `gpu/pipeline/mod.rs`.
- [ ] `format_pipelines.rs:9`, `raster_program.rs:185-188`.
  - Defect: `gpu::pipeline` and `gpu::raster` import each other. `RasterProgram` is a pipeline
    filed among the atlases.
  - Target: move it, with `RasterQuad` and its shader, to `gpu/pipeline/raster_pipeline/`.
- [ ] Each `*_ATTRS` (`quad_pipeline/mod.rs:437-463`, `curve_pipeline/mod.rs:137-168`,
  `image_pipeline/mod.rs:150-172`, `mesh_pipeline/mod.rs:174-210`, `raster_quad.rs:94-132`,
  `cutout_tables.rs:168-186`).
  - Defect: offsets packed by `vertex_attr_array!`, then checked against `offset_of!` by about 40
    hand asserts.
  - Target: attributes built from `offset_of!` in a `const`.
- [ ] `quad_pipeline/mod.rs:136-430`, `curve_pipeline/mod.rs:111-134`,
  `mesh_pipeline/mod.rs:103-160`, `image_pipeline/mod.rs:110-147`, `raster_pass.rs:258-279`.
  - Defect: five bind shapes and four upload shapes. The quad "upload" also records the cutout
    bake render pass.
  - Target: `InstancedPipeline<T: GpuInstance>`, and a `prepare_cutouts` step for the bake.
- [ ] `quad_pipeline/shader.wgsl:1097-1101` against `prelude.wgsl:90-94`.
  - Defect: `srgb_decode` is the prelude's `srgb_to_linear`.
  - Target: delete it, and move `srgb_encode` to the prelude.
- [ ] `shader.wgsl:1027-1045` against `1196-1215`.
  - Defect: `fs_backdrop` copies `fs`'s tail.
  - Target: one `shade_rect(in, fill, d)`.
- [ ] `quad_pipeline/shader.wgsl:6-7,642-652` against `curve_pipeline/shader.wgsl:59-60,418-434`.
  - Defect: the gradient bindings and the LUT read are written twice.
  - Target: a shared include for the shaders that bind the gradient group.
- [ ] `cutout_plan/mod.rs:428-475,520-532` against `shader.wgsl:519-529,1225-1250`.
  - Defect: `spread_radius` and the shadow box exist in Rust and in WGSL. The comment at line 519
    says "Only the shader applies it", which is false. `radius_rules_follow_the_shader` never runs
    the shader.
  - Target: fix the comment, and add a GPU cross-check of both copies on one table.
- [ ] `cutout_plan/mod.rs:225-229,293-317`, `cutout_tables.rs:133`.
  - Defect: one 16 B `CornerTables` per quad of the frame (+25 % over the 64 B quad stream) is
    uploaded whenever any shadow exists. In `Shade` mode it is all `NONE` and still uploaded.
  - Target: a compact per-shadow array indexed by the shadow's ordinal in `fill_lut_row`, and a
    static `NONE` stream in `Shade` mode.
- [ ] `shader.wgsl:436-454,607-631`, `quad_pipeline/mod.rs:417`, `pipeline_recipe.rs:89`.
  - Defect: nine fields re-bound to locals, narrating comments, and receiver-less associated fns
    that construct nothing.
  - Target: trim them, and make the two fns free fns.
- [ ] `blit_pipeline/mod.rs:64-65`.
  - Defect: the blit writes every texel with blending off but loads with `Clear`.
  - Target: `LoadOp::DontCare`, an invalidate on GLES.
- [ ] Tests:
  - the `QuadForm::of` table adds `Backdrop` and the shadow tags;
  - `curve_indices_tile_adjacent_cross_sections` (`curve_pipeline/mod.rs:177-200`) checks hand
    indices, not the generator's own formula;
  - remove `a_span_past_the_keys_panics` (`runs.rs:80-85`, std's bounds check) and
    `raster_quad_is_32_bytes` (already pinned by `hot_struct_sizes.rs:218`).

### Batch 9: the spin vocabulary

Files: `src/renderer/frontend/payload/spin.rs` and the payloads, `src/renderer/render_buffer/`
(`text_draw_row.rs`, `icon_draw_row.rs`), `src/gpu/raster/raster_pass.rs:18`,
`src/renderer/frontend/encoder/plane_ctx.rs:221-227`.

- [ ] Defect:
  - `render_buffer` and `gpu::raster` import `frontend::payload::spin::Spin`, which gives the cycle
    `payload ⇄ render_buffer` (`draw_image_payload.rs:8` → `render_buffer::image_flags`).
  - One type names a logical pivot in a payload and a physical pivot in a row.
  - The payloads use four pivot conventions, and the triangle is turned on the CPU.
- [ ] Target: one logical `Spin` (pivot relative to the payload origin) on every payload, the
  triangle included, and a physical `RowSpin` owned by `render_buffer`.

### Batch 10: present strategies

Files: `src/host/offscreen.rs:109-150`, `src/host/window_driver/mod.rs:197-216`,
`src/host/headless_window/mod.rs:95`, `benches/AGENTS.md:51-57`.

- [ ] Defect:
  - `retained_target(true)` says "small damage repaints straight into the target", but it maps to
    `DirectAdaptive`, which paints every partial frame below 0.4 coverage through the backbuffer
    and copies the whole surface out: 179 µs per partial frame on the 680M (PERF.md).
  - A direct full frame leaves the backbuffer stale, so the next partial frame becomes whole.
  - `HeadlessWindow` always sets the flag. The benches rely on it to emulate the winit host.
- [ ] Target: a retained strategy whose partial frame loads and paints the target itself, and an
  internals knob for the benches' swapchain emulation.

### Batch 11: smaller items

- [ ] `src/renderer/gradient_atlas/mod.rs:225-229` `grow`.
  - Defect: `mark_below(grown)` uploads every row, the zero rows too: 8 MiB in one frame for
    2048 → 4096. The comment's reason is inverted. The `counters.rs` doc of `rows_uploaded` is
    stale.
  - Target: a texture-to-texture copy of the old rows, which uploads nothing, and a correct doc.
- [ ] `frontend/mod.rs:43,79`, `composer/mod.rs:79`, `texture_limit/mod.rs:7-10`.
  - Defect: `TextureLimit` says it is "the one ceiling", but `Frontend` and `Composer` take a raw
    `NonZeroU32`. `TEST_MAX_TEXTURE_DIM` calls 8192 the downlevel default; that default is 2048,
    and 8192 is `Limits::default`.
  - Target: pass `TextureLimit`, and fix the doc.
- [ ] `composer/mod.rs:24-25`, `compose_rig.rs`.
  - Defect: a test and bench harness in its own gated module, which is none of AGENTS.md's four
    homes.
  - Target: move it to the composer's end-of-file `internals` or to `src/internals/`.
- [ ] `src/gpu/surface/owned_target/mod.rs:134-205` against `surface/capture.rs:67-246`.
  - Defect: `read_back` re-implements `Capture` (row padding, staging, swizzle). Texture to `UVec2`
    is written three times, and `Extent3d` is built by hand four times.
  - Target: `Capture` reads any `&Texture`, and `render_target::extent` everywhere.
- [ ] `src/gpu/resource/texture_region.rs`.
  - Defect: one caller, whose `first_row` is always 0. Its doc names a gradient-atlas user that no
    longer exists.
  - Target: inline it into `WgpuImageStore::write`.
- [ ] `src/primitives/paint/brush/gradient/color_ramp.rs:177`.
  - Defect: a test in `primitives` imports `renderer::gradient_atlas::bake`, an edge to the right.
  - Target: move the cross-check to `gradient_atlas/tests/bake.rs`.
- [ ] Stale references:
  - `prelude.wgsl:65` and `mesh_pipeline/shader.wgsl:14` cite an AGENTS.md "Color pipeline"
    section that does not exist;
  - `curve_pipeline/shader.wgsl:36` cites `render_buffer/curve.rs`, not `curve_instance.rs`;
  - `evict_one` is named in `text_backend/encode/cache/mod.rs:131`, `text_backend/bench.rs:45`,
    `text_backend/tests/mod.rs:22` and `raster_atlas/tests.rs:79`;
  - `mesh_pipeline/mod.rs:4` says the vertex stream is "content-stable", but it uploads every
    frame;
  - `gpu_ctx.rs:1-9` lists its fields;
  - the `FrameDraw.stats` doc (`wgpu_backend/mod.rs:167-170`) gives a reason that does not
    follow;
  - the `PresentStrategy` doc repeats `present_path`.
- [ ] `src/gpu/raster/raster_atlas/shader.wgsl:150-171`.
  - Defect: the color side stores straight alpha and premultiplies per tap, while
    `WgpuImageStore` premultiplies at upload.
  - Target: premultiply at upload.
- [ ] `src/gpu/test_gpu.rs:46-60`.
  - Defect: the lease keeps device and queue apart, and `handles()` clones a `Gpu` on each call.
  - Target: the lease holds a `Gpu`.
