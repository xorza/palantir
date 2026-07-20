use crate::display::Display;
use crate::primitives::{color::Color, corners::Corners, rect::Rect};
use crate::renderer::quad::Quad;
use glam::{UVec2, Vec2};
use soa_rs::Soa;
use std::time::Duration;

pub(crate) mod batch;
pub(crate) mod curve;
pub(crate) mod image;
pub(crate) mod mesh;
pub(crate) mod owner;
pub(crate) mod text;

use crate::renderer::render_buffer::batch::{DrawGroup, GroupBatch, TextBatch};
use crate::renderer::render_buffer::curve::CurveInstance;
use crate::renderer::render_buffer::image::{ImageDrawRow, RenderTargetDraw};
use crate::renderer::render_buffer::mesh::MeshDrawRow;
use crate::renderer::render_buffer::owner::RenderOwnerId;
use crate::renderer::render_buffer::text::TextRun;

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
    pub(crate) texts: Vec<TextRun>,
    /// Scene-wide mesh rows, SoA-stored. The underlying vertex/index
    /// bytes live in the recording's
    /// [`RecordPayloads::meshes`](crate::record_store::RecordPayloads::meshes);
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
    /// One entry per *batch* of mesh draws. Currently one [`GroupBatch`]
    /// per group that emitted meshes (mesh batches don't span scissor
    /// boundaries since meshes have no per-run bounds). Schedule and
    /// backend treat meshes structurally like text — drained via the
    /// same cursor-walking pattern as `text_batches`.
    pub(crate) mesh_batches: Vec<GroupBatch>,
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
    /// One entry per *batch* of image draws (currently one
    /// [`GroupBatch`] per group that emitted images). Schedule walks
    /// these in lockstep with `groups` via a cursor — same pattern as
    /// `text_batches` / `mesh_batches`.
    pub(crate) image_batches: Vec<GroupBatch>,
    /// Native GPU stroke instances + per-scissor-group batches. One
    /// [`GroupBatch`] per group that emitted strokes; the schedule walks
    /// them in lockstep with `mesh_batches` / `image_batches` via a
    /// cursor. Each instance is one [`CurveInstance`] basis kind —
    /// a `[t0, t1]` sub-range of a cubic/arc (adaptive count from
    /// on-screen length), a polyline segment, or joint chrome. The
    /// pipeline draws all instances in a batch with one indexed
    /// instanced draw over its immutable strip indices.
    pub(crate) curves: Vec<CurveInstance>,
    pub(crate) curve_batches: Vec<GroupBatch>,
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
    /// Stable submitter identity, minted once at construction (one
    /// `RenderBuffer` per `Frontend`, i.e. per window) and never reset by
    /// `start_frame`. The shared backend's `ImagePipeline::paint_gpu_views`
    /// scopes `GpuView`-target eviction to it, so window A's submit can't
    /// free window B's targets.
    pub(crate) owner: RenderOwnerId,
}

impl RenderBuffer {
    pub(crate) fn new(owner: RenderOwnerId) -> Self {
        Self {
            owner,
            quads: Vec::new(),
            texts: Vec::new(),
            meshes: Soa::default(),
            groups: Vec::new(),
            text_batches: Vec::new(),
            mesh_batches: Vec::new(),
            images: Soa::default(),
            frame_targets: Vec::new(),
            image_batches: Vec::new(),
            curves: Vec::new(),
            curve_batches: Vec::new(),
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
    pub(crate) fn start_frame(&mut self, display: Display) {
        self.discard_scene();
        self.clear_override = None;
        self.viewport_phys = display.physical;
        self.viewport_phys_f = display.physical.as_vec2();
        self.scale = display.scale_factor;
        // Not derivable from `display`; `Frontend::build` stamps the real
        // value after compose.
        self.time = Duration::ZERO;
    }

    /// Drop every scene column (capacity retained), leaving the per-frame
    /// stamps (`clear_override`, viewport, scale, time) untouched. Shared by
    /// [`Self::start_frame`] and the composer's clear fold, which discards
    /// everything composed so far when a fullscreen opaque cover proves it
    /// invisible — a new scene column added here resets on both paths at once.
    pub(crate) fn discard_scene(&mut self) {
        self.quads.clear();
        self.texts.clear();
        self.meshes.clear();
        self.images.clear();
        self.frame_targets.clear();
        self.groups.clear();
        self.text_batches.clear();
        self.mesh_batches.clear();
        self.image_batches.clear();
        self.curves.clear();
        self.curve_batches.clear();
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

#[cfg(test)]
mod tests {
    use super::RenderBuffer;
    use crate::renderer::render_buffer::owner::RenderOwnerId;

    #[test]
    fn render_owner_is_explicit_and_unique() {
        let first = RenderOwnerId::reserve();
        let second = RenderOwnerId::reserve();
        assert_ne!(first, second);

        let buffer = RenderBuffer::new(first);
        assert_eq!(buffer.owner, first);
    }
}
