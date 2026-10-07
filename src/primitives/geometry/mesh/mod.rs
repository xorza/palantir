//! App-supplied triangle geometry: the GPU vertex, the indexed mesh an app
//! builds, and the screens that keep a malformed one from the renderer.

use crate::common::hash::Hasher;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::rect::aabb::Aabb;
use crate::primitives::math::domain::vec2;
use crate::primitives::math::float_hash::FloatHash;
use crate::primitives::paint::color::srgba_u8::SrgbaU8;
use bytemuck::{Pod, Zeroable};
use glam::Vec2;
use std::cell::Cell;
use std::hash::Hasher as _;

/// One vertex of a user-supplied mesh: 12 B (pos 8 + color 4), no padding,
/// castable straight into a wgpu vertex buffer.
///
/// `pos` is in owner-local logical px (origin at the owner-rect top-left,
/// after `local_rect.min`); the composer bakes in the transform and DPI scale.
///
/// `color` is sRGB-encoded bytes with straight alpha: a hex colour survives
/// exactly, any other lands within half a display step. The shader decodes it
/// per vertex, so the rasterizer interpolates linear light, and premultiplies
/// at output.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct MeshVertex {
    /// Position in the owner's local logical pixels.
    pub pos: Vec2,
    /// Vertex colour, sRGB-encoded.
    pub color: SrgbaU8,
}

impl MeshVertex {
    /// Construct at `pos`; an `RgbaF32` colour is encoded exactly here, an
    /// `SrgbaU8` stored as is.
    pub fn new(pos: Vec2, color: impl Into<SrgbaU8>) -> Self {
        Self {
            pos,
            color: color.into(),
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
    pub(crate) vertices: Vec<MeshVertex>,
    pub(crate) indices: Vec<u32>,
    /// The largest index pushed, so [`Self::is_noop`] screens an index past
    /// the last vertex at one compare. Meaningless while `indices` is empty.
    max_index: u32,
    /// Lazy cache of `content_hash`, cleared by every public mutator.
    /// Internal arena pushes bypass it; arena meshes never hash. It turns a
    /// retained mesh's per-frame O(n) re-hash into a hit.
    cached_hash: Cell<Option<u64>>,
    /// Lazy cache of the owner-local AABB, same contract as `cached_hash`.
    cached_bbox: Cell<Option<Rect>>,
}

impl Mesh {
    /// An empty mesh, allocating nothing.
    #[inline]
    pub const fn new() -> Self {
        Self {
            vertices: Vec::new(),
            indices: Vec::new(),
            max_index: 0,
            cached_hash: Cell::new(None),
            cached_bbox: Cell::new(None),
        }
    }

    /// [`Self::new`] with both buffers reserved.
    #[inline]
    pub fn with_capacity(vertices: usize, indices: usize) -> Self {
        Self {
            vertices: Vec::with_capacity(vertices),
            indices: Vec::with_capacity(indices),
            max_index: 0,
            cached_hash: Cell::new(None),
            cached_bbox: Cell::new(None),
        }
    }

    /// Drop the contents and cached hash and bbox, keeping capacity.
    #[inline]
    pub fn clear(&mut self) {
        self.vertices.clear();
        self.indices.clear();
        self.max_index = 0;
        self.cached_hash.set(None);
        self.cached_bbox.set(None);
    }

    /// Non-paintable: no vertices, indices not forming whole triangles, or an
    /// index past the last vertex. Mirrors `DrawMeshPayload::is_noop`.
    #[inline]
    pub fn is_noop(&self) -> bool {
        self.vertices.is_empty()
            || self.indices.len() < 3
            || !self.indices.len().is_multiple_of(3)
            // A release build reaches here with what `triangle`'s debug
            // assert would have caught.
            || self.max_index as usize >= self.vertices.len()
            // A NaN vertex reaches the AABB by the fold's NaN contract, so
            // this memoized read stands in for scanning every position.
            || self.bbox().has_nan()
    }

    /// Stable visual hash of vertices and indices. Memoized until a public
    /// mutator runs.
    pub fn content_hash(&self) -> u64 {
        if let Some(h) = self.cached_hash.get() {
            return h;
        }
        let mut h = Hasher::new();
        for vertex in &self.vertices {
            vertex.pos.hash_visual(&mut h);
            h.write_u32(vertex.color.to_u32());
        }
        h.pod_slice(self.indices.as_slice());
        let v = h.finish();
        self.cached_hash.set(Some(v));
        v
    }

    /// Push a vertex; returns its index for [`Self::triangle`].
    ///
    /// # Panics
    ///
    /// Panics unless `pos` is an [offset](crate::widget::domain::offset), or
    /// if the new vertex index cannot be represented by `u32`.
    #[inline]
    #[track_caller]
    pub fn vertex(&mut self, pos: Vec2, color: impl Into<SrgbaU8>) -> u32 {
        let index = checked_vertex_index(self.vertices.len());
        self.vertices
            .push(MeshVertex::new(vec2::offset(pos), color));
        self.cached_hash.set(None);
        self.cached_bbox.set(None);
        index
    }

    /// Push three indices (CCW by convention).
    ///
    /// # Panics
    ///
    /// Panics in a debug build if an index does not refer to an existing
    /// vertex. Debug-only because it runs per triangle of a per-frame build; a
    /// release build draws nothing instead ([`Self::is_noop`]).
    #[inline]
    pub fn triangle(&mut self, a: u32, b: u32, c: u32) {
        debug_assert!(
            (a.max(b).max(c) as usize) < self.vertices.len(),
            "mesh triangle indices [{a}, {b}, {c}] exceed vertex count {}",
            self.vertices.len(),
        );
        self.indices.push(a);
        self.indices.push(b);
        self.indices.push(c);
        self.max_index = self.max_index.max(a.max(b).max(c));
        self.cached_hash.set(None);
    }

    /// Append another mesh, offsetting its indices into this mesh's vertex
    /// space. Public because `vertices` and `indices` are private, so a
    /// consumer cannot compose meshes otherwise.
    /// # Panics
    ///
    /// Panics if the combined vertex indices cannot be represented by `u32`.
    pub fn append(&mut self, other: &Mesh) {
        if other.vertices.is_empty() {
            return;
        }
        let combined_vertex_count = self
            .vertices
            .len()
            .checked_add(other.vertices.len())
            .expect("combined mesh vertex count overflowed usize");
        checked_vertex_index(combined_vertex_count - 1);
        let base = checked_vertex_index(self.vertices.len());
        self.vertices.extend_from_slice(&other.vertices);
        self.indices.reserve(other.indices.len());
        for &index in &other.indices {
            self.indices.push(checked_rebased_index(base, index));
        }
        if !other.indices.is_empty() {
            self.max_index = self
                .max_index
                .max(checked_rebased_index(base, other.max_index));
        }
        self.cached_hash.set(None);
        self.cached_bbox.set(None);
    }

    /// Owner-local AABB of `vertices`, memoized; empty gives `Rect::ZERO`.
    pub fn bbox(&self) -> Rect {
        if let Some(b) = self.cached_bbox.get() {
            return b;
        }
        let b = compute_aabb(&self.vertices);
        self.cached_bbox.set(Some(b));
        b
    }

    /// Filled triangle in a single colour, with its bbox pre-cached.
    ///
    /// # Panics
    ///
    /// As [`Self::vertex`].
    #[track_caller]
    pub fn filled_triangle(a: Vec2, b: Vec2, c: Vec2, color: impl Into<SrgbaU8>) -> Self {
        let color = color.into();
        let mut m = Self::with_capacity(3, 3);
        let i0 = m.vertex(a, color);
        let i1 = m.vertex(b, color);
        let i2 = m.vertex(c, color);
        m.triangle(i0, i1, i2);
        // Through `Aabb`, not a bare `min`/`max` fold: those drop a NaN
        // operand and would hand `is_noop` a finite box for a NaN vertex.
        m.cached_bbox.set(Some(Aabb::of_iter([a, b, c])));
        m
    }

    /// Filled convex polygon (fan around the first vertex), bbox pre-cached.
    /// A non-convex polygon renders wrong.
    ///
    /// # Panics
    ///
    /// As [`Self::vertex`].
    #[track_caller]
    pub fn filled_polygon(points: &[Vec2], color: impl Into<SrgbaU8>) -> Self {
        if points.len() < 3 {
            return Self::new();
        }
        let color = color.into();
        let mut m = Self::with_capacity(points.len(), (points.len() - 2) * 3);
        let i0 = m.vertex(points[0], color);
        let mut prev = m.vertex(points[1], color);
        for &p in &points[2..] {
            let next = m.vertex(p, color);
            m.triangle(i0, prev, next);
            prev = next;
        }
        // Through `Aabb`, not folded into the fan: a bare `min`/`max` fold
        // drops a NaN operand.
        m.cached_bbox.set(Some(Aabb::of(points)));
        m
    }
}

#[inline]
fn checked_vertex_index(index: usize) -> u32 {
    u32::try_from(index).expect("mesh vertex index exceeds u32 range")
}

#[inline]
const fn checked_rebased_index(base: u32, index: u32) -> u32 {
    base.checked_add(index)
        .expect("appended mesh index exceeds u32 range")
}

// Not fused into the copy loops in `shape/lower/`: splitting the AABB pass
// from the copy is ~3x faster past a handful of points, since the fold
// vectorizes and the copy becomes one `memcpy`.
fn compute_aabb(verts: &[MeshVertex]) -> Rect {
    Aabb::of_iter(verts.iter().map(|v| v.pos))
}

#[cfg(test)]
mod tests;
