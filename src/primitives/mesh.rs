use crate::primitives::color::{Color, ColorU8};
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use bytemuck::{Pod, Zeroable};
use glam::Vec2;
use rustc_hash::FxHasher;
use std::cell::Cell;
use std::hash::Hasher;

/// One vertex of a user-supplied mesh. 12 B (pos 8 + color 4), no
/// padding — directly castable into a wgpu vertex buffer.
///
/// `pos` is in **owner-local logical px** (origin = the shape's
/// owner-rect top-left, after `local_rect.min` offset if set). The
/// composer bakes the accumulated transform + DPI scale into a
/// physical-px copy at compose time.
///
/// `color` is **linear RGBA, premultiplied**, stored as `ColorU8`
/// (8 bits per channel, linear-space — the default `From<Color> for
/// ColorU8` is a linear quantize, no sRGB encoding). The GPU vertex
/// attribute is `Unorm8x4`, so `u8/255` lands in the rasterizer as
/// `0..1` linear floats with no shader decode. Banding in dark
/// gradients across a mesh face is the trade-off for the 12 B vertex
/// footprint vs. 24 B.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct MeshVertex {
    pub pos: Vec2,
    pub color: ColorU8,
}

impl MeshVertex {
    /// Construct from a linear `Color`; quantizes to `ColorU8`
    /// (linear u8, no sRGB encoding) at the boundary.
    pub fn new(pos: Vec2, color: Color) -> Self {
        Self {
            pos,
            color: color.into(),
        }
    }
}

/// User-side mesh builder. The framework copies the vertex/index
/// slices into the active `Tree`'s arena at `add_shape` time, so the
/// `Mesh` only has to outlive the `add_shape` call.
///
/// Indices are `u16`. 65 535 verts per mesh is enormous for a UI
/// primitive; revisit `u32` when a workload actually needs it.
///
/// Winding is conventionally CCW but the pipeline doesn't cull —
/// either order paints.
#[derive(Default, Clone, Debug)]
pub struct Mesh {
    pub(crate) vertices: Vec<MeshVertex>,
    pub(crate) indices: Vec<u16>,
    /// Lazy cache of `content_hash`. `None` = not computed or
    /// invalidated. Set by `content_hash`; cleared by every public
    /// mutator. Internal arena pushes bypass the cache by going
    /// straight at `pub(crate)` fields — fine, since arena meshes
    /// never call `content_hash`.
    cached_hash: Cell<Option<u64>>,
    /// Lazy cache of owner-local AABB. Same memoization contract as
    /// `cached_hash`; populated by [`Self::bbox`], invalidated by
    /// every public mutator. [`Self::with_known_bbox`] pre-seeds it
    /// to skip the compute entirely.
    cached_bbox: Cell<Option<Rect>>,
}

impl Mesh {
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    #[inline]
    pub fn with_capacity(vertices: usize, indices: usize) -> Self {
        Self {
            vertices: Vec::with_capacity(vertices),
            indices: Vec::with_capacity(indices),
            cached_hash: Cell::new(None),
            cached_bbox: Cell::new(None),
        }
    }

    #[inline]
    pub fn clear(&mut self) {
        self.vertices.clear();
        self.indices.clear();
        self.cached_hash.set(None);
        self.cached_bbox.set(None);
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.vertices.is_empty() || self.indices.is_empty()
    }

    /// Stable hash of vertex + index bytes. Memoized — repeat calls on
    /// an unmutated mesh return the cached value. Mutating through any
    /// public method invalidates the cache.
    pub fn content_hash(&self) -> u64 {
        if let Some(h) = self.cached_hash.get() {
            return h;
        }
        let mut h = FxHasher::default();
        h.write(bytemuck::cast_slice(self.vertices.as_slice()));
        h.write(bytemuck::cast_slice(self.indices.as_slice()));
        let v = h.finish();
        self.cached_hash.set(Some(v));
        v
    }

    /// Push a vertex; returns its `u16` index for use in [`Self::triangle`].
    /// Panics if the mesh already holds 65 535 vertices.
    #[inline]
    pub fn vertex(&mut self, pos: Vec2, color: Color) -> u16 {
        let idx = self.vertices.len();
        assert!(idx < u16::MAX as usize, "Mesh exceeds u16 vertex limit");
        self.vertices.push(MeshVertex::new(pos, color));
        self.cached_hash.set(None);
        self.cached_bbox.set(None);
        idx as u16
    }

    /// Push three indices (CCW by convention).
    #[inline]
    pub fn triangle(&mut self, a: u16, b: u16, c: u16) {
        self.indices.push(a);
        self.indices.push(b);
        self.indices.push(c);
        self.cached_hash.set(None);
    }

    /// Append another mesh, offsetting its indices into this mesh's
    /// vertex space.
    pub fn append(&mut self, other: &Mesh) {
        let base = self.vertices.len();
        assert!(
            base + other.vertices.len() <= u16::MAX as usize,
            "Mesh::append would overflow u16 vertex index space"
        );
        let base = base as u16;
        self.vertices.extend_from_slice(&other.vertices);
        self.indices.reserve(other.indices.len());
        for &i in &other.indices {
            self.indices.push(base + i);
        }
        self.cached_hash.set(None);
        self.cached_bbox.set(None);
    }

    /// Owner-local AABB of `vertices`. Memoized; first call after any
    /// public mutation does one O(n) pass, repeat calls are free.
    /// Empty mesh returns `Rect::ZERO`.
    pub fn bbox(&self) -> Rect {
        if let Some(b) = self.cached_bbox.get() {
            return b;
        }
        let b = compute_aabb(&self.vertices);
        self.cached_bbox.set(Some(b));
        b
    }

    /// Skip the lazy compute by handing over a pre-computed AABB.
    /// Caller is responsible for correctness — a wrong bbox silently
    /// breaks scissor culling. Use for procedural / baked meshes where
    /// the AABB falls out of the construction algorithm.
    pub fn with_known_bbox(self, bbox: Rect) -> Self {
        self.cached_bbox.set(Some(bbox));
        self
    }

    /// Convenience: filled triangle in a single color.
    pub fn filled_triangle(a: Vec2, b: Vec2, c: Vec2, color: Color) -> Self {
        let mut m = Self::with_capacity(3, 3);
        let i0 = m.vertex(a, color);
        let i1 = m.vertex(b, color);
        let i2 = m.vertex(c, color);
        m.triangle(i0, i1, i2);
        m
    }

    /// Convenience: filled convex polygon (fan triangulation around the
    /// first vertex). For non-convex polygons the result is visually
    /// wrong — caller's responsibility.
    pub fn filled_polygon(points: &[Vec2], color: Color) -> Self {
        if points.len() < 3 {
            return Self::new();
        }
        let mut m = Self::with_capacity(points.len(), (points.len() - 2) * 3);
        let i0 = m.vertex(points[0], color);
        let mut prev = m.vertex(points[1], color);
        for &p in &points[2..] {
            let next = m.vertex(p, color);
            m.triangle(i0, prev, next);
            prev = next;
        }
        m
    }
}

fn compute_aabb(verts: &[MeshVertex]) -> Rect {
    let Some((first, rest)) = verts.split_first() else {
        return Rect::ZERO;
    };
    let mut lo = first.pos;
    let mut hi = first.pos;
    for v in rest {
        lo = lo.min(v.pos);
        hi = hi.max(v.pos);
    }
    Rect {
        min: lo,
        size: Size {
            w: hi.x - lo.x,
            h: hi.y - lo.y,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mesh_vertex_is_12_bytes_no_padding() {
        assert_eq!(std::mem::size_of::<MeshVertex>(), 12);
    }

    #[test]
    fn mesh_vertex_pod_roundtrip() {
        let v = MeshVertex::new(
            Vec2::new(1.0, 2.0),
            Color {
                r: 0.1,
                g: 0.2,
                b: 0.3,
                a: 0.4,
            },
        );
        let bytes = bytemuck::bytes_of(&v);
        let back: MeshVertex = *bytemuck::from_bytes(bytes);
        assert_eq!(back, v);
    }

    #[test]
    fn triangle_indices_offset_in_append() {
        let mut a = Mesh::filled_triangle(Vec2::ZERO, Vec2::X, Vec2::Y, Color::default());
        let b = Mesh::filled_triangle(Vec2::ZERO, Vec2::X, Vec2::Y, Color::default());
        a.append(&b);
        assert_eq!(a.vertices.len(), 6);
        assert_eq!(a.indices, vec![0, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn polygon_fan_indices_share_pivot() {
        let pts = [
            Vec2::new(0.0, 0.0),
            Vec2::new(1.0, 0.0),
            Vec2::new(1.0, 1.0),
            Vec2::new(0.0, 1.0),
        ];
        let m = Mesh::filled_polygon(&pts, Color::default());
        assert_eq!(m.vertices.len(), 4);
        assert_eq!(m.indices, vec![0, 1, 2, 0, 2, 3]);
    }

    fn red_tri() -> Mesh {
        let red = Color {
            r: 1.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        };
        Mesh::filled_triangle(Vec2::ZERO, Vec2::X, Vec2::Y, red)
    }

    #[test]
    fn content_hash_stable_for_identical_input() {
        let a = red_tri();
        let b = red_tri();
        assert_eq!(a.content_hash(), b.content_hash());
    }

    #[test]
    fn content_hash_changes_on_reordered_indices() {
        let mut a = red_tri();
        let mut b = red_tri();
        a.indices = vec![0, 1, 2];
        a.cached_hash.set(None);
        b.indices = vec![0, 2, 1];
        b.cached_hash.set(None);
        assert_ne!(a.content_hash(), b.content_hash());
    }

    #[test]
    fn content_hash_memoizes_until_mutation() {
        let mut m = red_tri();
        let h0 = m.content_hash();
        assert_eq!(m.cached_hash.get(), Some(h0));
        // No mutation → same value, cache still populated.
        assert_eq!(m.content_hash(), h0);
        assert_eq!(m.cached_hash.get(), Some(h0));
        // Any builder mutation invalidates.
        m.vertex(Vec2::new(2.0, 2.0), Color::default());
        assert_eq!(m.cached_hash.get(), None);
        let h1 = m.content_hash();
        assert_ne!(h0, h1);
    }

    #[test]
    fn clone_preserves_cache() {
        let m = red_tri();
        let h = m.content_hash();
        let c = m.clone();
        assert_eq!(c.cached_hash.get(), Some(h));
    }

    #[test]
    fn bbox_empty_mesh_is_zero() {
        assert_eq!(Mesh::new().bbox(), Rect::ZERO);
    }

    #[test]
    fn bbox_spans_vertex_extent() {
        let m = Mesh::filled_triangle(
            Vec2::new(-1.0, 2.0),
            Vec2::new(4.0, 2.0),
            Vec2::new(0.0, 7.0),
            Color::default(),
        );
        let b = m.bbox();
        assert_eq!(b.min, Vec2::new(-1.0, 2.0));
        assert_eq!(b.size.w, 5.0);
        assert_eq!(b.size.h, 5.0);
    }

    #[test]
    fn bbox_memoizes_until_mutation() {
        let mut m = red_tri();
        let b0 = m.bbox();
        assert_eq!(m.cached_bbox.get(), Some(b0));
        m.vertex(Vec2::new(10.0, 10.0), Color::default());
        assert_eq!(m.cached_bbox.get(), None);
        let b1 = m.bbox();
        assert_ne!(b0, b1);
    }

    #[test]
    fn with_known_bbox_skips_compute() {
        let bogus = Rect {
            min: Vec2::new(100.0, 100.0),
            size: Size { w: 1.0, h: 1.0 },
        };
        let m = red_tri().with_known_bbox(bogus);
        assert_eq!(m.bbox(), bogus);
    }

    #[test]
    fn clear_invalidates_bbox() {
        let mut m = red_tri();
        let _ = m.bbox();
        m.clear();
        assert_eq!(m.cached_bbox.get(), None);
        assert_eq!(m.bbox(), Rect::ZERO);
    }

    #[test]
    fn triangle_keeps_bbox_cache() {
        let mut m = red_tri();
        let b0 = m.bbox();
        assert_eq!(m.cached_bbox.get(), Some(b0));
        // Pushing indices doesn't move any vertices, so bbox stays valid.
        m.triangle(0, 1, 2);
        assert_eq!(m.cached_bbox.get(), Some(b0));
        // ...but content_hash must invalidate — render output changed.
        assert_eq!(m.cached_hash.get(), None);
    }

    #[test]
    fn append_invalidates_bbox() {
        let mut a = red_tri();
        let _ = a.bbox();
        let b = Mesh::filled_triangle(
            Vec2::new(10.0, 10.0),
            Vec2::new(11.0, 10.0),
            Vec2::new(10.0, 11.0),
            Color::default(),
        );
        a.append(&b);
        assert_eq!(a.cached_bbox.get(), None);
        let bb = a.bbox();
        assert_eq!(bb.min, Vec2::ZERO);
        assert_eq!(bb.size.w, 11.0);
        assert_eq!(bb.size.h, 11.0);
    }
}
