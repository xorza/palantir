//! The frontend-to-backend contract: [`RenderBuffer`], the per-kind instance
//! rows the composer fills, and the group and batch tables that say in what
//! order the backend draws them.
//!
//! Every buffer here is retained and refilled, so a steady-state frame
//! allocates nothing for its output.

use crate::display::Display;
use crate::primitives::texture_id::TextureId;
use crate::primitives::{color::Color, corners::Corners, rect::Rect};
use crate::renderer::quad::Quad;
use glam::{UVec2, Vec2};
use soa_rs::Soa;
use std::time::Duration;

pub(crate) mod curve;
pub(crate) mod draw_group;
pub(crate) mod group_batch;
pub(crate) mod icon;
pub(crate) mod image;
pub(crate) mod mesh;
pub(crate) mod paint_tier;
pub(crate) mod per_group_batch;
pub(crate) mod text;
pub(crate) mod text_batch;

use crate::renderer::render_buffer::draw_group::DrawGroup;

use crate::renderer::render_buffer::group_batch::GroupBatch;

use crate::renderer::render_buffer::paint_tier::PaintTier;

use crate::renderer::render_buffer::curve::CurveInstance;
use crate::renderer::render_buffer::icon::IconDrawRow;
use crate::renderer::render_buffer::image::{FrameViews, ImageDrawRow, RenderTargetDraw};
use crate::renderer::render_buffer::mesh::MeshDrawRow;
use crate::renderer::render_buffer::text::TextDrawRow;
use crate::renderer::render_buffer::text_batch::TextBatch;

/// Deepest rounded-mask chain representable by the renderer's
/// eight-bit stencil counter.
pub(crate) const MAX_ROUNDED_CLIP_DEPTH: u32 = u8::MAX as u32;

/// Output of `compose`: physical-px instances grouped by scissor region plus
/// the wgpu callback sidecar for composited `GpuView`s.
///
/// Contains no compose-time scratch. Owns
/// its allocations across frames so steady-state composing is alloc-free for
/// the output; reuse a single `RenderBuffer` and call
/// `compose(.., &mut buffer)` each frame.
#[derive(Debug)]
pub(crate) struct RenderBuffer {
    pub(crate) quads: Vec<Quad>,
    pub(crate) texts: Vec<TextDrawRow>,
    /// Scene-wide mesh rows, SoA-stored. The underlying vertex/index
    /// bytes live in the recording's
    /// [`RecordPayloads::meshes`](crate::scene::record_store::record_payloads::RecordPayloads::meshes);
    /// each row's `draw` field carries spans into those payloads, and the
    /// `instance` field carries the Pod GPU state the backend uploads
    /// verbatim (read as a contiguous `&[MeshInstance]` via
    /// `meshes.instance()`).
    pub(crate) meshes: Soa<MeshDrawRow>,
    pub(crate) groups: Vec<DrawGroup>,
    /// One entry per *batch* of text runs that share a single text-backend
    /// `prepare`/`render` call. The composer coalesces text across
    /// adjacent groups when paint-order is preserved (no occluding
    /// quad/mesh, no rounded-clip change) — collapsing many small
    /// draw calls into one. Each batch's `texts` span is contiguous
    /// in `RenderBuffer.texts` by composer construction; batches anchor
    /// to groups via `TextBatch.last_group`.
    pub(crate) text_batches: Vec<TextBatch>,
    /// Per-group batches for every [`PaintTier`], indexed by
    /// [`PaintTier::idx`] and reached through [`Self::batches`] /
    /// [`Self::batches_mut`].
    ///
    /// One array rather than a column per tier: the four held the same
    /// type and the same shape, so a new tier meant a field, an init, a
    /// clear and two match arms that nothing checked were all present.
    /// Sized by [`PaintTier::COUNT`], so now it means a variant.
    ///
    /// Currently one batch per group that emitted into that tier —
    /// none of these span scissor boundaries, since only text carries
    /// per-run bounds. Schedule and backend drain them with the same
    /// cursor walk as `text_batches`.
    batches: [Vec<GroupBatch>; PaintTier::COUNT],
    /// Scene-wide image rows, SoA-stored; structurally mirrors
    /// [`Self::meshes`]. The backend binds a per-handle texture and
    /// issues one draw per row (no shared vertex/index buffers — every
    /// quad is implicit four-corner from the shader's `vertex_index`).
    /// A `GpuView` is just another image row here — the scene carries
    /// no render-target concept; its off-screen target is listed
    /// separately in [`Self::frame_targets`], but the row composites
    /// exactly like an image: same `id` in the shared texture cache,
    /// same draw.
    pub(crate) images: Soa<ImageDrawRow>,
    /// `GpuView` off-screen targets to paint this frame — one per composited
    /// `GpuView` image row. The composer fills this directly from the
    /// `DrawImage.target` link (resolving physical size, effective raster scale,
    /// and the app `paint` callback) as it walks image draws; the backend drains
    /// it to allocate + paint. Carries the callback, so the backend reaches the
    /// renderer without any `Ui`-side registry.
    pub(crate) frame_targets: Vec<RenderTargetDraw>,
    /// Every `GpuView` this frame *recorded*, painted or not — the backend's
    /// retention roster. Filled by `Frontend::build` from the frame's live
    /// view map, so it is a per-frame stamp rather than a scene column: an
    /// undamaged view is culled out of [`Self::frame_targets`] but stays
    /// here, which is what keeps its off-screen texture (and everything
    /// `GpuPaint::init` built into it) alive across frames the view sits out.
    pub(crate) live_targets: Vec<TextureId>,
    /// Icon draws in composite order, each already resolved to a physical-px
    /// origin and a raster key. Drained one batch at a time — the backend
    /// rasterizes any miss and binds its own atlas, so a run of icons is
    /// one draw.
    pub(crate) icons: Vec<IconDrawRow>,
    /// Native GPU stroke instances. Each is one [`CurveInstance`] basis
    /// kind — a `[t0, t1]` sub-range of a cubic/arc (adaptive count from
    /// on-screen length), a polyline segment, or joint chrome. The
    /// pipeline draws all instances in a batch with one indexed
    /// instanced draw over its immutable strip indices.
    pub(crate) curves: Vec<CurveInstance>,
    /// Flat pool of rounded-clip mask geometry. `DrawGroup.rounded_clips`
    /// and `TextBatch.rounded_clips` are spans into it, each an
    /// outer→inner chain of the rounded masks active for that group /
    /// batch (nested rounded clips stack — the stencil path stamps one
    /// mask per chain entry). The composer pushes one chain per rounded
    /// `PushClip` (ancestors copied so every chain is contiguous);
    /// value-equal chains from separate pushes dedup at mask staging.
    pub(crate) rounded_clips: Vec<RoundedClip>,
    /// Clear fold: when an unclipped opaque solid sharp quad covers the
    /// whole viewport, the composer discards everything composed before it
    /// (fully hidden), drops the quad, and records its fill here — the
    /// frame effectively starts at the last such cover. The backend clears
    /// (or pre-clears, on partial frames) to this color instead of the
    /// plan's — pixel-identical output, minus the hidden underlay and the
    /// full-surface fragment load of the biggest quad in the frame.
    pub(crate) clear_override: Option<Color>,
    /// Physical-px viewport, ceil'd. Backends use this as the default scissor
    /// when a group has no clip.
    pub(crate) viewport_phys: UVec2,
    /// Same viewport in float — needed by the wgpu vertex shader uniform.
    pub(crate) viewport_phys_f: Vec2,
    /// Logical→physical conversion factor, propagated from `Display`.
    /// Glyph rasterization needs it: shaped buffers are sized in logical px,
    /// so the text backend scales by this when emitting glyph quads.
    pub(crate) scale: f32,
    /// This frame's monotonic time (window-start `elapsed`), stamped by
    /// `Frontend::build` from the frame scene clock (not derivable from `Display`).
    /// The backend diffs it against each `GpuView`'s last paint to derive
    /// `GpuFrameCtx::dt`.
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
            viewport_phys: UVec2::ZERO,
            viewport_phys_f: Vec2::ZERO,
            scale: 1.0,
            time: Duration::ZERO,
        }
    }

    /// Reset every per-frame column (capacity retained) and stamp the
    /// frame's viewport + scale from `display`. Called by
    /// `Composer::compose` at frame start — the reset lives here,
    /// beside the fields, so adding a column forces choosing its reset
    /// in the same edit instead of in the composer's preamble.
    pub(crate) fn start_frame(&mut self, display: Display, time: Duration) {
        self.discard_scene();
        self.clear_override = None;
        self.viewport_phys = display.physical;
        self.viewport_phys_f = display.physical.as_vec2();
        self.scale = display.scale_factor;
        // Stamped here rather than after compose: not derivable from
        // `display`, and a field that held a placeholder for the whole
        // pass is one anything composing against it would read wrong.
        self.time = time;
    }

    /// How many draws this tier has emitted so far — the one place the
    /// per-tier row columns are still named, since each holds a different
    /// instance type. Paired with [`Self::batches_mut`] so the composer
    /// closes a group over every tier by iterating [`PaintTier::ALL`].
    pub(crate) fn draws_len(&self, tier: PaintTier) -> u32 {
        let len = match tier {
            PaintTier::Mesh => self.meshes.len(),
            PaintTier::Image => self.images.len(),
            PaintTier::Icon => self.icons.len(),
            PaintTier::Curve => self.curves.len(),
        };
        len as u32
    }

    /// This tier's per-group batches, for appending.
    pub(crate) fn batches_mut(&mut self, tier: PaintTier) -> &mut Vec<GroupBatch> {
        &mut self.batches[tier.idx()]
    }

    /// This tier's per-group batches. Named rather than indexed at the
    /// call site so every consumer that walks all of them goes through
    /// here and [`PaintTier::ALL`], keeping the replay order in one place.
    pub(crate) fn batches(&self, tier: PaintTier) -> &[GroupBatch] {
        &self.batches[tier.idx()]
    }

    /// This frame's `GpuView`s, as the backend takes them: what to repaint,
    /// and what to keep. See [`FrameViews`].
    pub(crate) fn frame_views(&self) -> FrameViews<'_> {
        FrameViews {
            draws: &self.frame_targets,
            live: &self.live_targets,
        }
    }

    /// Drop every scene column (capacity retained), leaving the per-frame
    /// stamps (`clear_override`, `live_targets`, viewport, scale, time)
    /// untouched. Shared by [`Self::start_frame`] and the composer's clear
    /// fold, which discards everything composed so far when a fullscreen
    /// opaque cover proves it invisible — a new scene column added here
    /// resets on both paths at once.
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

/// Physical-px rounded-clip geometry for stencil masking. `mask_rect`
/// is the clip's full physical-pixel rect — **not** clamped to viewport
/// or any ancestor scissor — so the mask SDF's corner curves stay
/// anchored at the rect's true edges even when the clip is partially
/// off-screen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct RoundedClip {
    pub(crate) mask_rect: Rect,
    pub(crate) corners: Corners,
}
