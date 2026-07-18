use crate::display::Display;
use crate::primitives::approx::{EPS, noop_f32};
use crate::primitives::brush::FillAxis;
use crate::primitives::color::{ColorF16, ColorU8};
use crate::primitives::corners::Corners;
use crate::primitives::fill_wire::FillKind;
use crate::primitives::fill_wire::LutRow;
use crate::primitives::spacing::Spacing;
use crate::primitives::span::Span;
use crate::primitives::{rect::Rect, transform::TranslateScale, urect::URect};
use crate::record_store::RecordPayloads;
use crate::renderer::frontend::cmd_buffer::{Command, RenderCmdBuffer};
use crate::renderer::quad::{AA_RADIUS, Quad};
use crate::renderer::render_buffer::batch::{DrawGroup, GroupBatch, PaintTier, TextBatch};
use crate::renderer::render_buffer::curve::{
    CURVE_KIND_ARC, CURVE_KIND_CUBIC, CURVE_KIND_JOIN_BEVEL, CURVE_KIND_JOIN_MITER,
    CURVE_KIND_JOIN_ROUND, CURVE_KIND_SEGMENT, CurveInstance, HALF_FRINGE, MITER_LIMIT,
    SEGMENTS_PER_INSTANCE, cap_lanes, stroked_bbox,
};
use crate::renderer::render_buffer::image::{ImageDrawRow, ImageInstance, RenderTargetDraw};
use crate::renderer::render_buffer::mesh::{MeshDraw, MeshDrawRow, MeshInstance};
use crate::renderer::render_buffer::text::TextRun;
use crate::renderer::render_buffer::{MAX_ROUNDED_CLIP_DEPTH, RenderBuffer, RoundedClip};
use crate::shape::{ColorMode, LineCap, LineJoin};
use glam::{UVec2, Vec2};
use std::num::NonZeroU32;

mod higher_kind;
mod occlusion;
mod text_grid;

use higher_kind::HigherKindRects;
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
/// **quads → text → meshes → images → curves**
/// (`schedule::emit_group_body`; polylines ride the curve tier as
/// segment + join-chrome instances). That
/// reorder is safe iff no overlapping pair of draws swaps its record
/// order — two rules, both enforced by forcing a [`Self::flush`]:
/// a *lower*-kind draw must not follow an overlapping higher-kind draw
/// in the same group (it would replay under it), and a *higher*-kind
/// draw must not follow an overlapping higher-kind draw of a
/// later-replaying kind (e.g. a mesh recorded after an overlapping
/// image or curve). The checks use
/// the batch state's open text grid (per-batch text AABBs, spatially indexed)
/// and [`higher_kinds`](Self::higher_kinds) (per-group, per-tier
/// AABBs of mesh/image/curve draws).
#[derive(Debug)]
pub(crate) struct Composer {
    /// Compose-time scratch — bounded by tree depth (typically <8).
    /// Pairs the resolved scissor with its rounded-mask chain; both
    /// ride together so a `PopClip` restores them as a unit.
    clip_stack: Vec<ClipFrame>,
    transform_stack: Vec<TranslateScale>,
    polyline: PolylineScratch,
    batch: BatchState,
    /// Per-group AABBs partitioned by above-text replay tier. A later
    /// lower-tier draw checks only tiers that replay after it, while
    /// text and quads use the aggregate union before scanning any set.
    /// Cleared per flush — independent of batch state since every
    /// higher-kind draw also closes the batch.
    higher_kinds: HigherKindRects,
    /// In-flight group clip state: the active scissor + rounded-mask
    /// chain stamped onto the group at [`Self::flush`]. Changed only
    /// through [`Self::set_clip`], which flushes when either differs
    /// (chains compare by value, so a pop/re-push of an identical
    /// rounded clip stays a no-op).
    current_scissor: Option<URect>,
    current_chain: Span,
    /// `*_start` cursors marking where the open group's per-kind slices
    /// begin in `out`. [`Self::flush`] closes each slice and advances
    /// the matching cursor.
    cursors: GroupCursors,
    /// Per-group occlusion-prune scratch: the solid-opaque occluders
    /// pushed into the in-flight group and the sweep that drops earlier
    /// quads they fully cover. See [`OcclusionPruner`].
    occlusion: OcclusionPruner,
    /// Device `max_texture_dimension_2d`, the cap on a `GpuView` off-screen
    /// target's size — the composer uniformly downsamples each composited
    /// `GpuView` whose physical rect exceeds it. Fixed for the device's
    /// lifetime, so it rides the ctor, not every compose.
    max_texture_dim: NonZeroU32,
}

#[derive(Clone, Copy, Debug)]
struct ClipFrame {
    scissor: URect,
    /// Outer→inner chain of rounded masks active for this frame's
    /// subtree — a span into `RenderBuffer.rounded_clips`. A rounded
    /// push extends the parent chain with its own mask; a rect push
    /// inherits it verbatim. Empty = no rounded ancestor.
    chain: Span,
}

/// One closed-but-not-yet-indexed text batch: its run span in `out.texts`
/// plus the union AABB of those runs' bounds.
#[derive(Clone, Copy, Debug)]
struct PendingClosedBatch {
    texts: Span,
    union: URect,
}

#[derive(Debug, Default)]
struct PolylineScratch {
    points: Vec<Vec2>,
    kept: Vec<u32>,
    directions: Vec<Vec2>,
}

/// Allocation-owning state for text batching. The open grid may span groups;
/// the closed grid and pending list are cleared at each group boundary.
#[derive(Debug, Default)]
struct BatchState {
    open: Option<OpenBatch>,
    open_grid: TextRectGrid,
    closed_grid: TextRectGrid,
    pending: Vec<PendingClosedBatch>,
}

/// Per-kind slice cursors for the in-flight group. Each field marks
/// where the open group's slice begins in the matching `out` buffer;
/// [`Composer::flush`] closes the slices and advances every cursor to
/// the buffer's current length. Bundled so the flush-boundary contract
/// is one value instead of five parallel fields. `texts` feeds only the
/// did-anything-emit check — a text-only group must still push a
/// `DrawGroup` so its batch's `last_group` index resolves; the run
/// spans themselves live on [`TextBatch`].
#[derive(Default, Clone, Copy, Debug)]
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
#[derive(Clone, Copy, Debug)]
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
    /// Union AABB of every rect in the batch state's open grid for this
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
    /// New composer capped at the device's `max_texture_dimension_2d` (the
    /// `GpuView` target-size ceiling). All scratch starts empty.
    pub(crate) fn new(max_texture_dim: u32) -> Self {
        Self {
            clip_stack: Vec::new(),
            transform_stack: Vec::new(),
            polyline: PolylineScratch::default(),
            batch: BatchState::default(),
            higher_kinds: HigherKindRects::default(),
            current_scissor: None,
            current_chain: Span::default(),
            cursors: GroupCursors::default(),
            occlusion: OcclusionPruner::default(),
            max_texture_dim: NonZeroU32::new(max_texture_dim)
                .expect("composer texture dimension limit must be positive"),
        }
    }

    /// Close the in-flight group: if anything was emitted into it,
    /// push a `DrawGroup` covering the open slice; either way advance
    /// the per-kind cursors and clear the overlap scratches. Scissor
    /// + rounded clip are preserved for the next group.
    fn flush(&mut self, out: &mut RenderBuffer) {
        self.occlusion.prune(out, self.cursors.quads);
        let q_end = out.quads.len() as u32;
        let t_end = out.texts.len() as u32;
        let m_end = out.meshes.len() as u32;
        let i_end = out.images.len() as u32;
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
                out.mesh_batches.push(GroupBatch {
                    items: (self.cursors.meshes..m_end).into(),
                    last_group: out.groups.len() as u32,
                });
            }
            if i_end > self.cursors.images {
                out.image_batches.push(GroupBatch {
                    items: (self.cursors.images..i_end).into(),
                    last_group: out.groups.len() as u32,
                });
            }
            if c_end > self.cursors.curves {
                out.curve_batches.push(GroupBatch {
                    items: (self.cursors.curves..c_end).into(),
                    last_group: out.groups.len() as u32,
                });
            }
            out.groups.push(DrawGroup {
                scissor: self.current_scissor,
                rounded_clips: self.current_chain,
                quads: (self.cursors.quads..q_end).into(),
            });
        }
        self.cursors = GroupCursors {
            quads: q_end,
            texts: t_end,
            meshes: m_end,
            images: i_end,
            curves: c_end,
        };
        self.higher_kinds.clear();
        self.occlusion.clear();
        // Closed-batch text is group-scoped: once we cross a group
        // boundary, any batch closed *in* this group has rendered (it
        // drains at its `last_group`), so its rects no longer gate quads.
        // The open-batch grid is NOT cleared here — it spans groups with
        // its (still-open) batch.
        self.batch.closed_grid.clear();
        self.batch.pending.clear();
    }

    /// Finalize the open text batch (if any): push a [`TextBatch`]
    /// entry covering `batch_texts_start..out.texts.len()`. No-op when no
    /// batch is active. Called at batch-split events — rounded-clip
    /// change, a higher-kind append, or a strict-bounds mismatch. The
    /// batch also lands on the batch state's pending list (group-scoped,
    /// cleared in `flush`) so a later quad still flushes for text in an
    /// already-closed batch that shares this group — the grid fill is
    /// deferred to [`Self::closed_hit`].
    fn close_batch(&mut self, out: &mut RenderBuffer) {
        let Some(b) = self.batch.open.take() else {
            return;
        };
        let texts_end = out.texts.len() as u32;
        // Record the batch for the group-scoped closed check, then
        // reset the open-batch grid for the next batch.
        self.batch.pending.push(PendingClosedBatch {
            texts: (b.texts_start..texts_end).into(),
            union: b.text_union,
        });
        self.batch.open_grid.clear();
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
            // Every close site runs before `current_chain` can change
            // (set_clip closes ahead of the update), so this is the
            // chain all the batch's runs were recorded under.
            rounded_clips: self.current_chain,
        });
    }

    /// Return a mutable handle to the open batch, opening a fresh one
    /// when none exists. Idempotent within a batch — repeated calls
    /// reuse the same `OpenBatch` and only refresh `last_group` to
    /// the in-flight group's eventual index.
    fn open_batch(&mut self, out: &RenderBuffer) -> &mut OpenBatch {
        let b = self.batch.open.get_or_insert(OpenBatch {
            texts_start: out.texts.len() as u32,
            last_group: 0,
            text_union: URect::default(),
            strict: false,
        });
        b.last_group = out.groups.len() as u32;
        b
    }

    /// `true` when `bounds` has no viewport area or doesn't intersect
    /// the active scissor — the caller should skip emission. Identical
    /// reject shape at every shape-draw site; centralising it keeps each
    /// handler from growing its own variant.
    fn cull_bounds(&self, bounds: URect) -> bool {
        bounds.w == 0
            || bounds.h == 0
            || self
                .current_scissor
                .is_some_and(|s| bounds.intersect(s).is_none())
    }

    /// Cull a higher-kind (mesh / image / curve) draw against the active
    /// clip and, if it survives, close any open text batch. Higher-kind
    /// geometry paints above text under the backend's kind reorder, and a
    /// batch renders at the END of its last group — past this draw if left
    /// open — so the batch must close here for its text to emit first. Done
    /// only after the cull: a culled draw must not split the batch. Also
    /// flushes the group when the draw cross-kind-conflicts with an earlier
    /// higher-kind draw (see [`HigherKindRects::conflicts`]), and then
    /// records the draw's own rect for the group's overlap tracking (after
    /// the flush, so it isn't wiped with the previous group's rects).
    /// Returns `false` when culled — the caller should `continue`.
    ///
    /// Polyline calls this only after its kept-point walk proves the
    /// stroke emits geometry (an all-coincident polyline must not split
    /// the batch), gated behind an early [`Self::cull_bounds`].
    fn enter_higher_kind(
        &mut self,
        tier: PaintTier,
        scissor: URect,
        out: &mut RenderBuffer,
    ) -> bool {
        if self.cull_bounds(scissor) {
            return false;
        }
        self.close_batch(out);
        if self.higher_kinds.conflicts(tier, scissor) {
            self.flush(out);
        }
        self.higher_kinds.push(tier, scissor);
        true
    }

    /// Conservative overlap of `rect` against every recorded higher-kind
    /// draw, kind-blind: any non-empty intersection counts. False
    /// positives are correctness-safe (extra flush, costs a drawcall);
    /// false negatives would reorder paint and corrupt the frame.
    fn any_higher_kind_overlap(&self, rect: URect) -> bool {
        self.higher_kinds.any_overlap(rect)
    }

    /// Force a flush / batch-close if a quad-tier draw at `overlap`
    /// overlaps something in the group that would be reordered above it.
    /// Quad is the lowest paint kind, so any higher-kind draw it overlaps
    /// would paint *under* it after the backend's intra-group reorder —
    /// flush to keep record order. Text overlap is checked against both
    /// the open batch's grid (which may span groups) and
    /// batches already closed in this group ([`Self::closed_hit`]);
    /// an open-batch hit additionally closes the batch so its text can't
    /// coalesce forward and re-cover this quad. The open check goes
    /// straight to the tiled grid — `any_overlap` pre-rejects on its
    /// internal union AABB, so no caller-side pre-reject is needed.
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
        if self.batch.open_grid.any_overlap(overlap) {
            self.close_batch(out);
            self.flush(out);
        } else if self.closed_hit(overlap, out) || self.any_higher_kind_overlap(overlap) {
            self.flush(out);
        }
    }

    /// `true` if `q` overlaps text of a batch closed within the
    /// in-flight group. Batches land on the batch state's pending list as
    /// span + union at close time (O(1)); the first query whose `q`
    /// hits a pending union drains *all* pending batches into
    /// the closed grid and every later query is a grid
    /// lookup. Groups nothing probes near closed text never pay the
    /// per-rect fill.
    fn closed_hit(&mut self, q: URect, out: &RenderBuffer) -> bool {
        if !self.batch.pending.is_empty()
            && self
                .batch
                .pending
                .iter()
                .any(|b| b.union.intersect(q).is_some())
        {
            for b in self.batch.pending.drain(..) {
                for ti in b.texts.range() {
                    self.batch.closed_grid.push(out.texts[ti].bounds);
                }
            }
        }
        self.batch.closed_grid.any_overlap(q)
    }

    /// Switch to a new clip (scissor + rounded-mask chain), flushing
    /// the in-flight group only if anything actually differs. Chains
    /// compare by value, so a same-clip Push/Pop is a no-op and
    /// accumulated overlap state persists through redundant clip
    /// transitions.
    fn set_clip(&mut self, scissor: Option<URect>, chain: Span, out: &mut RenderBuffer) {
        let chain_changed = !chains_equal(out, chain, self.current_chain);
        if chain_changed {
            // The stencil mask stack is tied to the active chain;
            // batched text under the wrong masks would either over- or
            // under-clip. Close before the group transition (while
            // `current_chain` still names the batch's chain).
            self.close_batch(out);
        }
        if scissor != self.current_scissor || chain_changed {
            self.flush(out);
            self.current_scissor = scissor;
            self.current_chain = chain;
        }
    }

    /// Consume a logical-px command stream → physical-px `Quad`s +
    /// `TextRun`s + draw groups (scissor ranges) into the caller-
    /// provided `out` buffer. Pure: no device, no queue.
    ///
    /// Gradient atlas registration happens at shape-lowering time
    /// (upstream of this stage), so each draw-rect payload carries a
    /// pre-resolved `fill_lut_row`; nothing here touches the atlas.
    #[profiling::function]
    pub(crate) fn compose(
        &mut self,
        cmds: &RenderCmdBuffer,
        payloads: &RecordPayloads,
        display: Display,
        out: &mut RenderBuffer,
    ) {
        let scale = display.scale_factor;
        let snap = display.pixel_snap;
        let viewport_phys = display.physical;

        out.start_frame(display);

        self.reset_group_scratch(viewport_phys);
        self.clip_stack.clear();
        self.transform_stack.clear();
        self.current_scissor = None;
        self.current_chain = Span::default();
        let mut current_transform = TranslateScale::IDENTITY;

        for command in cmds.iter() {
            match command {
                Command::PushClip(p) => {
                    let logical_radius = (!p.corners.approx_zero()).then_some(p.corners);
                    let world = current_transform.apply_rect(p.rect);
                    let me = scissor_from_logical(world, scale, snap, viewport_phys);
                    let parent = self.clip_stack.last().copied();
                    let scissor = match parent {
                        Some(parent) => me.clamp_to(parent.scissor),
                        None => me,
                    };
                    let parent_chain = parent.map_or(Span::default(), |f| f.chain);
                    let chain = if let Some(logical_radius) = logical_radius {
                        // Combine current transform's uniform scale with DPR
                        // so radii match the painted SDF's physical size.
                        let phys_scale = current_transform.scale * scale;
                        // `mask_rect` stays unclamped — the SDF needs the
                        // rect's true edges, otherwise corner curves
                        // would shift inward when the clip partially
                        // leaves the viewport.
                        let rc = RoundedClip {
                            mask_rect: world.scaled_by(scale, snap),
                            corners: logical_radius.scaled_by(phys_scale),
                        };
                        // A rounded push nested in rounded ancestors
                        // STACKS: child chain = ancestor chain + own
                        // mask, copied so every chain is one contiguous
                        // span the stencil path can stamp outer→inner.
                        // Re-pushing the innermost mask verbatim adds no
                        // depth (a redundant stamp would test/write the
                        // same pixels).
                        if out.rounded_clips[parent_chain.range()].last() == Some(&rc) {
                            parent_chain
                        } else {
                            let depth = parent_chain.len + 1;
                            if depth > MAX_ROUNDED_CLIP_DEPTH {
                                rounded_clip_depth_overflow(depth);
                            }
                            let chain_start = out.rounded_clips.len() as u32;
                            out.rounded_clips.extend_from_within(parent_chain.range());
                            out.rounded_clips.push(rc);
                            Span::new(chain_start, depth)
                        }
                    } else {
                        // Rect clip nested inside rounded ancestors: inherit
                        // the ancestor chain so children stay stencil-tested
                        // against the active masks. Without this, the child
                        // group would draw with ref=0 over pixels already
                        // stenciled nonzero by the ancestors' masks, and the
                        // stencil_test pipeline would discard every fragment.
                        parent_chain
                    };
                    self.clip_stack.push(ClipFrame { scissor, chain });
                    self.set_clip(Some(scissor), chain, out);
                }
                Command::PopClip => {
                    self.clip_stack
                        .pop()
                        .expect("PopClip without matching PushClip");
                    let parent = self.clip_stack.last().copied();
                    self.set_clip(
                        parent.map(|f| f.scissor),
                        parent.map_or(Span::default(), |f| f.chain),
                        out,
                    );
                }
                Command::PushTransform(t) => {
                    self.transform_stack.push(current_transform);
                    current_transform = current_transform.compose(t);
                }
                Command::PopTransform => {
                    current_transform = self
                        .transform_stack
                        .pop()
                        .expect("PopTransform without matching PushTransform");
                }
                Command::DrawRect(p) => {
                    let world_rect = current_transform.apply_rect(p.rect);
                    // Scale to physical px once: the cull `URect` and the
                    // emitted quad share this rect (the cull needs the
                    // scaled bounds anyway, so a culled draw costs the same).
                    let phys_rect = world_rect.scaled_by(scale, snap);
                    // Clear fold: an opaque solid sharp unclipped quad
                    // covering the whole viewport paints exactly what
                    // `LoadOp::Clear(fill)` would — every covered pixel
                    // is deep inside the SDF (coverage exactly 1.0), so
                    // the outputs are bit-identical. And being opaque
                    // over every pixel, it hides *everything painted
                    // before it*. So: discard the whole scene composed
                    // so far and record the fill as the pass clear —
                    // the frame effectively starts at the last such
                    // cover. The root window background is the common
                    // case (cover at position 0, nothing to discard); a
                    // fullscreen page/panel painted over an underlay
                    // drops the entire hidden underlay too. The active
                    // clip must be empty: a scissored cover only hides
                    // its scissor, and an empty scissor state also
                    // guarantees no group in flight references
                    // `rounded_clips` state that `discard` wipes.
                    if self.current_scissor.is_none()
                        && self.current_chain.len == 0
                        && p.fill_kind == FillKind::SOLID
                        && p.fill.is_opaque()
                        && noop_f32(p.stroke_width)
                        && p.corners.approx_zero()
                        && phys_rect.min.x <= EPS
                        && phys_rect.min.y <= EPS
                        && phys_rect.max().x >= out.viewport_phys_f.x - EPS
                        && phys_rect.max().y >= out.viewport_phys_f.y - EPS
                    {
                        self.discard_composed(out);
                        out.clear_override = Some(p.fill.unpack());
                        continue;
                    }
                    let quad_urect = urect_from_phys(phys_rect.min, phys_rect.max(), viewport_phys);
                    // Clip-cull: skip emitting the quad when it sits
                    // entirely outside the active scissor. The GPU
                    // would scissor it away anyway; this saves the
                    // `quads.push` + per-quad math.
                    if self.cull_bounds(quad_urect) {
                        continue;
                    }
                    self.quad_forces_flush(quad_urect, out);
                    let world_radius = p.corners.scaled_by(current_transform.scale);
                    let phys_radius = world_radius.scaled_by(scale);
                    let stroke_width_phys = p.stroke_width * current_transform.scale * scale;
                    // Fragment fast path: a solid, sharp, stroke-less
                    // quad whose physical rect is pixel-aligned
                    // rasterizes only interior fragments (SDF coverage
                    // exactly 1.0) — flag the instance so the shader
                    // returns the premultiplied fill directly, skipping
                    // the SDF + composite path. Alignment is exact, not
                    // approx: exactness is what makes the skip
                    // bitwise-identical (host pixel snapping yields
                    // exact integers when active; unsnapped fractional
                    // rects keep the full SDF for edge AA).
                    let pmax = phys_rect.max();
                    let fast = p.fill_kind == FillKind::SOLID
                        && noop_f32(stroke_width_phys)
                        && phys_radius.approx_zero()
                        && phys_rect.min.x == phys_rect.min.x.round()
                        && phys_rect.min.y == phys_rect.min.y.round()
                        && pmax.x == pmax.x.round()
                        && pmax.y == pmax.y.round();
                    let fill_kind = if fast {
                        p.fill_kind.with_fast()
                    } else {
                        p.fill_kind
                    };
                    out.quads.push(Quad {
                        rect: phys_rect,
                        fill: p.fill,
                        corners: phys_radius,
                        stroke_color: p.stroke_color,
                        stroke_width: stroke_width_phys,
                        fill_kind,
                        fill_lut_row: p.fill_lut_row,
                        fill_axis: p.fill_axis,
                    });
                    if p.fill_kind == FillKind::SOLID && p.fill.is_opaque() {
                        let inscribed = phys_rect.inscribed_for_corners(phys_radius);
                        let stroke_inset =
                            if noop_f32(stroke_width_phys) || p.stroke_color.is_opaque() {
                                0.0
                            } else {
                                stroke_width_phys
                            };
                        let aa_inset = if fast { 0.0 } else { AA_RADIUS };
                        let cover = inscribed.deflated_by(Spacing::all(stroke_inset + aa_inset));
                        if !cover.is_paint_empty() {
                            let idx = out.quads.len() as u32 - 1 - self.cursors.quads;
                            self.occlusion.record_opaque(idx, cover);
                        }
                    }
                }
                Command::DrawShadow(p) => {
                    let world_rect = current_transform.apply_rect(p.rect);
                    let phys_rect = world_rect.scaled_by(scale, snap);
                    let quad_urect = urect_from_phys(phys_rect.min, phys_rect.max(), viewport_phys);
                    if self.cull_bounds(quad_urect) {
                        continue;
                    }
                    self.quad_forces_flush(quad_urect, out);
                    let world_radius = p.corners.scaled_by(current_transform.scale);
                    let phys_radius = world_radius.scaled_by(scale);
                    // Live shadow parameters are logical-px scalars; scale
                    // them so the shader's `local` coords line up.
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
                Command::DrawTriangle(p) => {
                    // Fold owner origin + active transform, scale to physical
                    // px. No pixel-snap — the SDF handles sub-pixel placement;
                    // snapping the covering rect would only shift the AA band.
                    let phys_scale = current_transform.scale * scale;
                    let xf = |q: Vec2| current_transform.apply_point(q + p.origin) * scale;
                    let a = xf(p.a);
                    let b = xf(p.b);
                    let c = xf(p.c);
                    let radius_phys = (p.radius * phys_scale).max(0.0);
                    let stroke_phys = (p.stroke_width * phys_scale).max(0.0);
                    // Covering AABB: the rounded shape (the SDF offsets the
                    // triangle outward by `radius` to round its corners) plus
                    // the ½px AA fringe. The stroke sits on the *inner* edge
                    // (like `RoundedRect`), so it adds no outward reach.
                    let lo = a.min(b).min(c);
                    let hi = a.max(b).max(c);
                    let pad = radius_phys + 0.5;
                    let rect = Rect::from_min_max(lo, hi).inflated(pad);
                    let tri_urect = urect_from_phys(rect.min, rect.max(), viewport_phys);
                    // Triangle is a quad-tier draw (lowest paint kind), so it
                    // culls + flushes exactly like `DrawRect`.
                    if self.cull_bounds(tri_urect) {
                        continue;
                    }
                    self.quad_forces_flush(tri_urect, out);
                    // Pack the three points in rect-local coords (0..size,
                    // matching the shader's `in.local`) + the corner radius
                    // into the reused `corners` / `fill_axis` lanes;
                    // `FillKind::TRIANGLE` tells the shader to read them as a
                    // triangle SDF rather than rounded-rect radii / gradient
                    // axis. No occlusion annotation — a triangle covers only
                    // its interior, not the whole `rect`.
                    let al = a - rect.min;
                    let bl = b - rect.min;
                    let cl = c - rect.min;
                    out.quads.push(Quad {
                        rect,
                        fill: p.fill,
                        corners: Corners::from_array([al.x, al.y, bl.x, bl.y]),
                        stroke_color: p.stroke_color,
                        stroke_width: stroke_phys,
                        fill_kind: FillKind::TRIANGLE,
                        fill_lut_row: LutRow::FALLBACK,
                        fill_axis: FillAxis::from_lanes(cl.x, cl.y, radius_phys, 0.0),
                    });
                }
                Command::DrawMesh(p) => {
                    // `draw_mesh` already gated empty/degenerate meshes
                    // (`draw_mesh` applies its no-op gate), so `v_len >= 1` here.
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
                    if !self.enter_higher_kind(PaintTier::Mesh, mesh_urect, out) {
                        continue;
                    }
                    // Verts already live in RecordStore owner-local;
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
                    out.meshes.push(MeshDrawRow {
                        draw: MeshDraw {
                            vertices: (p.v_start..p.v_start + p.v_len).into(),
                            indices: (p.i_start..p.i_start + p.i_len).into(),
                        },
                        instance: MeshInstance {
                            translate: phys_translate,
                            scale: phys_scale,
                            tint: p.tint.into(),
                            ..bytemuck::Zeroable::zeroed()
                        },
                    });
                }
                Command::DrawImage { payload: p, paint } => {
                    let world_rect = current_transform.apply_rect(p.rect);
                    let phys_rect = world_rect.scaled_by(scale, snap);
                    let image_urect =
                        urect_from_phys(phys_rect.min, phys_rect.max(), viewport_phys);
                    // Clip-cull + batch-close: image sits above text in the
                    // kind order (same as mesh), so a surviving draw closes
                    // the open text batch first.
                    if !self.enter_higher_kind(PaintTier::Image, image_urect, out) {
                        continue;
                    }
                    out.images.push(ImageDrawRow {
                        // Just the registration id — the backend looks it
                        // up in its texture cache; the encoder already
                        // resolved fit into `rect` + UV. A `GpuView` row is
                        // identical (its `id` is the off-screen target's),
                        // so the draw stays uniform; `target` below only
                        // schedules the off-screen paint.
                        id: p.handle,
                        instance: ImageInstance {
                            rect: phys_rect,
                            uv_min: p.uv_min,
                            uv_size: p.uv_size,
                            tint: p.tint.into(),
                            flags: p.flags,
                            ..bytemuck::Zeroable::zeroed()
                        },
                    });
                    // A `GpuView` also needs its off-screen target painted:
                    // list it with the used physical size + display/raster
                    // scales + app paint callback from the cmd buffer's side
                    // channel. The draw above already composites the result by
                    // `id`.
                    if let Some(paint) = paint {
                        let cap = i64::from(self.max_texture_dim.get());
                        let s = phys_rect.size;
                        let downsample =
                            (self.max_texture_dim.get() as f32 / s.w.max(s.h)).min(1.0);
                        let px = |v: f32| ((v * downsample).ceil() as i64).clamp(1, cap) as u32;
                        out.frame_targets.push(RenderTargetDraw {
                            id: p.handle,
                            used: UVec2::new(px(s.w), px(s.h)),
                            display_scale: scale,
                            raster_scale: current_transform.scale * scale * downsample,
                            paint: paint.clone(),
                        });
                    }
                }
                Command::DrawCurve(p) => {
                    let width_phys = p.width * current_transform.scale * scale;
                    let cap = p.cap.get();
                    let bbox_scissor = stroke_bbox_scissor(
                        current_transform,
                        p.bbox,
                        p.origin,
                        width_phys,
                        cap,
                        None,
                        display,
                    );
                    // Clip-cull + batch-close: curve sits above text in the
                    // kind order (same as mesh/image), so a surviving draw
                    // closes the open text batch first.
                    if !self.enter_higher_kind(PaintTier::Curve, bbox_scissor, out) {
                        continue;
                    }
                    // Transform control points to physical px. Owner
                    // origin folds in here so the record stays
                    // owner-local (cross-frame stable). No pixel
                    // snapping — snapping control points would warp
                    // the curve shape; AA fringe lives in the shader.
                    // Paint-time spin rotates the control points about
                    // the payload-bbox centre first (the encoder's
                    // pivot contract, see `spin_bbox`) — exact for a
                    // bezier by affine invariance.
                    let mut ctrl = [p.p0, p.p1, p.p2, p.p3];
                    if p.rotation != 0.0 {
                        let pivot = p.bbox.center();
                        let rotor = Vec2::from_angle(p.rotation);
                        for q in &mut ctrl {
                            *q = rotor.rotate(*q - pivot) + pivot;
                        }
                    }
                    let [p0, p1, p2, p3] =
                        ctrl.map(|q| current_transform.apply_point(q + p.origin) * scale);
                    // Adaptive sub-instance count from post-transform
                    // control-polygon length. Polygon length bounds
                    // arc length from above — slight overshoot, but
                    // never undershoots → no faceting from too-coarse
                    // sampling. Near-straight cubics (`Shape::Line`
                    // lowers as one; graph wires often relax to one)
                    // short-circuit to a single instance: every chord
                    // of a flat curve lies on the segment, so the 16
                    // baked chords render it exactly at any length.
                    let n = if cubic_is_flat(p0, p1, p2, p3) {
                        1
                    } else {
                        let l = (p1 - p0).length() + (p2 - p1).length() + (p3 - p2).length();
                        sub_instance_count(l)
                    };
                    let color: ColorU8 = p.color.into();
                    push_sub_instances(
                        out,
                        n,
                        CurveInstance {
                            p0,
                            p1,
                            p2,
                            p3,
                            width: width_phys,
                            color0: color,
                            color1: color,
                            cap: cap_lanes(cap as u32, cap as u32),
                            fill_kind: p.fill_kind,
                            fill_lut_row: p.fill_lut_row,
                            kind: CURVE_KIND_CUBIC,
                            ..bytemuck::Zeroable::zeroed()
                        },
                    );
                }
                Command::DrawArc(p) => {
                    let width_phys = p.width * current_transform.scale * scale;
                    let cap = p.cap.get();
                    let bbox_scissor = stroke_bbox_scissor(
                        current_transform,
                        p.bbox,
                        p.origin,
                        width_phys,
                        cap,
                        None,
                        display,
                    );
                    if !self.enter_higher_kind(PaintTier::Curve, bbox_scissor, out) {
                        continue;
                    }
                    // Paint-time spin: rotate the center about the
                    // payload-bbox centre (the encoder guarantees
                    // `bbox.center()` is the owner-box pivot when
                    // `rotation != 0`, same contract as DrawPolyline)
                    // and shift both angles — exact for a circle, no
                    // control-point rotation needed.
                    let mut center = p.center;
                    let mut a0 = p.a0;
                    let mut a1 = p.a1;
                    if p.rotation != 0.0 {
                        let pivot = p.bbox.center();
                        center = Vec2::from_angle(p.rotation).rotate(center - pivot) + pivot;
                        a0 += p.rotation;
                        a1 += p.rotation;
                    }
                    // The transform stack is translate + uniform scale
                    // (no rotation/skew — see `TranslateScale`), so a
                    // circle maps to a circle: transform the center,
                    // scale the radius. Angles pass through untouched.
                    let center_phys = current_transform.apply_point(center + p.origin) * scale;
                    let radius_phys = p.radius * current_transform.scale * scale;
                    // Adaptive sub-instance count from the *exact* arc
                    // length `r·|sweep|` — no control-polygon overshoot.
                    // Same ~1.5 px chord target as the cubic path; at
                    // that density the chord sagitta is `≈ c²/(8r)` ≤
                    // 0.3 px even at r = 1, buried under the AA fringe.
                    let n = sub_instance_count(radius_phys * (a1 - a0).abs());
                    let color: ColorU8 = p.color.into();
                    push_sub_instances(
                        out,
                        n,
                        CurveInstance {
                            p0: center_phys,
                            p1: Vec2::new(radius_phys, 0.0),
                            p2: Vec2::new(a0, a1),
                            p3: Vec2::ZERO,
                            width: width_phys,
                            color0: color,
                            color1: color,
                            cap: cap_lanes(cap as u32, cap as u32),
                            fill_kind: p.fill_kind,
                            fill_lut_row: p.fill_lut_row,
                            kind: CURVE_KIND_ARC,
                            ..bytemuck::Zeroable::zeroed()
                        },
                    );
                }
                Command::DrawPolyline(p) => {
                    let mode = p.color_mode.get();
                    let cap = p.cap.get();
                    let join = p.join.get();
                    let width_phys = p.width * current_transform.scale * scale;

                    // Compute the inflated physical-px AABB once and
                    // reuse it for cull and overlap tracking. Inflating
                    // by the stroke's outer fringe means the cull never
                    // trims a pixel the stroke would reach, and it
                    // short-circuits before transforming the full point
                    // list — the win for long dense point runs.
                    let bbox_scissor = stroke_bbox_scissor(
                        current_transform,
                        p.bbox,
                        p.origin,
                        width_phys,
                        cap,
                        (p.points_len > 2).then_some(join),
                        display,
                    );
                    if self.cull_bounds(bbox_scissor) {
                        continue;
                    }

                    let pts_start = p.points_start as usize;
                    let pts_end = pts_start + p.points_len as usize;
                    let cs_start = p.colors_start as usize;
                    let cs_end = cs_start + p.colors_len as usize;
                    let src_points = &payloads.polyline_points[pts_start..pts_end];
                    let src_colors = &payloads.polyline_colors[cs_start..cs_end];

                    // Transform points into physical-px. Owner-local
                    // origin is folded in here so points stay owner-
                    // local in the record store (cross-frame stable). No
                    // pixel-snap — snapping stroke verts shifts thin
                    // lines off-axis. Hairline regime (<1 phys px) is
                    // the shader's trapezoid-plateau coverage.
                    self.polyline.points.clear();
                    if p.rotation == 0.0 {
                        self.polyline.points.extend(
                            src_points
                                .iter()
                                .map(|&q| current_transform.apply_point(q + p.origin) * scale),
                        );
                    } else {
                        // Spin: rotate each owner-local point about the
                        // bbox centre before placing it via the ancestor
                        // transform, so the shape rotates in place. The
                        // encoder replaced the payload bbox with a
                        // rotation-invariant square CENTRED on the spin
                        // pivot (the owner-box centre), so `bbox.center()`
                        // is the pivot by construction — keep the two
                        // ends of that contract in sync.
                        let pivot = p.bbox.center();
                        let rotor = Vec2::from_angle(p.rotation);
                        self.polyline.points.extend(src_points.iter().map(|&q| {
                            let local = rotor.rotate(q - pivot) + pivot;
                            current_transform.apply_point(local + p.origin) * scale
                        }));
                    }

                    // Keep only points beyond the coincidence threshold
                    // from their predecessor — degenerate segments
                    // contribute no geometry and their colors drop
                    // with them.
                    self.polyline.kept.clear();
                    let mut prev: Option<Vec2> = None;
                    for (i, &q) in self.polyline.points.iter().enumerate() {
                        if prev
                            .is_none_or(|p| (q - p).length_squared() > POLYLINE_COINCIDENT_EPS_SQ)
                        {
                            self.polyline.kept.push(i as u32);
                            prev = Some(q);
                        }
                    }
                    if self.polyline.kept.len() < 2 {
                        continue;
                    }
                    // Only now that the polyline will actually emit
                    // geometry — an empty or culled polyline must not
                    // split the batch or the group.
                    if !self.enter_higher_kind(PaintTier::Curve, bbox_scissor, out) {
                        continue;
                    }
                    let PolylineScratch {
                        points,
                        kept,
                        directions,
                    } = &mut self.polyline;
                    directions.clear();
                    directions.extend(kept.windows(2).map(|pair| {
                        (points[pair[1] as usize] - points[pair[0] as usize]).normalize()
                    }));
                    let pts = points.as_slice();
                    let kept = kept.as_slice();
                    let directions = directions.as_slice();
                    let pt = |k: usize| pts[kept[k] as usize];
                    // Segment color(s) for the kept segment `k → k+1`,
                    // honoring the original indices (coincident skips
                    // drop the degenerate segments' colors, mirroring
                    // the old `ColorPlan` walker).
                    let seg_colors = |k: usize| -> (ColorU8, ColorU8) {
                        match mode {
                            ColorMode::Single => (src_colors[0], src_colors[0]),
                            ColorMode::PerPoint => (
                                src_colors[kept[k] as usize],
                                src_colors[kept[k + 1] as usize],
                            ),
                            ColorMode::PerSegment => {
                                let c = src_colors[kept[k + 1] as usize - 1];
                                (c, c)
                            }
                        }
                    };
                    let user_cap = cap as u32;
                    let n_segs = directions.len();
                    for k in 0..n_segs {
                        // Pre-oriented bisector clip planes for the
                        // joint ends, riding the neighbor lanes ("keep"
                        // is `dot(x - endpoint, n) <= 0` in the shader);
                        // zero = cap end, no clip.
                        let n_start = if k > 0 {
                            -(directions[k - 1] + directions[k])
                        } else {
                            Vec2::ZERO
                        };
                        let n_end = if k + 1 < n_segs {
                            directions[k] + directions[k + 1]
                        } else {
                            Vec2::ZERO
                        };
                        let butt = LineCap::Butt as u32;
                        let start_cap = if k == 0 { user_cap } else { butt };
                        let end_cap = if k + 1 == n_segs { user_cap } else { butt };
                        let (color, color1) = seg_colors(k);
                        out.curves.push(CurveInstance {
                            p0: pt(k),
                            p1: n_start,
                            p2: n_end,
                            p3: pt(k + 1),
                            t0: 0.0,
                            t1: 1.0,
                            width: width_phys,
                            color0: color,
                            color1,
                            cap: cap_lanes(start_cap, end_cap),
                            kind: CURVE_KIND_SEGMENT,
                            ..bytemuck::Zeroable::zeroed()
                        });
                    }
                    // One chrome instance per interior joint fills the
                    // convex wedge between the two segment end faces.
                    // The face-plane normals ride the neighbor lanes
                    // pre-oriented for the shader's keep test
                    // (`p1 = -d_a`, `p2 = d_b`). Chrome paints with the
                    // average of the adjacent colors.
                    for k in 1..n_segs {
                        let d_a = directions[k - 1];
                        let d_b = directions[k];
                        let (_, ca) = seg_colors(k - 1);
                        let (cb, _) = seg_colors(k);
                        let color = ca.midpoint(cb);
                        out.curves.push(CurveInstance {
                            p0: pt(k),
                            p1: -d_a,
                            p2: d_b,
                            t0: 0.0,
                            t1: 1.0,
                            width: width_phys,
                            color0: color,
                            color1: color,
                            kind: polyline_join_kind(d_a, d_b, join),
                            ..bytemuck::Zeroable::zeroed()
                        });
                    }
                }
                Command::DrawText(t) => {
                    let world_rect = current_transform.apply_rect(t.rect);
                    // Scale once: `unclipped` (overlap/cull bounds) and the
                    // emitted run's `origin` both derive from this rect.
                    let phys_rect = world_rect.scaled_by(scale, snap);
                    // `bounds` feeds the batch GPU scissor (union of the
                    // batch's runs — see the strict-bounds rule below) and
                    // the backend's per-line y-cull; there is no per-glyph
                    // clip. Intersect with the active clip-stack top so
                    // ancestor `clip = true` panels actually clip glyphs;
                    // an empty intersection means the run can't reach
                    // pixels — skip the push entirely (cull).
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
                    if self.any_higher_kind_overlap(bounds) {
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
                    if let Some(b) = self.batch.open.as_ref()
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
                        // Aperture's native text shader (see
                        // `src/renderer/backend/text/`) consumes linear
                        // bytes and premultiplies at output — matching
                        // the rest of the renderer's pipelines. No sRGB
                        // roundtrip.
                        color: t.color.into(),
                        key: t.key,
                        // Snap the ancestor-transform component of the
                        // text scale to discrete 0.5% steps. Continuous
                        // zoom would otherwise mint a fresh glyph
                        // cache key every frame (subpixel font size +
                        // bin shift), forcing swash to re-rasterize
                        // every glyph. Snapping stabilizes the key
                        // across small zoom deltas so the atlas hits.
                        // Quads/meshes keep continuous scale — only
                        // text glyph crispness "steps."
                        scale: snap_text_scale(current_transform.scale),
                    });
                    self.batch.open_grid.push(bounds);
                }
            }
        }
        self.close_batch(out);
        self.flush(out);
    }

    /// Clear-fold discard: a fullscreen opaque cover proved everything
    /// composed so far invisible — drop the scene output and every piece of
    /// scratch that describes it. The *walk* state survives: `clip_stack` /
    /// `current_scissor` / `current_chain` are empty by the fold's
    /// precondition, and `transform_stack` + the caller's running transform
    /// stay untouched (the cover may sit under an active transform whose
    /// pops are still ahead in the stream).
    fn discard_composed(&mut self, out: &mut RenderBuffer) {
        out.discard_scene();
        self.reset_group_scratch(out.viewport_phys);
    }

    /// Reset every piece of scratch that describes composed *scene*
    /// output — group cursors, batch state, overlap tracking. Shared
    /// by the per-compose prologue and the clear-fold
    /// [`Self::discard_composed`], so a new scratch field added here
    /// resets on both paths. Walk state (clip/transform stacks, the
    /// active scissor + chain) is deliberately not touched — the
    /// discard path must preserve it.
    fn reset_group_scratch(&mut self, viewport_phys: UVec2) {
        self.batch.open_grid.start_frame(viewport_phys);
        self.batch.closed_grid.start_frame(viewport_phys);
        self.batch.pending.clear();
        self.higher_kinds.clear();
        self.cursors = GroupCursors::default();
        self.batch.open = None;
        self.occlusion.clear();
    }
}

/// Upper bound on sub-instances per curve. Long, fast-curving strokes
/// (think a 4k-px-long swooping bezier at 200% zoom) hit this cap;
/// beyond it the chord error rises but stays well under a pixel for
/// any realistic UI workload. Cap is a sanity belt — far above the
/// 1–4 sub-instance steady state.
const MAX_SUB_INSTANCES: u32 = 256;

/// Target chord length for GPU-stroke subdivision, physical px. The
/// shader bakes `SEGMENTS_PER_INSTANCE` chords per instance; the
/// composer sizes the instance count so each chord lands near this
/// length — short enough that the 0.5 px AA fringe fully covers any
/// sub-pixel kink between chords. Shared by the cubic (control-polygon
/// length bound) and arc (exact `r·|sweep|` length) paths.
const TARGET_CHORD_PX: f32 = 1.5;

/// Sub-instance count for a GPU stroke of on-screen length `len_px`:
/// enough `SEGMENTS_PER_INSTANCE`-chord instances that each chord
/// lands near [`TARGET_CHORD_PX`], clamped to [`MAX_SUB_INSTANCES`].
#[inline]
fn sub_instance_count(len_px: f32) -> u32 {
    let total_segments = (len_px / TARGET_CHORD_PX).ceil().max(1.0) as u32;
    total_segments
        .div_ceil(SEGMENTS_PER_INSTANCE)
        .clamp(1, MAX_SUB_INSTANCES)
}

/// Tile `t ∈ [0, 1]` into `n` contiguous ranges (the last ending at
/// exactly `1.0`, so the shader's trailing-cap test fires) and push
/// one instance per range; `proto` supplies every other lane.
fn push_sub_instances(out: &mut RenderBuffer, n: u32, proto: CurveInstance) {
    let inv_n = 1.0 / n as f32;
    for i in 0..n {
        let t1 = if i + 1 == n {
            1.0
        } else {
            (i + 1) as f32 * inv_n
        };
        out.curves.push(CurveInstance {
            t0: i as f32 * inv_n,
            t1,
            ..proto
        });
    }
}

/// Squared distance below which two consecutive transformed polyline
/// points count as coincident and the latter is dropped — a
/// zero-length segment has no direction (`normalize` would NaN the
/// joint planes), so it must contribute no geometry, and its color
/// drops with it.
const POLYLINE_COINCIDENT_EPS_SQ: f32 = 1e-12;

/// Chrome kind for the joint between two polyline segments with unit
/// directions `d_a` (into the joint) and `d_b` (out of it). `Miter`
/// downgrades to bevel past [`MITER_LIMIT`] — the SVG convention; an
/// antiparallel fold (180°, bisector undefined) renders round — the
/// only join whose shape is well-defined there.
fn polyline_join_kind(d_a: Vec2, d_b: Vec2, join: LineJoin) -> u32 {
    let sum = d_a + d_b;
    let len_sq = sum.length_squared();
    if len_sq < 1e-6 {
        return CURVE_KIND_JOIN_ROUND;
    }
    match join {
        LineJoin::Round => CURVE_KIND_JOIN_ROUND,
        LineJoin::Bevel => CURVE_KIND_JOIN_BEVEL,
        LineJoin::Miter => {
            // |d_a + d_b| = 2·cos(half turn angle) for unit inputs.
            let cos_half = 0.5 * len_sq.sqrt();
            if cos_half < 1.0 / MITER_LIMIT {
                CURVE_KIND_JOIN_BEVEL
            } else {
                CURVE_KIND_JOIN_MITER
            }
        }
    }
}

/// Max perpendicular distance (physical px) of a cubic's inner control
/// points from the chord line for the curve to count as flat. The
/// curve deviates at most `3/4 · max(d1, d2)` from the chord, so at
/// this threshold it sits within ~0.075 px of a straight line —
/// invisible under the 0.5 px AA fringe at any chord density.
const FLAT_EPS_PX: f32 = 0.1;

/// True when the cubic's trace is visually indistinguishable from the
/// straight segment `p0 → p3` (see [`FLAT_EPS_PX`]). Both inner CPs
/// must sit within the threshold of the *infinite* chord line; a
/// degenerate chord (closed curve) is never flat.
#[inline]
fn cubic_is_flat(p0: Vec2, p1: Vec2, p2: Vec2, p3: Vec2) -> bool {
    let chord = p3 - p0;
    let len = chord.length();
    if len <= FLAT_EPS_PX {
        return false;
    }
    let d1 = chord.perp_dot(p1 - p0).abs();
    let d2 = chord.perp_dot(p2 - p0).abs();
    d1.max(d2) <= FLAT_EPS_PX * len
}

/// Additive step on the text-scale ladder. Same step in *scale units*
/// across the range, so the step in *percent of current size* shrinks
/// as zoom grows (0.005/4 ≈ 0.125% at 4×, 0.005/1 = 0.5% at 1×, 0.005/0.5
/// = 1% at 0.5×). The user-perceptual case for this layout: at high
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
/// additive 0.5% ladder. Identity is preserved exactly so non-zoom UIs
/// stay on the trivial path. See call-site comment in `DrawText` for
/// rationale.
fn snap_text_scale(s: f32) -> f32 {
    if (s - 1.0).abs() < EPS {
        return 1.0;
    }
    (s / TEXT_SCALE_STEP).round() * TEXT_SCALE_STEP
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

/// Value equality of two rounded-mask chains (spans into
/// `out.rounded_clips`). Spans differ across a pop/re-push of an
/// identical clip — the composer pushes a fresh chain per rounded push —
/// but value-equal chains stamp identical masks, so clip-transition
/// decisions must not split on span identity alone.
fn chains_equal(out: &RenderBuffer, a: Span, b: Span) -> bool {
    out.rounded_clips[a.range()] == out.rounded_clips[b.range()]
}

#[cold]
#[inline(never)]
fn rounded_clip_depth_overflow(depth: u32) -> ! {
    panic!("rounded clip chain depth {depth} exceeds stencil capacity {MAX_ROUNDED_CLIP_DEPTH}");
}

/// Physical-px scissor for a stroked shape's owner-local centerline
/// `bbox`. Folds `origin` + the active transform into physical space,
/// applies the shared stroke/cap/join/AA bound once, then clamps to the
/// viewport. Shared by the curve and polyline paths so their cull and
/// overlap bounds cannot drift.
fn stroke_bbox_scissor(
    xform: TranslateScale,
    bbox: Rect,
    origin: Vec2,
    width_phys: f32,
    cap: LineCap,
    join: Option<LineJoin>,
    display: Display,
) -> URect {
    let world_bbox = xform.apply_rect(Rect {
        min: bbox.min + origin,
        size: bbox.size,
    });
    let centerline_phys = world_bbox.scaled_by(display.scale_factor, false);
    let painted = stroked_bbox(centerline_phys, width_phys, HALF_FRINGE, cap, join);
    urect_from_phys(painted.min, painted.max(), display.physical)
}

#[cfg(test)]
mod tests;
