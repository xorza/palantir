//! App-supplied triangle geometry: the GPU vertex, the indexed mesh an app
//! builds, and the screens that keep a malformed one from the renderer.

use crate::common::content_hash::ContentHash;
use crate::common::hash::Hasher;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::rect::aabb::Aabb;
use crate::primitives::math::domain::vec2;
use crate::primitives::math::float_hash::FloatHash;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::srgba_u8::SrgbaU8;
use bytemuck::{Pod, Zeroable};
use glam::Vec2;
use std::hash::Hasher as _;

/// One vertex of a user-supplied mesh: 12 B (pos 8 + color 4), no padding,
/// castable straight into a wgpu vertex buffer.
///
/// `pos` is in owner-local logical px (origin at the owner-rect top-left,
/// after `local_rect.min`); the composer bakes in the transform and DPI scale.
///
/// `color` is sRGB-encoded bytes with straight alpha: a hex color survives
/// exactly, any other lands within half a display step. The shader decodes it
/// per vertex, so the rasterizer interpolates linear light, and premultiplies
/// at output.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Pod, Zeroable)]
pub(crate) struct MeshVertex {
    /// Position in the owner's local logical pixels.
    pub(crate) pos: Vec2,
    /// Vertex color, sRGB-encoded.
    pub(crate) color: SrgbaU8,
}

impl MeshVertex {
    /// Construct at `pos`, `color` encoded to sRGB bytes, as a gradient stop stores its
    /// color. An `SrgbaU8` decodes and encodes back to itself exactly.
    pub(crate) fn new(pos: Vec2, color: impl Into<RgbaF32>) -> Self {
        Self {
            pos,
            color: color.into().to_srgba8(),
        }
    }
}

/// User-side mesh builder. The framework copies the slices into the active
/// `Tree`'s arena at `add_shape`, so the `Mesh` need only outlive that call.
///
/// Indices are `u32` (`wgpu::IndexFormat::Uint32`). Winding is conventionally
/// CCW but the pipeline does not cull.
#[derive(Default, Clone, Debug)]
pub struct Mesh {
    /// Private, as the hashes and the bounds follow every push: a write past the mutators would
    /// leave them stale.
    vertices: Vec<MeshVertex>,
    idxs: Vec<u32>,
    /// The vertices hashed as they are pushed, and the indices beside them: each mutator only
    /// appends, so [`Self::content_hash`] reads two states and a retained mesh hashes nothing
    /// per frame. Path-independent: one write per vertex and per index, however they came in.
    vertex_hash: Hasher,
    idx_hash: Hasher,
    /// The owner-local AABB of the vertices, grown as each is pushed; `None` while empty.
    bounds: Option<Aabb>,
}

impl Mesh {
    /// An empty mesh, allocating nothing.
    #[inline]
    pub const fn new() -> Self {
        Self {
            vertices: Vec::new(),
            idxs: Vec::new(),
            vertex_hash: Hasher::new(),
            idx_hash: Hasher::new(),
            bounds: None,
        }
    }

    /// [`Self::new`] with both buffers reserved.
    #[inline]
    pub fn with_capacity(vertices: usize, idxs: usize) -> Self {
        Self {
            vertices: Vec::with_capacity(vertices),
            idxs: Vec::with_capacity(idxs),
            vertex_hash: Hasher::new(),
            idx_hash: Hasher::new(),
            bounds: None,
        }
    }

    /// Drop the contents, keeping capacity.
    #[inline]
    pub fn clear(&mut self) {
        self.vertices.clear();
        self.idxs.clear();
        self.vertex_hash = Hasher::new();
        self.idx_hash = Hasher::new();
        self.bounds = None;
    }

    /// Non-paintable: no vertices, or indices not forming whole triangles. Mirrors
    /// `DrawMeshPayload::is_noop`.
    #[inline]
    pub const fn is_noop(&self) -> bool {
        self.vertices.is_empty() || self.idxs.len() < 3 || !self.idxs.len().is_multiple_of(3)
    }

    pub(crate) const fn vertices(&self) -> &[MeshVertex] {
        self.vertices.as_slice()
    }

    pub(crate) const fn idxs(&self) -> &[u32] {
        self.idxs.as_slice()
    }

    /// Stable visual hash of vertices and indices, the same however the mesh was built.
    pub fn content_hash(&self) -> ContentHash {
        let mut h = self.vertex_hash.clone();
        h.write_u64(self.idx_hash.finish());
        ContentHash::finish(&h)
    }

    /// Push `vertex`, hashing it and growing the bounds.
    #[inline]
    fn push_vertex(&mut self, vertex: MeshVertex) {
        self.hash_vertex(vertex);
        match &mut self.bounds {
            Some(bounds) => bounds.push(vertex.pos),
            None => self.bounds = Some(Aabb::at(vertex.pos)),
        }
        self.vertices.push(vertex);
    }

    /// Fold `vertex` into the vertex hash: the one write per vertex, whichever way it comes in.
    #[inline]
    fn hash_vertex(&mut self, vertex: MeshVertex) {
        vertex.pos.hash_visual(&mut self.vertex_hash);
        self.vertex_hash.write_u32(vertex.color.to_u32());
    }

    /// Push index `idx`, hashing it: the one way an index comes in.
    #[inline]
    fn push_idx(&mut self, idx: u32) {
        self.idx_hash.write_u32(idx);
        self.idxs.push(idx);
    }

    /// Push a vertex; returns its index for [`Self::add_triangle`].
    ///
    /// # Panics
    ///
    /// Panics unless `pos` is an [offset](crate::widget::domain::offset), or
    /// if the new vertex index cannot be represented by `u32`.
    #[inline]
    #[track_caller]
    pub fn add_vertex(&mut self, pos: Vec2, color: impl Into<RgbaF32>) -> u32 {
        let idx = checked_vertex_idx(self.vertices.len());
        self.push_vertex(MeshVertex::new(vec2::offset(pos), color));
        idx
    }

    /// Push three indices (CCW by convention).
    ///
    /// # Panics
    ///
    /// Panics if an index does not refer to an existing vertex: one comparison per triangle, as
    /// a wrong value never draws quietly.
    #[inline]
    #[track_caller]
    pub fn add_triangle(&mut self, a: u32, b: u32, c: u32) {
        assert!(
            (a.max(b).max(c) as usize) < self.vertices.len(),
            "mesh triangle indices [{a}, {b}, {c}] exceed vertex count {}",
            self.vertices.len(),
        );
        self.push_idx(a);
        self.push_idx(b);
        self.push_idx(c);
    }

    /// Append another mesh, offsetting its indices into this mesh's vertex
    /// space. Public because `vertices` and `idxs` are private, so a
    /// consumer cannot compose meshes otherwise.
    ///
    /// # Panics
    ///
    /// Panics if the combined vertex indices cannot be represented by `u32`.
    pub fn append(&mut self, other: &Self) {
        if other.vertices.is_empty() {
            return;
        }
        let combined_vertex_count = self
            .vertices
            .len()
            .checked_add(other.vertices.len())
            .expect("combined mesh vertex count overflowed usize");
        checked_vertex_idx(combined_vertex_count - 1);
        let base = checked_vertex_idx(self.vertices.len());
        for &vertex in &other.vertices {
            self.hash_vertex(vertex);
        }
        self.vertices.extend_from_slice(&other.vertices);
        let other_bounds = other.bounds.expect("a mesh with vertices has bounds");
        self.bounds = Some(
            self.bounds
                .map_or(other_bounds, |bounds| bounds.union(other_bounds)),
        );
        self.idxs.reserve(other.idxs.len());
        for &idx in &other.idxs {
            self.push_idx(checked_rebased_idx(base, idx));
        }
    }

    /// Owner-local AABB of `vertices`; empty gives `Rect::ZERO`.
    pub const fn bbox(&self) -> Rect {
        match self.bounds {
            Some(bounds) => bounds.finish(),
            None => Rect::ZERO,
        }
    }

    /// Filled convex polygon (fan around the first vertex).
    /// A non-convex polygon renders wrong.
    ///
    /// # Panics
    ///
    /// As [`Self::add_vertex`].
    #[track_caller]
    pub fn filled_polygon(points: &[Vec2], color: impl Into<RgbaF32>) -> Self {
        if points.len() < 3 {
            return Self::new();
        }
        let color = color.into();
        let mut m = Self::with_capacity(points.len(), (points.len() - 2) * 3);
        let i0 = m.add_vertex(points[0], color);
        let mut prev = m.add_vertex(points[1], color);
        for &p in &points[2..] {
            let next = m.add_vertex(p, color);
            m.add_triangle(i0, prev, next);
            prev = next;
        }
        m
    }
}

#[inline]
fn checked_vertex_idx(idx: usize) -> u32 {
    u32::try_from(idx).expect("mesh vertex index exceeds u32 range")
}

#[inline]
const fn checked_rebased_idx(base: u32, idx: u32) -> u32 {
    base.checked_add(idx)
        .expect("appended mesh index exceeds u32 range")
}

#[cfg(test)]
mod tests;
