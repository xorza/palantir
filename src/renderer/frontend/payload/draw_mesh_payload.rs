//! One indexed-triangle mesh draw.

use crate::primitives::color::ColorF16;
use crate::primitives::rect::Rect;

/// Mesh draw payload. Vertex/index data lives in the window's
/// [`RecordPayloads`] (`meshes`); the payload only carries the spans
/// (owner-local). The composer folds `origin` (owner-rect top-left)
/// into the per-instance translate so the vertex stream stays
/// content-stable across frames.
///
/// [`RecordPayloads`]: crate::scene::record_store::record_payloads::RecordPayloads
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DrawMeshPayload {
    /// Owner-local AABB of `vertices`. The composer transforms the
    /// four corners (uniform-scale `TranslateScale` preserves AABBs)
    /// after adding `origin`, scales to physical px, and uses the
    /// result for the overlap test + scissor cull.
    pub(crate) bbox: Rect,
    pub(crate) origin: glam::Vec2,
    pub(crate) tint: ColorF16,
    pub(crate) v_start: u32,
    pub(crate) v_len: u32,
    pub(crate) i_start: u32,
    pub(crate) i_len: u32,
}

impl DrawMeshPayload {
    /// Paints nothing when: empty vertex buffer, fewer than
    /// one full triangle, an index count that isn't a multiple of 3,
    /// or fully transparent tint.
    #[inline]
    pub(crate) fn is_noop(&self) -> bool {
        self.v_len == 0 || self.i_len < 3 || !self.i_len.is_multiple_of(3) || self.tint.is_noop()
    }
}
