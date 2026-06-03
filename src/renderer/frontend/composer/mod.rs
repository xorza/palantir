use crate::forest::frame_arena::FrameArenaInner;
use crate::layout::types::display::Display;
use crate::primitives::approx::EPS;
use crate::primitives::color::{Color, ColorF16, ColorU8};
use crate::primitives::paint::FillKind;
use crate::primitives::paint::LutRow;
use crate::primitives::{rect::Rect, size::Size, transform::TranslateScale, urect::URect};
use crate::renderer::frontend::cmd_buffer::{
    CmdKind, DrawCurvePayload, DrawImagePayload, DrawMeshPayload, DrawPolylinePayload,
    DrawRectPayload, DrawShadowPayload, DrawTextPayload, PushClipPayload, RenderCmdBuffer,
};
use crate::renderer::quad::Quad;
use crate::renderer::render_buffer::{
    CurveBatch, CurveInstance, DrawGroup, ImageBatch, ImageDrawRow, ImageInstance, MeshBatch,
    MeshDraw, MeshDrawRow, MeshInstance, RenderBuffer, RoundedClip, TextBatch, TextRun,
};
use crate::renderer::stroke_tessellate::{StrokeStyle, tessellate_polyline_aa};
use glam::{UVec2, Vec2};

mod occlusion;
mod text_grid;

use occlusion::OcclusionPruner;
use text_grid::TextRectGrid;

/// CPU-only compose engine: turns a `RenderCmdBuffer` stream into a `RenderBuffer`
/// (physical-px quads + text runs + scissor groups). Owns its output buffer
/// + compose-time scratch stacks so steady-state rendering is alloc-free.
///
/// Composer doesn't know about `Tree` or `encode` — it's pure algorithm +
/// scratch + output. [`Frontend`](crate::renderer::frontend::Frontend) orchestrates
/// encode + compose.
///
/// Render order *within* a group is fixed by the backend:
/// **quads → text → meshes**. That's safe iff for every prior draw of
/// a higher kind, no later draw of a lower kind overlaps it — a draw
/// that violates the rule forces a [`Self::flush`] so record order is
/// honored. The check uses
/// [`text_grid`](Self::text_grid) (per-batch text AABBs, spatially indexed)
/// and [`higher_kind_rects`](Self::higher_kind_rects) (per-group AABBs
/// of mesh/image/curve/polyline draws that paint above text under
/// kind-reorder).
#[derive(Default)]
pub(crate) struct Composer {
    /// Compose-time scratch — bounded by tree depth (typically <8).
    /// Pairs the resolved scissor with its rounded-clip data (when the
    /// push carried a non-zero radius); both ride together so a `PopClip`
    /// restores them as a unit.
    clip_stack: Vec<ClipFrame>,
    transform_stack: Vec<TranslateScale>,
    /// Scratch for `DrawPolyline`: transformed physical-px points
    /// fed to the stroke tessellator. Cleared per cmd, capacity
    /// reused — keeps steady-state alloc-free.
    polyline_scratch: Vec<Vec2>,
    /// Spatial index over the physical-px AABBs of the *open* text
    /// batch's runs (spans groups with its batch). A new quad overlapping
    /// any of these would paint *under* the merged batch text — closes
    /// the batch (and flushes the group) so paint order is preserved.
    /// Cleared in [`Self::close_batch`].
    ///
    /// Backed by a viewport-tiled grid (see [`TextRectGrid`]). The
    /// grid replaces a flat `Vec<URect>` linear scan that dominated
    /// compose time in dense text UIs (~9% of frame on the resizing
    /// benchmark): batches grew to ~120 rects on average and every
    /// `quad_forces_flush` did a 4-axis AABB compare per rect. The
    /// grid lookup walks only the tiles the query rect overlaps and
    /// drops typical scan length to 1-4 candidates.
    text_grid: TextRectGrid,
    /// Spatial index over text runs of batches **already closed within
    /// the in-flight group**. Such text still drains at its `last_group`
    /// (= this group), *after* the group's quads, so a later overlapping
    /// quad must flush even though the batch is no longer open
    /// ([`Self::text_grid`] tracks only the open one). Filled from each
    /// closing batch in [`Self::close_batch`]; cleared in [`Self::flush`]
    /// at the group boundary (closed batches have rendered by then).
    closed_text_grid: TextRectGrid,
    /// Per-group AABBs of draws that paint above both quads and text
    /// under the kind-reorder (mesh, image, curve, polyline). Used by
    /// two checks: a later quad overlapping one would be reordered
    /// *under* it (`quad_forces_flush`), and text recorded after one
    /// would be reordered *above* it (`DrawText`) — either forces a
    /// flush to preserve record order. Cleared per flush — independent
    /// of batch state since every higher-kind draw also closes the batch.
    higher_kind_rects: Vec<URect>,
    /// In-flight group state. `*_start` cursors mark where the open
    /// group's `quads`/`texts`/`meshes` slice begins in `out`;
    /// [`Self::flush`] closes the slice and advances them.
    current_scissor: Option<URect>,
    current_rounded: Option<RoundedClip>,
    /// `*_start` cursors marking where the open group's per-kind slices
    /// begin in `out`. [`Self::flush`] closes each slice and advances
    /// the matching cursor.
    cursors: GroupCursors,
    /// Bundled state for the currently-open text batch — `Some` while
    /// the composer is accumulating runs into a batch, `None`
    /// between batches. The rect scratch lives outside in
    /// `text_grid` so its tile vectors stay capacity-retained across
    /// open/close cycles (steady-state alloc-free).
    open_batch: Option<OpenBatch>,
    /// Per-group occlusion-prune scratch: the solid-opaque occluders
    /// pushed into the in-flight group and the sweep that drops earlier
    /// quads they fully cover. See [`OcclusionPruner`].
    occlusion: OcclusionPruner,
}

#[derive(Clone, Copy)]
struct ClipFrame {
    scissor: URect,
    rounded: Option<RoundedClip>,
}

/// Per-kind slice cursors for the in-flight group. Each field marks
/// where the open group's slice begins in the matching `out` buffer;
/// [`Composer::flush`] closes the slices and advances every cursor to
/// the buffer's current length. Bundled so the flush-boundary contract
/// is one value instead of five parallel fields.
#[derive(Default, Clone, Copy)]
struct GroupCursors {
    quads: u32,
    texts: u32,
    meshes: u32,
    images: u32,
    curves: u32,
}

/// State carried while a text batch is mid-accumulation. Pushed onto
/// `out.text_batches` as a [`TextBatch`] when [`Composer::close_batch`]
/// finalizes it.
#[derive(Clone, Copy)]
struct OpenBatch {
    /// Cursor into `out.texts` where this batch's run span begins.
    /// Combined with `out.texts.len()` at close-time to compute the
    /// finalized [`Span`].
    texts_start: u32,
    /// Index (into `out.groups`) of the last group whose text
    /// contributed to this batch. Refreshed on every text push (the
    /// in-flight group's eventual index is `out.groups.len()`).
    /// Tells the schedule where to emit the merged render step.
    last_group: u32,
    /// Union AABB of every rect in `Composer.text_grid` for this
    /// batch. The first reject for a new quad's overlap test — O(1)
    /// before falling through to the grid lookup.
    text_union: URect,
    /// `true` once a "strict" run has joined this batch — one whose
    /// ancestor clip cuts its full unclipped extent in X. The batch's
    /// GPU scissor (= `text_union`) must then stay equal to that
    /// strict bound; subsequent runs can only join if their `bounds`
    /// match exactly. Otherwise the merged scissor would let the
    /// strict run's glyphs paint past their intended clip (the text
    /// shader has no per-instance clip).
    strict: bool,
}

impl Composer {
    /// Close the in-flight group: if anything was emitted into it,
    /// push a `DrawGroup` covering the open slice; either way advance
    /// the per-kind cursors and clear the overlap scratches. Scissor
    /// + rounded clip are preserved for the next group.
    fn flush(&mut self, out: &mut RenderBuffer) {
        self.occlusion.prune(out, self.cursors.quads);
        let q_end = out.quads.len() as u32;
        let t_end = out.texts.len() as u32;
        let m_end = out.meshes.rows.len() as u32;
        let i_end = out.images.rows.len() as u32;
        let c_end = out.curves.len() as u32;
        if q_end > self.cursors.quads
            || t_end > self.cursors.texts
            || m_end > self.cursors.meshes
            || i_end > self.cursors.images
            || c_end > self.cursors.curves
        {
            // Push the mesh/image batches BEFORE the group itself so
            // their `last_group` matches the in-flight group's
            // eventual index (= current `out.groups.len()`).
            if m_end > self.cursors.meshes {
                out.mesh_batches.push(MeshBatch {
                    meshes: (self.cursors.meshes..m_end).into(),
                    last_group: out.groups.len() as u32,
                });
            }
            if i_end > self.cursors.images {
                out.image_batches.push(ImageBatch {
                    images: (self.cursors.images..i_end).into(),
                    last_group: out.groups.len() as u32,
                });
            }
            if c_end > self.cursors.curves {
                out.curve_batches.push(CurveBatch {
                    instances: (self.cursors.curves..c_end).into(),
                    last_group: out.groups.len() as u32,
                });
            }
            out.groups.push(DrawGroup {
                scissor: self.current_scissor,
                rounded_clip: self.current_rounded,
                quads: (self.cursors.quads..q_end).into(),
                texts: (self.cursors.texts..t_end).into(),
            });
        }
        self.cursors = GroupCursors {
            quads: q_end,
            texts: t_end,
            meshes: m_end,
            images: i_end,
            curves: c_end,
        };
        self.higher_kind_rects.clear();
        self.occlusion.clear();
        // Closed-batch text is group-scoped: once we cross a group
        // boundary, any batch closed *in* this group has rendered (it
        // drains at its `last_group`), so its rects no longer gate quads.
        // The open-batch grid is NOT cleared here — it spans groups with
        // its (still-open) batch.
        self.closed_text_grid.clear();
    }

    /// Finalize the open text batch (if any): push a [`TextBatch`]
    /// entry covering `batch_texts_start..out.texts.len()`. No-op when no
    /// batch is active. Called at batch-split events — rounded-clip
    /// change, mesh/polyline append, or a strict-bounds mismatch. The
    /// text-overlap grid is NOT cleared here (it's group-scoped, cleared
    /// in `flush`) so a later quad still flushes for text in an
    /// already-closed batch that shares this group.
    fn close_batch(&mut self, out: &mut RenderBuffer) {
        let Some(b) = self.open_batch.take() else {
            return;
        };
        let texts_end = out.texts.len() as u32;
        // Carry this batch's text rects into the group-scoped closed grid
        // so a later quad sharing the group still flushes for them (they
        // drain at `last_group` = this group, *after* the group's quads).
        // Then reset the open-batch grid for the next batch.
        for ti in b.texts_start..texts_end {
            self.closed_text_grid.push(out.texts[ti as usize].bounds);
        }
        self.text_grid.clear();
        // Invariants the schedule cursor relies on: batches are pushed
        // in walk order so `last_group` is monotonically non-decreasing
        // (multiple batches can anchor to the same group when a mesh
        // splits mid-group), and their `texts` spans concatenate
        // without gaps in `out.texts`.
        debug_assert!(
            out.text_batches
                .last()
                .is_none_or(|prev| prev.last_group <= b.last_group),
        );
        debug_assert!(
            out.text_batches
                .last()
                .is_none_or(|prev| prev.texts.start + prev.texts.len == b.texts_start),
        );
        out.text_batches.push(TextBatch {
            texts: (b.texts_start..texts_end).into(),
            last_group: b.last_group,
            // `text_union` is already in physical pixels and clamped
            // to every contributing run's clip-stack-narrowed bounds.
            // Hand it through as the GPU scissor for this batch — the
            // schedule was previously widening to the full viewport
            // here and relying on per-run shader clipping that the
            // inlined text backend doesn't actually implement.
            scissor: b.text_union,
        });
    }

    /// Return a mutable handle to the open batch, opening a fresh one
    /// when none exists. Idempotent within a batch — repeated calls
    /// reuse the same `OpenBatch` and only refresh `last_group` to
    /// the in-flight group's eventual index.
    fn open_batch(&mut self, out: &RenderBuffer) -> &mut OpenBatch {
        let b = self.open_batch.get_or_insert(OpenBatch {
            texts_start: out.texts.len() as u32,
            last_group: 0,
            text_union: URect::default(),
            strict: false,
        });
        b.last_group = out.groups.len() as u32;
        b
    }

    /// `true` when `bounds` doesn't intersect the active scissor — the
    /// caller should skip emission. Identical reject shape at every
    /// shape-draw site; centralising it keeps each handler from
    /// growing its own variant.
    fn cull_against_active_clip(&self, bounds: URect) -> bool {
        self.current_scissor
            .is_some_and(|s| bounds.intersect(s).is_none())
    }

    /// Cull a higher-kind (mesh / image / curve) draw against the active
    /// clip and, if it survives, close any open text batch. Higher-kind
    /// geometry paints above text under the backend's kind reorder, and a
    /// batch renders at the END of its last group — past this draw if left
    /// open — so the batch must close here for its text to emit first. Done
    /// only after the cull: a culled draw must not split the batch. Returns
    /// `false` when culled — the caller should `continue`.
    ///
    /// Polyline doesn't use this: its close must wait until after
    /// tessellation (an empty tessellation must not split the batch), so it
    /// culls early via [`Self::cull_against_active_clip`] and closes by hand.
    fn enter_higher_kind(&mut self, scissor: URect, out: &mut RenderBuffer) -> bool {
        if self.cull_against_active_clip(scissor) {
            return false;
        }
        self.close_batch(out);
        true
    }

    /// Force a flush / batch-close if a quad-tier draw at `overlap`
    /// overlaps something in the group that would be reordered above it.
    /// Quad is the lowest paint kind, so any higher-kind draw it overlaps
    /// would paint *under* it after the backend's intra-group reorder —
    /// flush to keep record order. Text overlap is checked against both
    /// the open batch ([`Self::text_grid`], which may span groups) and
    /// batches already closed in this group ([`Self::closed_text_grid`]);
    /// an open-batch hit additionally closes the batch so its text can't
    /// coalesce forward and re-cover this quad. Both checks go straight to
    /// the tiled grid — `any_overlap` early-exits on an empty grid, so no
    /// separate union pre-reject is needed.
    fn quad_forces_flush(&mut self, overlap: URect, out: &mut RenderBuffer) {
        // Text painted in (or scheduled after) this group sits in two
        // places: the open batch (`text_grid`, spans groups with its
        // batch) and batches already closed within this group
        // (`closed_text_grid`). A quad overlapping either would be painted
        // *under* that text by the backend's quads→text order, so flush so
        // the text renders first.
        //
        // An open-batch hit additionally *closes* the batch: leaving it
        // open would let the overlapping run coalesce forward and schedule
        // at a later `last_group`, re-covering this quad. A closed-grid
        // hit needs no close — that text's batch is already finalized at
        // this group; flushing alone puts the quad in the next group.
        if self.text_grid.any_overlap(overlap) {
            self.close_batch(out);
            self.flush(out);
        } else if self.closed_text_grid.any_overlap(overlap)
            || any_overlap(&self.higher_kind_rects, overlap)
        {
            self.flush(out);
        }
    }

    /// Switch to a new clip (scissor + optional rounded), flushing
    /// the in-flight group only if anything actually differs. A
    /// same-clip Push/Pop is a no-op so accumulated overlap state
    /// persists through redundant clip transitions.
    fn set_clip(
        &mut self,
        scissor: Option<URect>,
        rounded: Option<RoundedClip>,
        out: &mut RenderBuffer,
    ) {
        if rounded != self.current_rounded {
            // Stencil ref is tied to the active rounded clip; batched
            // text under the wrong mask would either over- or
            // under-clip. Close before the group transition.
            self.close_batch(out);
        }
        if scissor != self.current_scissor || rounded != self.current_rounded {
            self.flush(out);
            self.current_scissor = scissor;
            self.current_rounded = rounded;
        }
    }

    /// Consume a logical-px command stream → physical-px `Quad`s +
    /// `TextRun`s + draw groups (scissor ranges) into the caller-
    /// provided `out` buffer. Pure: no device, no queue.
    ///
    /// Gradient atlas registration happens at shape-lowering time
    /// (upstream of this stage), so each `DrawRectPayload` carries a
    /// pre-resolved `fill_lut_row`; nothing here touches the atlas.
    #[profiling::function]
    pub(crate) fn compose(
        &mut self,
        cmds: &RenderCmdBuffer,
        arena: &mut FrameArenaInner,
        display: Display,
        out: &mut RenderBuffer,
    ) {
        let scale = display.scale_factor;
        let snap = display.pixel_snap;
        let viewport_phys = display.physical;
        let viewport_phys_f = viewport_phys.as_vec2();

        out.quads.clear();
        out.texts.clear();
        out.meshes.rows.clear();
        out.images.rows.clear();
        out.groups.clear();
        out.text_batches.clear();
        out.mesh_batches.clear();
        out.image_batches.clear();
        out.curves.clear();
        out.curve_batches.clear();
        out.has_rounded_clip = false;
        out.viewport_phys = viewport_phys;
        out.viewport_phys_f = viewport_phys_f;
        out.scale = scale;

        self.clip_stack.clear();
        self.transform_stack.clear();
        self.text_grid.start_frame(viewport_phys);
        self.closed_text_grid.start_frame(viewport_phys);
        self.higher_kind_rects.clear();
        self.current_scissor = None;
        self.current_rounded = None;
        self.cursors = GroupCursors::default();
        self.open_batch = None;
        self.occlusion.clear();
        let mut current_transform = TranslateScale::IDENTITY;

        for i in 0..cmds.kinds.len() {
            let kind = cmds.kinds[i];
            let start = cmds.starts[i];
            match kind {
                CmdKind::PushClip => {
                    let p: PushClipPayload = cmds.read(start);
                    let logical_radius = (!p.corners.approx_zero()).then_some(p.corners);
                    let world = current_transform.apply_rect(p.rect);
                    let me = scissor_from_logical(world, scale, snap, viewport_phys);
                    let scissor = match self.clip_stack.last() {
                        Some(parent) => me.clamp_to(parent.scissor),
                        None => me,
                    };
                    let rounded = if let Some(logical_radius) = logical_radius {
                        // Combine current transform's uniform scale with DPR
                        // so radii match the painted SDF's physical size.
                        let phys_scale = current_transform.scale * scale;
                        // `mask_rect` stays unclamped — the SDF needs the
                        // rect's true edges, otherwise corner curves
                        // would shift inward when the clip partially
                        // leaves the viewport.
                        out.has_rounded_clip = true;
                        Some(RoundedClip {
                            mask_rect: world.scaled_by(scale, snap),
                            corners: logical_radius.scaled_by(phys_scale),
                        })
                    } else {
                        // Rect clip nested inside a rounded ancestor: inherit
                        // the ancestor's rounded data so children stay
                        // stencil-tested against the active mask. Without
                        // this, the child group would draw with ref=0 over
                        // pixels already stenciled to 1 by the parent's
                        // mask, and the stencil_test pipeline would discard
                        // every fragment.
                        self.clip_stack.last().and_then(|f| f.rounded)
                    };
                    self.clip_stack.push(ClipFrame { scissor, rounded });
                    self.set_clip(Some(scissor), rounded, out);
                }
                CmdKind::PopClip => {
                    self.clip_stack
                        .pop()
                        .expect("PopClip without matching PushClip");
                    let parent = self.clip_stack.last().copied();
                    self.set_clip(
                        parent.map(|f| f.scissor),
                        parent.and_then(|f| f.rounded),
                        out,
                    );
                }
                CmdKind::PushTransform => {
                    let t: TranslateScale = cmds.read(start);
                    self.transform_stack.push(current_transform);
                    current_transform = current_transform.compose(t);
                }
                CmdKind::PopTransform => {
                    current_transform = self
                        .transform_stack
                        .pop()
                        .expect("PopTransform without matching PushTransform");
                }
                CmdKind::DrawRect => {
                    let p: DrawRectPayload = cmds.read(start);
                    let world_rect = current_transform.apply_rect(p.rect);
                    // Scale to physical px once: the cull `URect` and the
                    // emitted quad share this rect (the cull needs the
                    // scaled bounds anyway, so a culled draw costs the same).
                    let phys_rect = world_rect.scaled_by(scale, snap);
                    let quad_urect = urect_from_phys(phys_rect.min, phys_rect.max(), viewport_phys);
                    // Clip-cull: skip emitting the quad when it sits
                    // entirely outside the active scissor. The GPU
                    // would scissor it away anyway; this saves the
                    // `quads.push` + per-quad math.
                    if self.cull_against_active_clip(quad_urect) {
                        continue;
                    }
                    self.quad_forces_flush(quad_urect, out);
                    let world_radius = p.corners.scaled_by(current_transform.scale);
                    let phys_radius = world_radius.scaled_by(scale);
                    let stroke_width_phys = p.stroke_width * current_transform.scale * scale;
                    out.quads.push(Quad {
                        rect: phys_rect,
                        fill: p.fill,
                        corners: phys_radius,
                        stroke_color: p.stroke_color,
                        stroke_width: stroke_width_phys,
                        fill_kind: p.fill_kind,
                        fill_lut_row: p.fill_lut_row,
                        fill_axis: p.fill_axis,
                    });
                    // Occlusion-prune annotation: a solid-opaque
                    // quad fully covers a sub-rect of its bounding
                    // rect — for sharp corners the cover is the
                    // whole rect; for rounded corners it's the
                    // inscribed rect deflated by KAPPA·radius per
                    // side. Strokes don't shrink coverage (a centred
                    // stroke paints OUTSIDE the rect; the interior
                    // is still fully covered by the fill), so
                    // stroke_width is irrelevant on this side.
                    // Record the cover rect with the in-flight slice
                    // index so `flush()` can drop earlier quads
                    // contained in it.
                    if p.fill_kind == FillKind::SOLID && p.fill.is_opaque() {
                        let cover = phys_rect.inscribed_for_corners(phys_radius);
                        if !cover.is_paint_empty() {
                            let idx = out.quads.len() as u32 - 1 - self.cursors.quads;
                            self.occlusion.record_opaque(idx, cover);
                        }
                    }
                }
                CmdKind::DrawShadow => {
                    let p: DrawShadowPayload = cmds.read(start);
                    let world_rect = current_transform.apply_rect(p.rect);
                    let phys_rect = world_rect.scaled_by(scale, snap);
                    let quad_urect = urect_from_phys(phys_rect.min, phys_rect.max(), viewport_phys);
                    if self.cull_against_active_clip(quad_urect) {
                        continue;
                    }
                    // Shadow quads use a 2σ-deflated rect for the
                    // overlap check: the outer 2σ rim of a Gaussian
                    // contributes <5% alpha and is visually
                    // indistinguishable from the background, so we
                    // shouldn't force a batch flush for that ring.
                    // Keeps adjacent text in the same batch when a
                    // soft drop shadow sits 1–2σ away from text.
                    let sigma_phys =
                        p.fill_axis.lanes()[2].max(0.0) * current_transform.scale * scale;
                    let overlap_urect = quad_urect.deflated((2.0 * sigma_phys) as u32);
                    self.quad_forces_flush(overlap_urect, out);
                    let world_radius = p.corners.scaled_by(current_transform.scale);
                    let phys_radius = world_radius.scaled_by(scale);
                    // Shadow params (offset, σ) are logical-px scalars;
                    // scale to physical px so the shader's `local`
                    // coords line up.
                    let fill_axis = p.fill_axis.scaled(current_transform.scale * scale);
                    out.quads.push(Quad {
                        rect: phys_rect,
                        fill: p.color,
                        corners: phys_radius,
                        stroke_color: ColorF16::TRANSPARENT,
                        stroke_width: 0.0,
                        fill_kind: p.fill_kind,
                        fill_lut_row: LutRow::FALLBACK,
                        fill_axis,
                    });
                }
                CmdKind::DrawMesh => {
                    let p: DrawMeshPayload = cmds.read(start);
                    // `draw_mesh` already gated empty/degenerate meshes
                    // (`DrawMeshPayload::is_noop`), so `v_len >= 1` here.
                    // Inflate by 0.5 phys-px to match polyline's AA-fringe
                    // policy. Mesh today paints inside its vertex hull,
                    // but a future AA edge or displacement shader would
                    // silently produce false negatives — and false
                    // negatives in the overlap test reorder paint. The
                    // same inflated rect feeds the clip cull below.
                    let world_bbox = current_transform.apply_rect(Rect {
                        min: p.bbox.min + p.origin,
                        size: p.bbox.size,
                    });
                    // Mesh skips snapping (matches polyline/curve); route
                    // through the shared scaler so the cull tracks `DrawRect`
                    // instead of open-coding `* scale`.
                    let phys_bbox = world_bbox.scaled_by(scale, false);
                    let fringe = Vec2::splat(0.5);
                    let mesh_urect = urect_from_phys(
                        phys_bbox.min - fringe,
                        phys_bbox.max() + fringe,
                        viewport_phys,
                    );
                    // Clip-cull + batch-close: a mesh fully outside the
                    // active scissor (e.g. scrolled out of an ancestor clip)
                    // is skipped; a surviving one closes the open text batch
                    // so its text emits before this above-text geometry.
                    if !self.enter_higher_kind(mesh_urect, out) {
                        continue;
                    }
                    // Verts already live in FrameArena owner-local;
                    // span passes through to `MeshDraw` verbatim. The
                    // per-instance translate folds in both the owner
                    // origin and the active push-transform stack so the
                    // shader produces physical coords. Phase 1's
                    // transform/tint move plus this slice eliminates
                    // both the per-vertex CPU multiply and the
                    // per-frame vertex copy.
                    let phys_scale = current_transform.scale * scale;
                    let phys_translate = (current_transform.scale * p.origin
                        + current_transform.translation)
                        * scale;
                    let tint_color: Color = p.tint.into();
                    out.meshes.rows.push(MeshDrawRow {
                        draw: MeshDraw {
                            vertices: (p.v_start..p.v_start + p.v_len).into(),
                            indices: (p.i_start..p.i_start + p.i_len).into(),
                        },
                        instance: MeshInstance {
                            translate: phys_translate,
                            scale: phys_scale,
                            tint: tint_color.into(),
                            ..bytemuck::Zeroable::zeroed()
                        },
                    });
                    self.higher_kind_rects.push(mesh_urect);
                }
                CmdKind::DrawImage => {
                    let p: DrawImagePayload = cmds.read(start);
                    let world_rect = current_transform.apply_rect(p.rect);
                    let phys_rect = world_rect.scaled_by(scale, snap);
                    let image_urect =
                        urect_from_phys(phys_rect.min, phys_rect.max(), viewport_phys);
                    // Clip-cull + batch-close: image sits above text in the
                    // kind order (same as mesh), so a surviving draw closes
                    // the open text batch first.
                    if !self.enter_higher_kind(image_urect, out) {
                        continue;
                    }
                    let tint_color: Color = p.tint.into();
                    out.images.rows.push(ImageDrawRow {
                        // Just the registration id — the backend looks it
                        // up in its texture cache; the encoder already
                        // resolved fit into `rect` + UV.
                        id: p.handle,
                        instance: ImageInstance {
                            rect: phys_rect,
                            uv_min: p.uv_min,
                            uv_size: p.uv_size,
                            tint: tint_color.into(),
                            tiled: p.tiled,
                            ..bytemuck::Zeroable::zeroed()
                        },
                    });
                    // Track for paint-order overlap with mesh-tier draws.
                    self.higher_kind_rects.push(image_urect);
                }
                CmdKind::DrawCurve => {
                    let p: DrawCurvePayload = cmds.read(start);
                    let width_phys = p.width * current_transform.scale * scale;
                    // Inflate the owner-local bbox by the stroke's AA
                    // fringe, transform to physical px, then cull. Same
                    // bound the polyline path uses.
                    let bbox_scissor = stroke_bbox_scissor(
                        current_transform,
                        p.bbox,
                        p.origin,
                        width_phys,
                        scale,
                        viewport_phys,
                    );
                    // Clip-cull + batch-close: curve sits above text in the
                    // kind order (same as mesh/image), so a surviving draw
                    // closes the open text batch first.
                    if !self.enter_higher_kind(bbox_scissor, out) {
                        continue;
                    }
                    // Transform control points to physical px. Owner
                    // origin folds in here so the record stays
                    // owner-local (cross-frame stable). No pixel
                    // snapping — snapping control points would warp
                    // the curve shape; AA fringe lives in the shader.
                    let xform = |q: Vec2| current_transform.apply_point(q + p.origin) * scale;
                    let p0 = xform(p.p0);
                    let p1 = xform(p.p1);
                    let p2 = xform(p.p2);
                    let p3 = xform(p.p3);
                    // Adaptive sub-instance count from post-transform
                    // control-polygon length. Polygon length bounds
                    // arc length from above — slight overshoot, but
                    // never undershoots → no faceting from too-coarse
                    // sampling. Shader bakes `SEGMENTS_PER_INSTANCE`
                    // chord-subdivisions per instance; aim for ~1.5 px
                    // chord per segment so AA fringe (0.5 px) fully
                    // covers any sub-pixel kink.
                    let l = (p1 - p0).length() + (p2 - p1).length() + (p3 - p2).length();
                    let target_chord_px = 1.5_f32;
                    let total_segments = (l / target_chord_px).ceil().max(1.0) as u32;
                    let n = total_segments
                        .div_ceil(SEGMENTS_PER_INSTANCE)
                        .clamp(1, MAX_SUB_INSTANCES);
                    let color = Color::from(p.color).into();
                    let fill_kind = p.fill_kind;
                    let fill_lut_row = p.fill_lut_row;
                    let inv_n = 1.0 / n as f32;
                    for i in 0..n {
                        let t0 = i as f32 * inv_n;
                        let t1 = if i + 1 == n {
                            1.0
                        } else {
                            (i + 1) as f32 * inv_n
                        };
                        out.curves.push(CurveInstance {
                            p0,
                            p1,
                            p2,
                            p3,
                            t0,
                            t1,
                            width: width_phys,
                            color,
                            cap: p.cap,
                            fill_kind,
                            fill_lut_row,
                            ..bytemuck::Zeroable::zeroed()
                        });
                    }
                    self.higher_kind_rects.push(bbox_scissor);
                }
                CmdKind::DrawPolyline => {
                    let p: DrawPolylinePayload = cmds.read(start);
                    let mode = p.color_mode.get();
                    let cap = p.cap.get();
                    let join = p.join.get();
                    let width_phys = p.width * current_transform.scale * scale;

                    // Compute the inflated physical-px AABB once and
                    // reuse it for cull and overlap tracking. Inflating
                    // by the tessellator's outer fringe means the cull
                    // never trims a pixel the stroke would reach, and it
                    // short-circuits before transforming the full point
                    // list — the win for long flattened curves.
                    let bbox_scissor = stroke_bbox_scissor(
                        current_transform,
                        p.bbox,
                        p.origin,
                        width_phys,
                        scale,
                        viewport_phys,
                    );
                    if self.cull_against_active_clip(bbox_scissor) {
                        continue;
                    }

                    let pts_start = p.points_start as usize;
                    let pts_end = pts_start + p.points_len as usize;
                    let cs_start = p.colors_start as usize;
                    let cs_end = cs_start + p.colors_len as usize;
                    let src_points = &arena.polyline_points[pts_start..pts_end];
                    let src_colors = &arena.polyline_colors[cs_start..cs_end];

                    // Transform points into physical-px. Owner-local
                    // origin is folded in here so points stay owner-
                    // local in the arena (cross-frame stable). No
                    // pixel-snap — snapping stroke verts shifts thin
                    // lines off-axis. Hairline regime (<1 phys px)
                    // handled inside the tessellator.
                    self.polyline_scratch.clear();
                    self.polyline_scratch.extend(
                        src_points
                            .iter()
                            .map(|&q| current_transform.apply_point(q + p.origin) * scale),
                    );

                    let phys_v_start = arena.meshes.vertices.len() as u32;
                    let phys_i_start = arena.meshes.indices.len() as u32;
                    tessellate_polyline_aa(
                        &self.polyline_scratch,
                        src_colors,
                        StrokeStyle {
                            mode,
                            cap,
                            join,
                            width_phys,
                        },
                        &mut arena.meshes.vertices,
                        &mut arena.meshes.indices,
                    );
                    let v_len = arena.meshes.vertices.len() as u32 - phys_v_start;
                    let i_len = arena.meshes.indices.len() as u32 - phys_i_start;
                    if v_len == 0 {
                        continue;
                    }
                    // Polyline tessellates to a mesh — same paint-order
                    // rule as DrawMesh: close any open text batch so its
                    // text emits before this group's mesh. Only now that
                    // the polyline will actually emit geometry — an
                    // empty or culled polyline must not split the batch.
                    self.close_batch(out);
                    // Polyline points are pre-transformed to physical-px
                    // on CPU (the tessellator needs phys-px width to pick
                    // its AA fringe), so the shader's per-instance state
                    // is identity. Tint is white — colors live per-vertex.
                    out.meshes.rows.push(MeshDrawRow {
                        draw: MeshDraw {
                            vertices: (phys_v_start..phys_v_start + v_len).into(),
                            indices: (phys_i_start..phys_i_start + i_len).into(),
                        },
                        instance: MeshInstance {
                            translate: Vec2::ZERO,
                            scale: 1.0,
                            tint: ColorU8::WHITE,
                            ..bytemuck::Zeroable::zeroed()
                        },
                    });
                    self.higher_kind_rects.push(bbox_scissor);
                }
                CmdKind::DrawText => {
                    let t: DrawTextPayload = cmds.read(start);
                    let world_rect = current_transform.apply_rect(t.rect);
                    // Scale once: `unclipped` (overlap/cull bounds) and the
                    // emitted run's `origin` both derive from this rect.
                    let phys_rect = world_rect.scaled_by(scale, snap);
                    // Glyphon clips per-`TextArea` against the run's own
                    // `bounds`, ignoring whatever `wgpu` scissor is active.
                    // Intersect with the active clip-stack top so ancestor
                    // `clip = true` panels actually clip glyphs; an empty
                    // intersection means the run can't reach pixels — skip
                    // the push entirely (cull).
                    let unclipped = urect_from_phys(phys_rect.min, phys_rect.max(), viewport_phys);
                    let bounds = match self.clip_stack.last() {
                        Some(parent) => unclipped.clamp_to(parent.scissor),
                        None => unclipped,
                    };
                    if bounds.w == 0 || bounds.h == 0 {
                        continue;
                    }
                    // Text sits below mesh/image/curve/polyline in the
                    // kind order — flush if any prior higher-kind draw in
                    // the group overlaps so this text doesn't get
                    // reordered above it. (No need to check quads: text
                    // paints over quads anyway.)
                    if any_overlap(&self.higher_kind_rects, bounds) {
                        self.flush(out);
                    }
                    // Batch GPU scissor = `text_union` (union of every
                    // run's `bounds` in the batch). The text shader has
                    // no per-instance clip, so a "strict" run — one
                    // whose ancestor clip cuts the unclipped extent —
                    // can only batch with peers whose `bounds` matches
                    // exactly; anything wider would let the strict
                    // run's glyphs paint past their intended clip.
                    // Non-strict-with-non-strict coalesces freely.
                    let new_strict = bounds != unclipped;
                    if let Some(b) = self.open_batch.as_ref()
                        && (b.strict || new_strict)
                        && b.text_union != bounds
                    {
                        self.close_batch(out);
                    }
                    // open_batch must run BEFORE the text push so the
                    // batch's `texts_start` captures this run's index.
                    let b = self.open_batch(out);
                    b.text_union = b.text_union.union(bounds);
                    b.strict |= new_strict;
                    out.texts.push(TextRun {
                        origin: phys_rect.min,
                        bounds,
                        // Linear ColorU8 straight to the text backend.
                        // Palantir's native text shader (see
                        // `src/renderer/backend/text/`) consumes linear
                        // bytes and premultiplies at output — matching
                        // the rest of the renderer's pipelines. No sRGB
                        // roundtrip.
                        color: ColorU8::from(Color::from(t.color)),
                        key: t.key,
                        // Snap the ancestor-transform component of the
                        // text scale to discrete 2.5% steps. Continuous
                        // zoom would otherwise mint a fresh glyphon
                        // cache key every frame (subpixel font size +
                        // bin shift), forcing swash to re-rasterize
                        // every glyph. Snapping stabilizes the key
                        // across small zoom deltas so the atlas hits.
                        // Quads/meshes keep continuous scale — only
                        // text glyph crispness "steps."
                        scale: snap_text_scale(current_transform.scale),
                    });
                    self.text_grid.push(bounds);
                }
            }
        }
        self.close_batch(out);
        self.flush(out);
    }
}

/// Chord-subdivisions per curve sub-instance. The shader expands one
/// instance into this many quads (= 2× this many triangles = 6× this
/// many indices). Has to stay in lockstep with the `SEGMENTS_PER_INSTANCE`
/// constant in `curve.wgsl`.
pub(crate) const SEGMENTS_PER_INSTANCE: u32 = 16;

/// Upper bound on sub-instances per curve. Long, fast-curving strokes
/// (think a 4k-px-long swooping bezier at 200% zoom) hit this cap;
/// beyond it the chord error rises but stays well under a pixel for
/// any realistic UI workload. Cap is a sanity belt — far above the
/// 1–4 sub-instance steady state.
const MAX_SUB_INSTANCES: u32 = 256;

/// Additive step on the text-scale ladder. Same step in *scale units*
/// across the range, so the step in *percent of current size* shrinks
/// as zoom grows (0.025/4 ≈ 0.6% at 4×, 0.025/1 = 2.5% at 1×, 0.025/0.5
/// = 5% at 0.5×). The user-perceptual case for this layout: at high
/// zoom every percent of size change is visible, so we want fine steps;
/// at low zoom text is small and crispness stepping doesn't matter, so
/// coarse steps + fewer atlas keys is the right trade.
///
/// **Geometric note.** Measurement uses the unscaled `font_size_px`
/// (`TextShaper::measure`) — only the paint-time scale is snapped. At a
/// non-rung zoom level the rendered glyph block is up to `STEP/2`
/// wider/narrower than the layout-space rect it nominally fills. The
/// extra width is clipped at `TextRun.bounds`, and the cascade
/// inflates text damage rects by the same fraction so a rung-jump
/// between consecutive frames repaints all affected pixels (see
/// `forest::shapes::record::text_paint_bbox_local`).
///
/// Sourced from [`crate::text::TEXT_SCALE_STEP`] so the cascade's
/// inflation and the composer's snap stay locked in step.
const TEXT_SCALE_STEP: f32 = crate::text::TEXT_SCALE_STEP;

/// Snap the ancestor-transform component of a text run's scale to the
/// additive 2.5% ladder. Identity is preserved exactly so non-zoom UIs
/// stay on the trivial path. See call-site comment in `DrawText` for
/// rationale.
fn snap_text_scale(s: f32) -> f32 {
    if (s - 1.0).abs() < EPS {
        return 1.0;
    }
    (s / TEXT_SCALE_STEP).round() * TEXT_SCALE_STEP
}

/// Conservative overlap test: any non-empty intersection counts.
/// False positives are correctness-safe (extra flush, costs a
/// drawcall); false negatives would reorder paint and corrupt the
/// frame.
fn any_overlap(slots: &[URect], r: URect) -> bool {
    slots.iter().any(|s| s.intersect(r).is_some())
}

/// Clamp a physical-px AABB to the viewport, returning the
/// non-negative `URect` the GPU can consume. NaN/non-finite inputs
/// collapse to `URect::default()` (zero-sized).
///
/// Floor on min, ceil on max — so unsnapped float inputs (curve/
/// polyline bbox with `snap=false`) expand outward to fully cover
/// their source rect. For snapped inputs the edges are already
/// integer floats so floor == ceil and behavior is unchanged.
/// Under-bounding the bbox would feed false-negatives to overlap
/// tracking (paint reorder) and cull (dropped paints).
fn urect_from_phys(min: Vec2, max: Vec2, viewport: UVec2) -> URect {
    if !(min.x.is_finite() && min.y.is_finite() && max.x.is_finite() && max.y.is_finite()) {
        return URect::default();
    }
    let x = (min.x.max(0.0) as u32).min(viewport.x);
    let y = (min.y.max(0.0) as u32).min(viewport.y);
    let right = (max.x.max(0.0).ceil() as u32).min(viewport.x);
    let bottom = (max.y.max(0.0).ceil() as u32).min(viewport.y);
    URect {
        x,
        y,
        w: right.saturating_sub(x),
        h: bottom.saturating_sub(y),
    }
}

fn scissor_from_logical(r: Rect, scale: f32, snap: bool, viewport: UVec2) -> URect {
    let phys = r.scaled_by(scale, snap);
    urect_from_phys(phys.min, phys.max(), viewport)
}

/// Physical-px scissor for a stroked shape's owner-local `bbox`. Folds
/// `origin` + the active transform into world space, inflates by the
/// tessellator's outer AA-fringe (`max(width_phys/2, 0.5) + 0.5` phys
/// px, expressed back in logical units), then clamps to the viewport.
/// Shared by the curve and polyline paths so their cull / overlap bound
/// can't drift. `snap = false` — snapping a stroke bbox would shift thin
/// lines off-axis; `urect_from_phys` floors/ceils to fully cover it.
fn stroke_bbox_scissor(
    xform: TranslateScale,
    bbox: Rect,
    origin: Vec2,
    width_phys: f32,
    scale: f32,
    viewport: UVec2,
) -> URect {
    let world_bbox = xform.apply_rect(Rect {
        min: bbox.min + origin,
        size: bbox.size,
    });
    let inflate_phys = (width_phys * 0.5).max(0.5) + 0.5;
    let inflate_logical = inflate_phys / scale;
    let inflated = Rect {
        min: world_bbox.min - Vec2::splat(inflate_logical),
        size: Size {
            w: world_bbox.size.w + 2.0 * inflate_logical,
            h: world_bbox.size.h + 2.0 * inflate_logical,
        },
    };
    scissor_from_logical(inflated, scale, false, viewport)
}

#[cfg(test)]
mod tests;
