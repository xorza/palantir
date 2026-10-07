//! The frontend-to-backend contract: [`RenderBuffer`], the per-kind instance rows the composer fills, and the group and batch tables that order the backend's draws. Every buffer is retained and refilled, so a steady-state frame allocates nothing for its output.

use crate::common::span::Span;
use crate::display::Display;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::texture_id::TextureId;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::renderer::quad::Quad;
use crate::renderer::render_buffer::curve::CurveInstance;
use crate::renderer::render_buffer::draw_group::DrawGroup;
use crate::renderer::render_buffer::group_batch::GroupBatch;
use crate::renderer::render_buffer::icon::IconDrawRow;
use crate::renderer::render_buffer::image::{FrameViews, ImageDrawRow, RenderTargetDraw};
use crate::renderer::render_buffer::mesh::MeshDrawRow;
use crate::renderer::render_buffer::paint_tier::PaintTier;
use crate::renderer::render_buffer::text::TextDrawRow;
use crate::renderer::render_buffer::text_batch::TextBatch;
use soa_rs::Soa;
use std::time::Duration;

pub(crate) mod curve;
pub(crate) mod curve_caps;
pub(crate) mod curve_kind;
pub(crate) mod draw_group;
pub(crate) mod group_batch;
pub(crate) mod icon;
pub(crate) mod image;
pub(crate) mod image_flags;
pub(crate) mod mesh;
pub(crate) mod paint_tier;
pub(crate) mod per_group_batch;
pub(crate) mod text;
pub(crate) mod text_batch;

/// Deepest rounded-mask chain the eight-bit stencil counter can represent.
pub(crate) const MAX_ROUNDED_CLIP_DEPTH: u32 = u8::MAX as u32;

/// Output of `compose`: physical-px instances grouped by scissor region plus the wgpu callback sidecar for composited `GpuView`s. Holds no compose-time scratch; reuse one buffer across frames.
#[derive(Debug)]
pub(crate) struct RenderBuffer {
    pub(crate) quads: Vec<Quad>,
    pub(crate) texts: Vec<TextDrawRow>,
    /// Scene-wide mesh rows, SoA. Vertex/index bytes live in [`RecordStore::meshes`](crate::scene::record_store::RecordStore::meshes); each row's `draw` carries spans into them and `instance` the Pod GPU state uploaded verbatim.
    pub(crate) meshes: Soa<MeshDrawRow>,
    pub(crate) groups: Vec<DrawGroup>,
    /// One entry per batch of text runs sharing a text-backend call. Text coalesces across adjacent groups when paint order is preserved. A batch's `texts` span is contiguous; it anchors to a group via `TextBatch.last_group`.
    pub(crate) text_batches: Vec<TextBatch>,
    /// Per-group batches for every [`PaintTier`], indexed by [`PaintTier::idx`] via [`Self::batches`] / [`Self::batches_mut`] and sized by [`PaintTier::COUNT`], so a new tier is a variant, not a field. None span scissor boundaries; only text carries per-run bounds.
    batches: [Vec<GroupBatch>; PaintTier::COUNT],
    /// Scene-wide image rows, SoA, mirroring [`Self::meshes`]; one quad draw per row. A `GpuView` is just an image row, its off-screen target listed in [`Self::frame_targets`].
    pub(crate) images: Soa<ImageDrawRow>,
    /// `GpuView` off-screen targets to paint this frame, one per composited view row, filled by the composer from `DrawImage.target`.
    pub(crate) frame_targets: Vec<RenderTargetDraw>,
    /// Every `GpuView` recorded this frame, painted or not: the backend's retention roster. An undamaged view leaves [`Self::frame_targets`] but stays here, keeping its texture alive.
    pub(crate) live_targets: Vec<TextureId>,
    pub(crate) icons: Vec<IconDrawRow>,
    /// Native GPU stroke instances: a `[t0, t1]` sub-range of a cubic/arc, a polyline segment, or joint chrome. One indexed instanced draw per batch.
    pub(crate) curves: Vec<CurveInstance>,
    /// Flat pool of rounded-clip mask chains (outer→inner); `DrawGroup.rounded_clips` and `TextBatch.rounded_clips` are spans into it. Value-equal chains dedup at mask staging.
    pub(crate) rounded_clips: Vec<RoundedClip>,
    /// Clear fold: an unclipped opaque solid sharp quad covering the viewport discards everything composed before it and is dropped, its fill recorded here for the backend to clear to.
    pub(crate) clear_override: Option<RgbaF16>,
    /// The display this buffer was composed for, held whole so every reader sees the viewport the compose session used.
    pub(crate) display: Display,
    /// This frame's monotonic time, stamped by `Frontend::build`; the backend diffs it for `GpuFrameContext::dt`.
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
            clear_override: None,
            display: Display::default(),
            time: Duration::ZERO,
        }
    }

    /// Reset every per-frame column (capacity retained) and stamp viewport and scale from `display`.
    pub(crate) fn start_frame(&mut self, display: Display, time: Duration) {
        self.discard_scene();
        self.clear_override = None;
        self.display = display;
        self.time = time;
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

    /// Drop every scene column, leaving per-frame stamps; shared with the composer's clear fold.
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

/// Physical-px rounded-clip geometry for stencil masking. `mask_rect` is the clip's full rect, **not** clamped to viewport or ancestor scissor, so corner curves stay anchored at the true edges when partially off-screen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct RoundedClip {
    pub(crate) mask_rect: Rect,
    pub(crate) corners: Corners,
}
