//! Mesh payload spans and per-draw GPU instance data.

#![expect(
    clippy::expl_impl_clone_on_copy,
    reason = "`soa_rs`'s `Soars` derive writes `Clone` by hand for the `Copy` rows it generates"
)]

use crate::common::span::Span;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use glam::Vec2;
use soa_rs::Soars;

/// One mesh draw within a group; vertex/index slices live in the recording's [`RecordStore::meshes`](crate::scene::record_store::RecordStore::meshes), transform and tint in [`MeshDrawRow::instance`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct MeshDraw {
    pub(crate) vertices: Span,
    pub(crate) indices: Span,
}

/// One mesh draw row; the SoA split lets the backend upload `rows.instance()` in one `write_buffer`.
#[derive(Soars, Clone, Copy, Debug, PartialEq)]
#[soa_derive(Debug)]
pub(crate) struct MeshDrawRow {
    pub(crate) draw: MeshDraw,
    pub(crate) instance: MeshInstance,
}

/// Per-mesh GPU state in a `step_mode: Instance` buffer: `physical = pos * scale + translate`, `out_color = vertex.color * tint`.
#[padding_struct::padding_struct]
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct MeshInstance {
    pub(crate) translate: Vec2,
    pub(crate) scale: f32,
    pub(crate) tint: RgbaF16,
}
