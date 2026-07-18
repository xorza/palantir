use crate::primitives::rect::Rect;
use crate::primitives::{approx, color::ColorU8};
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
/// `color` is **straight-alpha linear RGBA** — the mesh shader
/// premultiplies at output — stored as `ColorU8` (8 bits per channel,
/// linear-space — the default `From<Color> for ColorU8` is a linear
/// quantize, no sRGB encoding). The GPU vertex
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
    /// Construct at `pos` with any `Into<ColorU8>` colour — accepts a
    /// linear `Color` (quantized at the boundary) or a `ColorU8`
    /// (passthrough), so call sites that already hold quantized colour
    /// don't round-trip through f32.
    pub fn new(pos: Vec2, color: impl Into<ColorU8>) -> Self {
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
/// Indices are `u32` — the mesh pipeline draws its shared arena index
/// stream with `wgpu::IndexFormat::Uint32`.
///
/// Winding is conventionally CCW but the pipeline doesn't cull —
/// either order paints.
#[derive(Default, Clone, Debug)]
pub struct Mesh {
    pub(crate) vertices: Vec<MeshVertex>,
    pub(crate) indices: Vec<u32>,
    /// Lazy cache of `content_hash`. `None` = not computed or
    /// invalidated. Set by `content_hash`; cleared by every public
    /// mutator. Internal arena pushes bypass the cache by going
    /// straight at `pub(crate)` fields — fine, since arena meshes
    /// never call `content_hash`. A retained `Mesh` redrawn every frame
    /// is lowered (and so hashed) once per frame; the cache turns that
    /// per-frame O(n) re-hash into a hit after the first frame.
    cached_hash: Cell<Option<u64>>,
    /// Lazy cache of owner-local AABB. Same memoization contract as
    /// `cached_hash` — a retained mesh re-lowered each frame would
    /// otherwise recompute its AABB every frame. [`Self::with_known_bbox`]
    /// pre-seeds it to skip the compute entirely.
    cached_bbox: Cell<Option<Rect>>,
}

impl Mesh {
    #[inline]
    pub const fn new() -> Self {
        Self {
            vertices: Vec::new(),
            indices: Vec::new(),
            cached_hash: Cell::new(None),
            cached_bbox: Cell::new(None),
        }
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

    /// Non-paintable: missing vertices, or indices that don't form whole
    /// triangles. Mirror of `DrawMeshPayload::is_noop` at the user-mesh layer.
    #[inline]
    pub fn is_noop(&self) -> bool {
        self.vertices.is_empty() || self.indices.len() < 3 || !self.indices.len().is_multiple_of(3)
    }

    /// Stable visual hash of vertices + indices. Memoized — repeat calls
    /// on an unmutated mesh return the cached value. Mutating through any
    /// public method invalidates the cache.
    pub fn content_hash(&self) -> u64 {
        if let Some(h) = self.cached_hash.get() {
            return h;
        }
        let mut h = FxHasher::default();
        for vertex in &self.vertices {
            approx::hash_visual_vec2(vertex.pos, &mut h);
            h.write_u32(vertex.color.to_u32());
        }
        h.write(bytemuck::cast_slice(self.indices.as_slice()));
        let v = h.finish();
        self.cached_hash.set(Some(v));
        v
    }

    /// Push a vertex; returns its index for use in [`Self::triangle`].
    /// `color` accepts `Color` or `ColorU8`.
    ///
    /// # Panics
    ///
    /// Panics if the new vertex index cannot be represented by `u32`.
    #[inline]
    pub fn vertex(&mut self, pos: Vec2, color: impl Into<ColorU8>) -> u32 {
        let index = checked_vertex_index(self.vertices.len());
        self.vertices.push(MeshVertex::new(pos, color));
        self.cached_hash.set(None);
        self.cached_bbox.set(None);
        index
    }

    /// Push three indices (CCW by convention).
    ///
    /// # Panics
    ///
    /// Panics if any index does not refer to an existing vertex.
    #[inline]
    pub fn triangle(&mut self, a: u32, b: u32, c: u32) {
        let max_index = a.max(b).max(c) as usize;
        assert!(
            max_index < self.vertices.len(),
            "mesh triangle indices [{a}, {b}, {c}] exceed vertex count {}",
            self.vertices.len(),
        );
        self.indices.push(a);
        self.indices.push(b);
        self.indices.push(c);
        self.cached_hash.set(None);
    }

    /// Append another mesh, offsetting its indices into this mesh's
    /// vertex space.
    ///
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

    /// Convenience: filled triangle in a single color (`Color` or
    /// `ColorU8`). Bbox falls out of the three known vertices —
    /// pre-cached so the first `bbox()` call is free.
    pub fn filled_triangle(a: Vec2, b: Vec2, c: Vec2, color: impl Into<ColorU8>) -> Self {
        let color = color.into();
        let mut m = Self::with_capacity(3, 3);
        let i0 = m.vertex(a, color);
        let i1 = m.vertex(b, color);
        let i2 = m.vertex(c, color);
        m.triangle(i0, i1, i2);
        let lo = a.min(b).min(c);
        let hi = a.max(b).max(c);
        m.cached_bbox.set(Some(Rect::from_min_max(lo, hi)));
        m
    }

    /// Convenience: filled convex polygon (fan triangulation around the
    /// first vertex). For non-convex polygons the result is visually
    /// wrong — caller's responsibility. `color` accepts `Color` or
    /// `ColorU8`. Bbox tracked during the fan loop and pre-cached, so
    /// the first `bbox()` call is free.
    pub fn filled_polygon(points: &[Vec2], color: impl Into<ColorU8>) -> Self {
        if points.len() < 3 {
            return Self::new();
        }
        let color = color.into();
        let mut m = Self::with_capacity(points.len(), (points.len() - 2) * 3);
        let mut lo = points[0];
        let mut hi = points[0];
        let i0 = m.vertex(points[0], color);
        let mut prev = m.vertex(points[1], color);
        lo = lo.min(points[1]);
        hi = hi.max(points[1]);
        for &p in &points[2..] {
            let next = m.vertex(p, color);
            m.triangle(i0, prev, next);
            prev = next;
            lo = lo.min(p);
            hi = hi.max(p);
        }
        m.cached_bbox.set(Some(Rect::from_min_max(lo, hi)));
        m
    }
}

#[inline]
fn checked_vertex_index(index: usize) -> u32 {
    u32::try_from(index).expect("mesh vertex index exceeds u32 range")
}

#[inline]
fn checked_rebased_index(base: u32, index: u32) -> u32 {
    base.checked_add(index)
        .expect("appended mesh index exceeds u32 range")
}

// Sister inline loops in `forest/shapes/lower.rs` — `polyline` /
// `curve_inner` — fuse this AABB-of-points pattern with their copy
// pass — don't "DRY" them into a shared helper, the fusion is the win.
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
    Rect::from_min_max(lo, hi)
}

#[cfg(test)]
mod tests {
    use crate::primitives::color::Color;
    use crate::primitives::mesh::*;
    use crate::primitives::size::Size;

    fn mesh_with_vertices(count: usize) -> Mesh {
        let mut mesh = Mesh::with_capacity(count, 0);
        for index in 0..count {
            mesh.vertex(Vec2::new(index as f32, 0.0), Color::WHITE);
        }
        mesh
    }

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
    fn mesh_index_arithmetic_accepts_boundaries_and_rejects_overflow() {
        assert_eq!(checked_vertex_index(u32::MAX as usize), u32::MAX);
        if let Some(overflow) = (u32::MAX as usize).checked_add(1) {
            assert!(
                std::panic::catch_unwind(|| checked_vertex_index(overflow)).is_err(),
                "vertex indices above u32::MAX must panic",
            );
        }

        assert_eq!(checked_rebased_index(u32::MAX - 1, 1), u32::MAX);
        assert!(
            std::panic::catch_unwind(|| checked_rebased_index(u32::MAX, 1)).is_err(),
            "rebased indices above u32::MAX must panic",
        );
    }

    #[test]
    fn triangle_validates_each_index_before_mutating() {
        #[derive(Debug)]
        struct Case {
            label: &'static str,
            indices: [u32; 3],
        }

        for case in [
            Case {
                label: "first",
                indices: [3, 1, 2],
            },
            Case {
                label: "second",
                indices: [0, 3, 2],
            },
            Case {
                label: "third",
                indices: [0, 1, 3],
            },
        ] {
            let mut mesh = mesh_with_vertices(3);
            let [a, b, c] = case.indices;
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                mesh.triangle(a, b, c);
            }));
            assert!(result.is_err(), "{} index must be rejected", case.label);
            assert!(
                mesh.indices.is_empty(),
                "{} failure must not partially append indices",
                case.label,
            );
        }

        let mut mesh = mesh_with_vertices(3);
        mesh.triangle(2, 1, 0);
        assert_eq!(mesh.indices, [2, 1, 0]);
    }

    #[test]
    fn triangle_indices_offset_in_append() {
        let mut a = Mesh::filled_triangle(Vec2::ZERO, Vec2::X, Vec2::Y, Color::default());
        let b = Mesh::filled_triangle(Vec2::ZERO, Vec2::X, Vec2::Y, Color::default());
        a.append(&b);
        assert_eq!(a.vertices.len(), 6);
        assert_eq!(a.indices, vec![0, 1, 2, 3, 4, 5]);
        assert_eq!(a.bbox(), Rect::new(0.0, 0.0, 1.0, 1.0));

        let mut expected = Mesh::with_capacity(6, 6);
        for _ in 0..2 {
            let i0 = expected.vertex(Vec2::ZERO, Color::default());
            let i1 = expected.vertex(Vec2::X, Color::default());
            let i2 = expected.vertex(Vec2::Y, Color::default());
            expected.triangle(i0, i1, i2);
        }
        assert_eq!(a.vertices, expected.vertices);
        assert_eq!(a.content_hash(), expected.content_hash());
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

        let make = |first| Mesh::filled_triangle(first, Vec2::X, Vec2::Y, Color::WHITE);
        assert_eq!(
            make(Vec2::ZERO).content_hash(),
            make(Vec2::new(approx::EPS * 0.5, -approx::EPS * 0.5)).content_hash(),
        );
        assert_ne!(
            make(Vec2::ZERO).content_hash(),
            make(Vec2::new(approx::EPS * 2.0, 0.0)).content_hash(),
        );
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
    fn filled_triangle_precaches_bbox() {
        let m = Mesh::filled_triangle(
            Vec2::new(-1.0, 2.0),
            Vec2::new(4.0, 2.0),
            Vec2::new(0.0, 7.0),
            Color::default(),
        );
        // No `bbox()` call yet — must already be cached.
        let cached = m
            .cached_bbox
            .get()
            .expect("filled_triangle should pre-cache bbox");
        assert_eq!(cached.min, Vec2::new(-1.0, 2.0));
        assert_eq!(cached.size.w, 5.0);
        assert_eq!(cached.size.h, 5.0);
    }

    #[test]
    fn filled_polygon_precaches_bbox() {
        let pts = [
            Vec2::new(0.0, 0.0),
            Vec2::new(3.0, 0.0),
            Vec2::new(3.0, 2.0),
            Vec2::new(0.0, 2.0),
        ];
        let m = Mesh::filled_polygon(&pts, Color::default());
        let cached = m
            .cached_bbox
            .get()
            .expect("filled_polygon should pre-cache bbox");
        assert_eq!(cached.min, Vec2::ZERO);
        assert_eq!(cached.size.w, 3.0);
        assert_eq!(cached.size.h, 2.0);
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
