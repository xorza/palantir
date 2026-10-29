//! The frontend-to-backend contract: [`RenderBuffer`], the per-kind instance rows the composer fills, and the group and batch tables that order the backend's draws. Every buffer is retained and refilled, so a steady-state frame allocates nothing for its output.

use crate::common::span::Span;
use crate::display::Display;
use crate::display::frame_stamp::FrameStamp;
use crate::primitives::identity::pane_key::PaneKey;
use crate::primitives::identity::texture_id::TextureId;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::renderer::quad::Quad;
use crate::renderer::render_buffer::backdrop_row::{BackdropRow, PaneSource};
use crate::renderer::render_buffer::curve_instance::CurveInstance;
use crate::renderer::render_buffer::draw_group::DrawGroup;
use crate::renderer::render_buffer::group_batch::GroupBatch;
use crate::renderer::render_buffer::icon_draw_row::IconDrawRow;
use crate::renderer::render_buffer::image_draw_row::ImageDrawRow;
use crate::renderer::render_buffer::mesh_draw_row::MeshDrawRow;
use crate::renderer::render_buffer::paint_tier::PaintTier;
use crate::renderer::render_buffer::render_target_draw::{FrameViews, RenderTargetDraw};
use crate::renderer::render_buffer::rounded_clip::RoundedClip;
use crate::renderer::render_buffer::text_batch::TextBatch;
use crate::renderer::render_buffer::text_draw_row::TextDrawRow;
use soa_rs::Soa;
use std::time::Duration;

pub(crate) mod backdrop_row;
pub(crate) mod curve_caps;
pub(crate) mod curve_instance;
pub(crate) mod curve_kind;
pub(crate) mod draw_group;
pub(crate) mod group_batch;
pub(crate) mod icon_draw_row;
pub(crate) mod image_draw_row;
pub(crate) mod image_flags;
pub(crate) mod mesh_draw_row;
pub(crate) mod paint_tier;
pub(crate) mod per_group_batch;
pub(crate) mod render_target_draw;
pub(crate) mod rounded_clip;
pub(crate) mod text_batch;
pub(crate) mod text_draw_row;

/// Deepest rounded-mask chain the eight-bit stencil counter can represent.
pub(crate) const MAX_ROUNDED_CLIP_DEPTH: u32 = u8::MAX as u32;

/// Output of `compose`: physical-px instances grouped by scissor region plus the wgpu callback sidecar for composited `GpuView`s. Holds no compose-time scratch; reuse one buffer across frames.
#[derive(Debug)]
pub(crate) struct RenderBuffer {
    pub(crate) quads: Vec<Quad>,
    pub(crate) texts: Vec<TextDrawRow>,
    /// Scene-wide mesh rows, SoA. Vertex/index bytes live in [`RecordStore::meshes`](crate::shape::record_store::RecordStore::meshes); each row's `draw` carries spans into them and `instance` the Pod GPU state uploaded verbatim.
    pub(crate) meshes: Soa<MeshDrawRow>,
    pub(crate) groups: Vec<DrawGroup>,
    /// One entry per batch of text runs sharing a text-backend call. Text coalesces across adjacent groups when paint order is preserved. A batch's `texts` span is contiguous; it anchors to a group via `TextBatch.last_group`.
    pub(crate) text_batches: Vec<TextBatch>,
    /// Per-group batches for every [`PaintTier`], indexed by [`PaintTier::idx`] via [`Self::batches`] / [`Self::batches_mut`] and sized by [`PaintTier::COUNT`], so a new tier is a variant, not a field. None span scissor boundaries; only text carries per-run bounds.
    batches: [Vec<GroupBatch>; PaintTier::COUNT],
    /// Scene-wide image rows, SoA, mirroring [`Self::meshes`]; one quad draw per row. A `GpuView` is just an image row, its off-screen target listed in [`Self::frame_targets`].
    pub(crate) images: Soa<ImageDrawRow>,
    /// `GpuView` off-screen targets of this frame, one per composited view row, filled by the composer from each row's `ImageDraw::view`. The backend repaints one only when its stamp changed.
    pub(crate) frame_targets: Vec<RenderTargetDraw>,
    /// Every `GpuView` recorded this frame, painted or not: the backend's retention roster. An undamaged view leaves [`Self::frame_targets`] but stays here, keeping its texture alive.
    pub(crate) live_targets: Vec<TextureId>,
    pub(crate) icons: Vec<IconDrawRow>,
    /// Native GPU stroke instances: a `[t0, t1]` sub-range of a cubic/arc, a polyline segment, or a join billboard. One indexed instanced draw per batch.
    pub(crate) curves: Vec<CurveInstance>,
    /// Flat pool of rounded-clip mask chains (outer→inner); `DrawGroup.rounded_clips` and `TextBatch.rounded_clips` are spans into it. Value-equal chains dedup at mask staging.
    pub(crate) rounded_clips: Vec<RoundedClip>,
    /// The backdrop panes in paint order; each one that captures breaks the main pass at its
    /// barrier and copies what the groups before it painted.
    pub(crate) backdrops: Vec<BackdropRow>,
    /// Every pane of the frame, drawn or not: the backend's roster, which keeps an unchanged
    /// pane's cached blur while it stays out of the frame's damage.
    pub(crate) live_panes: Vec<PaneKey>,
    /// Clear fold: an unclipped opaque solid sharp quad covering the viewport discards everything composed before it and is dropped, its fill recorded here for the backend to clear to.
    pub(crate) clear_override: Option<RgbaF16>,
    /// The display this buffer was composed for, held whole so every reader sees the viewport the compose session used.
    pub(crate) display: Display,
    /// This frame's monotonic time, stamped by `Frontend::build`; the backend diffs it for `GpuFrameCtx::dt`.
    pub(crate) time: Duration,
}

impl RenderBuffer {
    pub(crate) fn new() -> Self {
        Self {
            quads: Vec::new(),
            texts: Vec::new(),
            meshes: Soa::default(),
            groups: Vec::new(),
            text_batches: Vec::new(),
            batches: [const { Vec::new() }; PaintTier::COUNT],
            images: Soa::default(),
            frame_targets: Vec::new(),
            live_targets: Vec::new(),
            icons: Vec::new(),
            curves: Vec::new(),
            rounded_clips: Vec::new(),
            backdrops: Vec::new(),
            live_panes: Vec::new(),
            clear_override: None,
            display: Display::default(),
            time: Duration::ZERO,
        }
    }

    /// Reset every per-frame column (capacity retained) and stamp viewport, scale and time from `stamp`.
    pub(crate) fn start_frame(&mut self, stamp: FrameStamp) {
        self.discard_scene();
        self.backdrops.clear();
        self.clear_override = None;
        self.display = stamp.display;
        self.time = stamp.time;
    }

    pub(crate) fn draws_len(&self, tier: PaintTier) -> u32 {
        let len = match tier {
            PaintTier::Mesh => self.meshes.len(),
            PaintTier::Image => self.images.len(),
            PaintTier::Icon => self.icons.len(),
            PaintTier::Curve => self.curves.len(),
        };
        len as u32
    }

    pub(crate) const fn batches_mut(&mut self, tier: PaintTier) -> &mut Vec<GroupBatch> {
        &mut self.batches[tier.idx()]
    }

    /// Value equality of two rounded-mask chains: a pop and re-push of an identical clip gives different spans but identical masks.
    pub(crate) fn chains_equal(&self, a: Span, b: Span) -> bool {
        self.rounded_clips[a.range()] == self.rounded_clips[b.range()]
    }

    /// This tier's per-group batches; consumers walking all tiers go through here and [`PaintTier::ALL`] to keep replay order in one place.
    pub(crate) fn batches(&self, tier: PaintTier) -> &[GroupBatch] {
        &self.batches[tier.idx()]
    }

    pub(crate) fn frame_views(&self) -> FrameViews<'_> {
        FrameViews {
            draws: &self.frame_targets,
            live: &self.live_targets,
            display_scale: self.display.scale_factor(),
        }
    }

    /// The physical px the frame's captures copy out of the target: the footprints of the panes
    /// that capture, overlaps counted twice, as each copies its own.
    pub(crate) fn captured_area(&self) -> u64 {
        self.backdrops
            .iter()
            .filter(|row| row.source == PaneSource::Capture)
            .map(|row| row.footprint.area())
            .sum()
    }

    /// Drop every scene column but the backdrop rows, leaving per-frame stamps; shared with the
    /// composer's clear fold, which keeps the rows of the panes it hides.
    pub(crate) fn discard_scene(&mut self) {
        self.quads.clear();
        self.texts.clear();
        self.meshes.clear();
        self.images.clear();
        self.frame_targets.clear();
        self.groups.clear();
        self.text_batches.clear();
        for batches in &mut self.batches {
            batches.clear();
        }
        self.icons.clear();
        self.curves.clear();
        self.rounded_clips.clear();
    }
}
