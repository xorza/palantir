//! [`ShadowInstance`]: a shadow's instance in the quad stream.

use crate::primitives::packed::half_simd::F16x4;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::renderer::render_buffer::quad_draw_row::QuadInstance;
use crate::renderer::render_buffer::shape_header::ShapeHeader;
use bytemuck::{Pod, Zeroable};

/// A shadow's instance, a [`QuadInstance`]'s size, so it lies in the quad stream among the fills it
/// paints under or over, in record order. The shadow pipelines read it through their own
/// attributes (`SHADOW_INSTANCE_ATTRS`), its lanes named as a shadow's. Refused: a shadow's lanes
/// under a fill's names, its offset, blur and spread in a gradient's axis; a stream of its own,
/// which would move shadows to a tier and close a group at each shadowed box; a Rust union with a
/// `QuadInstance`, whose reads are `unsafe`.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Pod, Zeroable)]
pub(crate) struct ShadowInstance {
    /// The quad the shadow paints over and its source's radii: a drop shadow's source moved by
    /// its offset and grown by its halo, an inset shadow's source itself. The shader recovers
    /// the source from the quad's center.
    pub(crate) header: ShapeHeader,
    /// Linear-RGB straight-alpha color.
    pub(crate) color: RgbaF16,
    pub(crate) kind: ShadowKind,
    /// `(offset.x, offset.y, σ, spread)` in physical px.
    pub(crate) geometry: F16x4,
    /// Unread: a quad's lanes past a shadow's, so the two share one instance stride.
    pub(crate) unused: [u32; 4],
}

impl ShadowInstance {
    /// The blur's standard deviation, physical px.
    pub(crate) fn sigma(&self) -> f32 {
        self.geometry.lanes()[2]
    }
}

/// Whether a shadow lies outside its source, under it, or inside it, over its fill.
#[repr(transparent)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
pub(crate) struct ShadowKind(u32);

impl ShadowKind {
    /// A drop shadow, read as `SHADOW_DROP`.
    pub(crate) const DROP: Self = Self(0);
    /// An inset shadow, read as `SHADOW_INSET`.
    pub(crate) const INSET: Self = Self(1);

    /// The kind as the shader compares it.
    pub(crate) const fn bits(self) -> u32 {
        self.0
    }
}

impl From<ShadowInstance> for QuadInstance {
    /// The shadow in a quad's bytes, as the stream holds it; its row's key says shadow.
    fn from(shadow: ShadowInstance) -> Self {
        bytemuck::cast(shadow)
    }
}

impl From<QuadInstance> for ShadowInstance {
    /// A shadow row's instance read back as the shadow it holds.
    fn from(quad: QuadInstance) -> Self {
        bytemuck::cast(quad)
    }
}

// One stride, and the header where every quad pipeline reads it.
const _: () = {
    assert!(size_of::<ShadowInstance>() == size_of::<QuadInstance>());
    assert!(
        std::mem::offset_of!(ShadowInstance, header) == std::mem::offset_of!(QuadInstance, header)
    );
};
