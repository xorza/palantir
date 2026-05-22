use super::cmd_buffer::{
    CmdKind, DrawCurvePayload, DrawImagePayload, DrawMeshPayload, DrawPolylinePayload,
    DrawRectPayload, DrawShadowPayload, DrawTextPayload, PushClipPayload, RenderCmdBuffer,
};
use crate::common::frame_arena::FrameArenaInner;
use crate::layout::types::display::Display;
use crate::primitives::approx::EPS;
use crate::primitives::color::{Color, ColorF16, ColorU8};
use crate::primitives::image::ImageHandle;
use crate::primitives::{rect::Rect, transform::TranslateScale, urect::URect};
use crate::renderer::gradient_atlas::LutRow;
use crate::renderer::quad::{FillKind, Quad};
use crate::renderer::render_buffer::{
    CurveBatch, CurveInstance, DrawGroup, ImageBatch, ImageDrawRow, ImageInstance, MeshBatch,
    MeshDraw, MeshDrawRow, MeshInstance, RenderBuffer, RoundedClip, TextBatch, TextRun,
};
use crate::renderer::stroke_tessellate::{StrokeStyle, tessellate_polyline_aa};
use glam::{UVec2, Vec2};
use tinyvec::TinyVec;

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
/// and [`above_text_rects`](Self::above_text_rects) (per-group AABBs
/// of mesh/image/polyline draws that paint above text under
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
    /// Spatial index over the physical-px AABBs of text runs accumulated
    /// in the current text-batch (potentially across multiple groups).
    /// A new quad overlapping any of these would paint *under* the
    /// merged batch text — closes the batch (and flushes the group)
    /// so paint order is preserved. Cleared in [`Self::close_batch`].
    ///
    /// Backed by a viewport-tiled grid (see [`TextRectGrid`]). The
    /// grid replaces a flat `Vec<URect>` linear scan that dominated
    /// compose time in dense text UIs (~9% of frame on the resizing
    /// benchmark): batches grew to ~120 rects on average and every
    /// `quad_forces_flush` did a 4-axis AABB compare per rect. The
    /// grid lookup walks only the tiles the query rect overlaps and
    /// drops typical scan length to 1-4 candidates.
    text_grid: TextRectGrid,
    /// Per-group AABBs of draws that paint above text under the
    /// kind-reorder (mesh, image, polyline). Used by the intra-group
    /// text-after-X check: text recorded after a same-group higher-kind
    /// draw would be reordered above it on flush, so we force a flush
    /// when the new text overlaps. Cleared per flush — independent of
    /// batch state since every higher-kind draw also closes the batch.
    above_text_rects: Vec<URect>,
    /// In-flight group state. `*_start` cursors mark where the open
    /// group's `quads`/`texts`/`meshes` slice begins in `out`;
    /// [`Self::flush`] closes the slice and advances them.
    current_scissor: Option<URect>,
    current_rounded: Option<RoundedClip>,
    quads_start: u32,
    texts_start: u32,
    meshes_start: u32,
    images_start: u32,
    curves_start: u32,
    /// Bundled state for the currently-open text batch — `Some` while
    /// the composer is accumulating runs into a batch, `None`
    /// between batches. The rect scratch lives outside in
    /// `text_grid` so its tile vectors stay capacity-retained across
    /// open/close cycles (steady-state alloc-free).
    open_batch: Option<OpenBatch>,
    /// Solid-opaque no-stroke occluders in the in-flight group.
    /// Each entry pairs the quad's slice-relative index (for
    /// "drawn-on-top" ordering — only indices `> i` can occlude
    /// quad `i`) with its **cover rect**: the largest axis-aligned
    /// rect guaranteed to receive full opaque coverage. For
    /// sharp-cornered quads `cover == Quad.rect`; for rounded
    /// quads, `cover` is `Quad.rect` deflated per-side by
    /// `max(adjacent_radii) * (1 − 1/√2)` (the inscribed-square
    /// offset of a corner arc). Populated in the `DrawRect` push
    /// handler; consumed and cleared in `flush()`.
    opaque_in_group: Vec<Occluder>,
    /// Indices (relative to `quads_start`) of quads in the in-flight
    /// group marked for removal by the prune sweep. Sorted ascending
    /// by construction. Cleared at the end of each `flush()`.
    drop_indices: Vec<u32>,
    /// Prefix-max of `cover.size` (as a `Vec2` of `(w, h)`) over the
    /// tail of `opaque_in_group`, built once per flush.
    /// `prefix_max_cover[i]` = elementwise max over
    /// `opaque_in_group[i..]`. Lets the prune sweep reject an
    /// occludee with one size compare when no later occluder is
    /// large enough to contain it — turns the common "nested
    /// panels, child smaller than parent" case from O(N·K) into
    /// O(N + K).
    prefix_max_cover: Vec<Vec2>,
}

/// One entry in `Composer.opaque_in_group`. See the field docstring
/// for the cover-rect contract.
#[derive(Clone, Copy)]
struct Occluder {
    /// Index inside the in-flight group's quad slice
    /// (`out.quads[quads_start + idx]`).
    idx: u32,
    /// Largest axis-aligned rect with full opaque coverage. Used
    /// as the left-hand side of
    /// `Rect::contains_rect(occludee.painted)` in the prune sweep,
    /// where `painted = occludee.rect.inflated(stroke_width / 2)`.
    cover: Rect,
}

#[derive(Clone, Copy)]
struct ClipFrame {
    scissor: URect,
    rounded: Option<RoundedClip>,
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
        self.prune_occluded_quads(out);
        let q_end = out.quads.len() as u32;
        let t_end = out.texts.len() as u32;
        let m_end = out.meshes.rows.len() as u32;
        let i_end = out.images.rows.len() as u32;
        let c_end = out.curves.len() as u32;
        if q_end > self.quads_start
            || t_end > self.texts_start
            || m_end > self.meshes_start
            || i_end > self.images_start
            || c_end > self.curves_start
        {
            // Push the mesh/image batches BEFORE the group itself so
            // their `last_group` matches the in-flight group's
            // eventual index (= current `out.groups.len()`).
            if m_end > self.meshes_start {
                out.mesh_batches.push(MeshBatch {
                    meshes: (self.meshes_start..m_end).into(),
                    last_group: out.groups.len() as u32,
                });
            }
            if i_end > self.images_start {
                out.image_batches.push(ImageBatch {
                    images: (self.images_start..i_end).into(),
                    last_group: out.groups.len() as u32,
                });
            }
            if c_end > self.curves_start {
                out.curve_batches.push(CurveBatch {
                    instances: (self.curves_start..c_end).into(),
                    last_group: out.groups.len() as u32,
                });
            }
            out.groups.push(DrawGroup {
                scissor: self.current_scissor,
                rounded_clip: self.current_rounded,
                quads: (self.quads_start..q_end).into(),
                texts: (self.texts_start..t_end).into(),
            });
        }
        self.quads_start = q_end;
        self.texts_start = t_end;
        self.meshes_start = m_end;
        self.images_start = i_end;
        self.curves_start = c_end;
        self.above_text_rects.clear();
        self.opaque_in_group.clear();
        self.drop_indices.clear();
        self.prefix_max_cover.clear();
    }

    /// Drop quads in the in-flight group that are fully covered by a
    /// later opaque quad in the same group. Pure CPU prune — no
    /// pipeline / shader changes. See `docs/roadmap/occlusion-pruning.md`.
    ///
    /// Preconditions:
    /// - `out.quads[self.quads_start..]` is the in-flight group's
    ///   contiguous slice (composer's flush boundary contract).
    /// - `self.opaque_in_group` holds `Occluder` entries for every
    ///   solid-opaque quad pushed into the slice, in push order
    ///   (ascending `idx`). Each entry's `cover` is the largest
    ///   axis-aligned rect with full coverage — `Quad.rect` for
    ///   sharp corners, deflated by `KAPPA * max(adjacent_radii)`
    ///   per side for rounded ones. Stroke status is irrelevant on
    ///   this side (fill alone covers the interior).
    ///
    /// Behaviour:
    /// - For each quad at slice index `i`, compute its painted
    ///   extent as `q.rect.inflated(q.stroke_width / 2)` (centred
    ///   strokes spill outward; non-stroked inflate by zero). Drop
    ///   it if some occluder with `idx > i` (drawn on top) has
    ///   `cover.contains_rect(painted)`.
    /// - Shadows (`FillKind::is_shadow`) are never dropped — their
    ///   visual blur extends past the stored rect.
    /// - Compacts in place via `swap`-and-truncate; preserves the
    ///   relative order of survivors.
    fn prune_occluded_quads(&mut self, out: &mut RenderBuffer) {
        let start = self.quads_start as usize;
        if out.quads.len() - start < 2 || self.opaque_in_group.is_empty() {
            return;
        }
        let slice = &out.quads[start..];
        let occs = self.opaque_in_group.as_slice();

        // Prefix-max of cover dimensions over the tail of occs. After
        // this loop, `prefix_max_cover[i]` is the elementwise max
        // `(w, h)` over `occs[i..]`. Used below as a one-comparison
        // reject: if the occludee's painted rect is wider or taller
        // than every remaining cover, no `contains_rect` can succeed.
        self.prefix_max_cover.clear();
        self.prefix_max_cover.resize(occs.len(), Vec2::ZERO);
        let mut acc = Vec2::ZERO;
        for (i, occ) in occs.iter().enumerate().rev() {
            acc = acc.max(Vec2::new(occ.cover.size.w, occ.cover.size.h));
            self.prefix_max_cover[i] = acc;
        }

        self.drop_indices.clear();
        // Cursor into `occs` advancing in lockstep with `i`: it's
        // always positioned at the first occluder with `idx > i`.
        // Since `i` and `occs[*].idx` are both monotonically
        // ascending, the cursor only moves forward across the outer
        // loop — total work is O(N + K), not O(N·K).
        let mut cursor = 0;
        for (i, q) in slice.iter().enumerate() {
            // Shadows paint past the stored rect by blur sigma (no
            // closed-form extent we can test cheaply) — never drop.
            if q.fill_kind.is_shadow() {
                continue;
            }
            while cursor < occs.len() && occs[cursor].idx as usize <= i {
                cursor += 1;
            }
            // No later occluder exists for this `i` — and since
            // subsequent `i` values need even later occluders, none
            // can be covered. Done.
            if cursor >= occs.len() {
                break;
            }
            // Centred strokes paint outside the rect by
            // `stroke_width / 2` on every edge. Inflate the
            // occludee's painted extent for the containment test;
            // non-stroked quads inflate by zero. Rounded under-quads
            // share their bounding rect with the painted region, so
            // no corner-specific handling needed on this side.
            let painted = q.rect.inflated(q.stroke_width * 0.5);
            // Cheap reject: no remaining cover is large enough to
            // contain `painted` on at least one axis. This catches
            // the dominant "nested panels, parent larger than every
            // descendant" pattern without touching the inner loop.
            let max = self.prefix_max_cover[cursor];
            if painted.size.w > max.x || painted.size.h > max.y {
                continue;
            }
            for occ in &occs[cursor..] {
                if occ.cover.contains_rect(painted) {
                    self.drop_indices.push(i as u32);
                    break;
                }
            }
        }
        if self.drop_indices.is_empty() {
            return;
        }
        // Compact in place: walk forward, copy survivors down. The
        // drop list is sorted ascending by construction.
        let mut drop_iter = self.drop_indices.iter().copied().peekable();
        let mut write = start;
        for read in start..out.quads.len() {
            let rel = (read - start) as u32;
            if drop_iter.peek().copied() == Some(rel) {
                drop_iter.next();
                continue;
            }
            if read != write {
                out.quads[write] = out.quads[read];
            }
            write += 1;
        }
        out.quads.truncate(write);
    }

    /// Finalize the open text batch (if any): push a [`TextBatch`]
    /// entry covering `batch_texts_start..out.texts.len()` and clear
    /// batch-scoped scratch. No-op when no batch is active. Called
    /// at batch-split events — rounded-clip change, mesh/polyline
    /// append, or a quad that would paint under accumulated batch
    /// text. The id used by the just-pushed batch matches what
    /// [`Self::open_batch`] stamped on contributing groups.
    fn close_batch(&mut self, out: &mut RenderBuffer) {
        let Some(b) = self.open_batch.take() else {
            return;
        };
        let texts_end = out.texts.len() as u32;
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
        self.text_grid.clear();
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

    /// Force a flush / batch-close if a quad-tier draw at `overlap`
    /// overlaps something already in the group (or the open batch's
    /// text) that would be reordered above it. Quad is the lowest
    /// paint kind, so any higher-kind draw it overlaps would paint
    /// *under* it after the backend's intra-group reorder — flush to
    /// keep record order. Text overlap is checked against the whole
    /// open batch (which may span multiple groups); a hit also closes
    /// the batch so the merged text doesn't paint over this quad at
    /// end-of-batch. Coarse reject first against the batch's union
    /// AABB before scanning per-rect — common case is "quad far from
    /// any text," so the O(n) scan is wasted work without it.
    fn quad_forces_flush(&mut self, overlap: URect, out: &mut RenderBuffer) {
        let batch_text_hit = self
            .open_batch
            .as_ref()
            .is_some_and(|b| b.text_union.intersect(overlap).is_some())
            && self.text_grid.any_overlap(overlap);
        if batch_text_hit {
            self.close_batch(out);
            self.flush(out);
        } else if any_overlap(&self.above_text_rects, overlap) {
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
        out.meshes.clear();
        out.images.clear();
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
        self.above_text_rects.clear();
        self.current_scissor = None;
        self.current_rounded = None;
        self.quads_start = 0;
        self.texts_start = 0;
        self.meshes_start = 0;
        self.images_start = 0;
        self.curves_start = 0;
        self.open_batch = None;
        self.opaque_in_group.clear();
        self.drop_indices.clear();
        self.prefix_max_cover.clear();
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
                    let quad_urect = scissor_from_logical(world_rect, scale, snap, viewport_phys);
                    // Clip-cull: skip emitting the quad when it sits
                    // entirely outside the active scissor. The GPU
                    // would scissor it away anyway; this saves the
                    // `quads.push` + per-quad math.
                    if self.cull_against_active_clip(quad_urect) {
                        continue;
                    }
                    self.quad_forces_flush(quad_urect, out);
                    let world_radius = p.corners.scaled_by(current_transform.scale);
                    let phys_rect = world_rect.scaled_by(scale, snap);
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
                            let idx = out.quads.len() as u32 - 1 - self.quads_start;
                            self.opaque_in_group.push(Occluder { idx, cover });
                        }
                    }
                }
                CmdKind::DrawShadow => {
                    let p: DrawShadowPayload = cmds.read(start);
                    let world_rect = current_transform.apply_rect(p.rect);
                    let quad_urect = scissor_from_logical(world_rect, scale, snap, viewport_phys);
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
                    let phys_rect = world_rect.scaled_by(scale, snap);
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
                    // Mesh paints above text in the kind order. With
                    // text batching, the batch render emits at the END
                    // of its last group — past this mesh in schedule
                    // walk if the batch were left open. Close so the
                    // batch's text emits before this group's mesh and
                    // mesh-over-text is preserved.
                    self.close_batch(out);
                    let p: DrawMeshPayload = cmds.read(start);
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
                    if p.v_len > 0 {
                        // Inflate by 0.5 phys-px to match polyline's
                        // AA-fringe policy. Mesh today paints inside
                        // its vertex hull, but a future AA edge or
                        // displacement shader would silently produce
                        // false negatives — and false negatives in
                        // the overlap test reorder paint.
                        let world_bbox = current_transform.apply_rect(Rect {
                            min: p.bbox.min + p.origin,
                            size: p.bbox.size,
                        });
                        let min = world_bbox.min * scale;
                        let max = world_bbox.max() * scale;
                        let fringe = Vec2::splat(0.5);
                        self.above_text_rects.push(urect_from_phys(
                            min - fringe,
                            max + fringe,
                            viewport_phys,
                        ));
                    }
                }
                CmdKind::DrawImage => {
                    // Image sits above text in the kind order (same as
                    // mesh): close any open text batch so batched text
                    // emits before this group's images.
                    self.close_batch(out);
                    let p: DrawImagePayload = cmds.read(start);
                    let world_rect = current_transform.apply_rect(p.rect);
                    let image_urect = scissor_from_logical(world_rect, scale, snap, viewport_phys);
                    if self.cull_against_active_clip(image_urect) {
                        continue;
                    }
                    let phys_rect = world_rect.scaled_by(scale, snap);
                    let tint_color: Color = p.tint.into();
                    out.images.rows.push(ImageDrawRow {
                        // Composer doesn't need `size` (the encoder
                        // already resolved fit into `rect`+UV); pass
                        // a size-less handle so paint-side equality
                        // ignores the lane the registry uses for
                        // bookkeeping.
                        handle: ImageHandle {
                            id: p.handle,
                            size: glam::U16Vec2::ZERO,
                        },
                        instance: ImageInstance {
                            rect: phys_rect,
                            uv_min: p.uv_min,
                            uv_size: p.uv_size,
                            tint: tint_color.into(),
                            ..bytemuck::Zeroable::zeroed()
                        },
                    });
                    // Track for paint-order overlap with mesh-tier draws.
                    self.above_text_rects.push(image_urect);
                }
                CmdKind::DrawCurve => {
                    // Curve sits above text in the kind order (same as
                    // mesh/image): close any open text batch so batched
                    // text emits before this group's curves.
                    self.close_batch(out);
                    let p: DrawCurvePayload = cmds.read(start);
                    let width_phys = p.width * current_transform.scale * scale;
                    // Inflate the owner-local bbox by the AA fringe in
                    // *logical* px, then transform & cull. Same shape
                    // as the polyline path.
                    let world_bbox = current_transform.apply_rect(Rect {
                        min: p.bbox.min + p.origin,
                        size: p.bbox.size,
                    });
                    let inflate_phys = (width_phys * 0.5).max(0.5) + 0.5;
                    let inflate_logical = inflate_phys / scale;
                    let inflated = Rect {
                        min: world_bbox.min - Vec2::splat(inflate_logical),
                        size: crate::primitives::size::Size {
                            w: world_bbox.size.w + 2.0 * inflate_logical,
                            h: world_bbox.size.h + 2.0 * inflate_logical,
                        },
                    };
                    let bbox_scissor = scissor_from_logical(inflated, scale, false, viewport_phys);
                    if self.cull_against_active_clip(bbox_scissor) {
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
                    let color = crate::primitives::color::Color::from(p.color).into();
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
                    self.above_text_rects.push(bbox_scissor);
                }
                CmdKind::DrawPolyline => {
                    // Polyline tessellates to a mesh — same paint-order
                    // rule as DrawMesh. Close any open text batch
                    // before appending so batched text emits earlier.
                    self.close_batch(out);
                    let p: DrawPolylinePayload = cmds.read(start);
                    let mode = p.color_mode.get();
                    let cap = p.cap.get();
                    let join = p.join.get();
                    let width_phys = p.width * current_transform.scale * scale;

                    // Compute the inflated physical-px AABB once and
                    // reuse it for cull and overlap tracking. Encoder
                    // shipped a logical-px AABB over the cmd-buffer
                    // points; `TranslateScale` is uniform-scale so the
                    // transformed rect stays axis-aligned. Inflate
                    // by the tessellator's outer-fringe offset
                    // (`max(w/2, 0.5) + 0.5` in *phys* px) so the
                    // cull never trims a pixel the stroke would
                    // reach. Short-circuits before transforming the
                    // full point list — the win for long flattened
                    // curves.
                    let world_bbox = current_transform.apply_rect(Rect {
                        min: p.bbox.min + p.origin,
                        size: p.bbox.size,
                    });
                    let inflate_phys = (width_phys * 0.5).max(0.5) + 0.5;
                    let inflate_logical = inflate_phys / scale;
                    let inflated = Rect {
                        min: world_bbox.min - Vec2::splat(inflate_logical),
                        size: crate::primitives::size::Size {
                            w: world_bbox.size.w + 2.0 * inflate_logical,
                            h: world_bbox.size.h + 2.0 * inflate_logical,
                        },
                    };
                    let bbox_scissor = scissor_from_logical(inflated, scale, false, viewport_phys);
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
                            tint: crate::primitives::color::ColorU8::WHITE,
                            ..bytemuck::Zeroable::zeroed()
                        },
                    });
                    self.above_text_rects.push(bbox_scissor);
                }
                CmdKind::DrawText => {
                    let t: DrawTextPayload = cmds.read(start);
                    let world_rect = current_transform.apply_rect(t.rect);
                    // Glyphon clips per-`TextArea` against the run's own
                    // `bounds`, ignoring whatever `wgpu` scissor is active.
                    // Intersect with the active clip-stack top so ancestor
                    // `clip = true` panels actually clip glyphs; an empty
                    // intersection means the run can't reach pixels — skip
                    // the push entirely (cull).
                    let unclipped = scissor_from_logical(world_rect, scale, snap, viewport_phys);
                    let bounds = match self.clip_stack.last() {
                        Some(parent) => unclipped.clamp_to(parent.scissor),
                        None => unclipped,
                    };
                    if bounds.w == 0 || bounds.h == 0 {
                        continue;
                    }
                    // Text sits below mesh in the kind order — flush
                    // if any prior mesh in the group overlaps so this
                    // text doesn't get reordered above it. (No need
                    // to check quads: text paints over quads anyway.)
                    if any_overlap(&self.above_text_rects, bounds) {
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
                    let phys_rect = world_rect.scaled_by(scale, snap);
                    out.texts.push(TextRun {
                        origin: phys_rect.min,
                        bounds,
                        // Linear ColorU8 straight to the text backend.
                        // Palantir's native text shader (replaced glyphon,
                        // see `src/text_backend/`) consumes linear bytes
                        // and premultiplies at output — matching the rest
                        // of the renderer's pipelines. No sRGB roundtrip.
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

/// Physical-pixel size of one tile in [`TextRectGrid`]. Each text rect
/// is registered into every tile it overlaps; each
/// `quad_forces_flush` walks the tiles a quad covers and intersects
/// against per-tile rect lists. 64 px balances tile count (~4500 for a
/// 4K viewport, fits in L1) against per-tile rect count (typically 1-3
/// in dense UIs).
const TILE_SIZE: u32 = 64;

/// Per-tile inline capacity for the grid's index lists. Sized
/// empirically from the `frame/resizing` workload (dense UI at 32×
/// bench scale, viewport 3840×4800 phys px): observed max occupancy
/// was **3**. `N = 8` keeps every tile fully inline with substantial
/// headroom in any realistic UI — a 64-px tile holds 2-3 stacked
/// labels in a typical column.
///
/// `TinyVec` rather than `ArrayVec` to keep pathological text-dense
/// workloads (e.g. spreadsheet-grid layouts with tiny fonts and no
/// padding) functional rather than panicking. Once a tile spills to
/// the heap, its `clear()` between batches only resets `len`; the
/// heap buffer is retained, so a one-time allocation amortizes across
/// every subsequent frame. Steady-state alloc-free after warmup
/// holds.
type TileBucket = TinyVec<[u16; 8]>;

/// Spatial index over the open batch's text-rect AABBs. Replaces a
/// flat `Vec<URect>` linear scan that dominated compose time in
/// text-dense UIs. Backed by a row-major grid of tiles
/// ([`TILE_SIZE`] phys px); each rect lives in the tiles it covers,
/// each query walks only the tiles its rect overlaps and may visit a
/// rect twice for rects spanning >1 tile — fine, we early-exit on
/// first hit so duplicate visits cost only constant-factor false
/// positives.
#[derive(Default)]
struct TextRectGrid {
    cols: u32,
    rows: u32,
    /// Per-tile rect-index lists. Row-major: `tiles[ty * cols + tx]`.
    /// The outer `Vec` is retained across batches; each inner
    /// `TinyVec` is cleared (cheap, no dealloc) on
    /// [`Self::clear`].
    tiles: Vec<TileBucket>,
    /// Indices (into `tiles`) that received at least one `push` this
    /// frame — the set we walk on [`Self::clear`] instead of the full
    /// row-major grid. A tile is recorded the first time it
    /// transitions from empty to non-empty within a frame; subsequent
    /// pushes to the same tile skip the record. Capacity is retained
    /// across frames.
    ///
    /// Profiling motivation: `Composer::compose` was spending ~37% of
    /// its self-time clearing all ~4500 tiles every frame (4K viewport
    /// / 64-px tiles), even though only ~100-300 actually held
    /// anything in the bench fixture. Tracking touches drops the
    /// per-frame clear walk to the tiles we genuinely touched.
    touched: Vec<u32>,
    /// All rects inserted into the current batch, in insertion order.
    /// `tiles` stores indices into this vec.
    rects: Vec<URect>,
}

impl TextRectGrid {
    /// Reshape to cover `viewport` and reset all state. Called once
    /// per frame at compose start. Cheap when the viewport hasn't
    /// changed (no allocation — the outer `Vec` is already sized).
    fn start_frame(&mut self, viewport: UVec2) {
        let cols = viewport.x.div_ceil(TILE_SIZE).max(1);
        let rows = viewport.y.div_ceil(TILE_SIZE).max(1);
        let want = (cols * rows) as usize;
        // Grow-only — never shrink. A smaller-viewport frame reuses
        // the larger backing vector; tiles beyond the active grid
        // never get touched because `push` clamps indices to
        // `cols - 1` / `rows - 1`. `touched` stores absolute indices
        // into `tiles`, so `clear` works the same regardless of how
        // `cols × rows` map onto positions inside the vec.
        //
        // Profiling motivation: the resize-arm bench cycles through
        // 4 different viewports per frame. With unconditional
        // `tiles.clear()` + `resize_with(...)` the per-frame
        // `drop_in_place` sweep over every old TinyVec dominated
        // `Composer::compose` (~7% of the bench's CPU cycles).
        if want > self.tiles.len() {
            self.tiles.resize_with(want, TileBucket::default);
        }
        self.cols = cols;
        self.rows = rows;
        self.clear();
    }

    /// Drop every registered rect. Only walks the tiles that actually
    /// got pushed to this frame (`touched`), not the full row-major
    /// grid — `~100-300` tile clears in the dense-text fixture vs
    /// `~4500` on the full sweep.
    fn clear(&mut self) {
        for &i in &self.touched {
            self.tiles[i as usize].clear();
        }
        self.touched.clear();
        self.rects.clear();
    }

    /// Register `r`. No-op for zero-area input (degenerate text rects
    /// can't intersect anything anyway).
    fn push(&mut self, r: URect) {
        if r.w == 0 || r.h == 0 {
            return;
        }
        let idx = self.rects.len() as u16;
        self.rects.push(r);
        let max_x = self.cols - 1;
        let max_y = self.rows - 1;
        let cx0 = (r.x / TILE_SIZE).min(max_x);
        let cy0 = (r.y / TILE_SIZE).min(max_y);
        let cx1 = ((r.x + r.w - 1) / TILE_SIZE).min(max_x);
        let cy1 = ((r.y + r.h - 1) / TILE_SIZE).min(max_y);
        for ty in cy0..=cy1 {
            let row = ty * self.cols;
            for tx in cx0..=cx1 {
                let tile_idx = (row + tx) as usize;
                let tile = &mut self.tiles[tile_idx];
                // First touch this frame? Track for the next `clear`
                // so we don't have to walk the whole grid.
                let was_empty = tile.is_empty();
                tile.push(idx);
                if was_empty {
                    self.touched.push(tile_idx as u32);
                }
            }
        }
    }

    /// `true` if any registered rect intersects `q`. Returns on first
    /// hit. Walks every tile in `q`'s tile range and checks each
    /// tile's rect list — typical workload visits 1-4 tiles with 1-3
    /// rects each (avg total: ~4-8 intersect tests vs ~120 for the
    /// old flat scan).
    fn any_overlap(&self, q: URect) -> bool {
        if q.w == 0 || q.h == 0 || self.rects.is_empty() {
            return false;
        }
        let max_x = self.cols - 1;
        let max_y = self.rows - 1;
        let cx0 = (q.x / TILE_SIZE).min(max_x);
        let cy0 = (q.y / TILE_SIZE).min(max_y);
        let cx1 = ((q.x + q.w - 1) / TILE_SIZE).min(max_x);
        let cy1 = ((q.y + q.h - 1) / TILE_SIZE).min(max_y);
        for ty in cy0..=cy1 {
            let row = ty * self.cols;
            for tx in cx0..=cx1 {
                for &i in self.tiles[(row + tx) as usize].iter() {
                    if self.rects[i as usize].intersect(q).is_some() {
                        return true;
                    }
                }
            }
        }
        false
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

#[cfg(test)]
mod tests;
