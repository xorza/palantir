//! App-supplied triangle geometry: the vertex the GPU takes, the indexed
//! mesh an app builds, and the index and bounds screens that keep a
//! malformed one from reaching the renderer.

use crate::common::hash::Hasher;
use crate::primitives::approx::FloatHash;
use crate::primitives::color::RgbaU8;
use crate::primitives::rect::Rect;
use crate::primitives::rect::aabb::Aabb;
use bytemuck::{Pod, Zeroable};
use glam::Vec2;
use std::cell::Cell;
use std::hash::Hasher as _;

/// One vertex of a user-supplied mesh. 12 B (pos 8 + color 4), no
/// padding — directly castable into a wgpu vertex buffer.
///
/// `pos` is in **owner-local logical px** (origin = the shape's
/// owner-rect top-left, after `local_rect.min` offset if set). The
/// composer bakes the accumulated transform + DPI scale into a
/// physical-px copy at compose time.
///
/// `color` is **straight-alpha linear RGBA** — the mesh shader
/// premultiplies at output — stored as `RgbaU8` (8 bits per channel,
/// linear-space — the default `From<RgbaF32> for RgbaU8` is a linear
/// quantize, no sRGB encoding). The GPU vertex
/// attribute is `Unorm8x4`, so `u8/255` lands in the rasterizer as
/// `0..1` linear floats with no shader decode. Banding in dark
/// gradients across a mesh face is the trade-off for the 12 B vertex
/// footprint vs. 24 B.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct MeshVertex {
    pub pos: Vec2,
    pub color: RgbaU8,
}

impl MeshVertex {
    /// Construct at `pos` with any `Into<RgbaU8>` colour — accepts a
    /// linear `RgbaF32` (quantized at the boundary) or a `RgbaU8`
    /// (passthrough), so call sites that already hold quantized colour
    /// don't round-trip through f32.
    pub fn new(pos: Vec2, color: impl Into<RgbaU8>) -> Self {
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
        self.vertices.is_empty()
            || self.indices.len() < 3
            || !self.indices.len().is_multiple_of(3)
            // A NaN vertex reaches the AABB by the fold's NaN contract,
            // so this `O(1)` read stands in for scanning every position.
            // `bbox` is memoized, so repeat calls are a load.
            || self.bbox().has_nan()
    }

    /// Stable visual hash of vertices + indices. Memoized — repeat calls
    /// on an unmutated mesh return the cached value. Mutating through any
    /// public method invalidates the cache.
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

    /// Push a vertex; returns its index for use in [`Self::triangle`].
    /// `color` accepts `RgbaF32` or `RgbaU8`.
    ///
    /// # Panics
    ///
    /// Panics if the new vertex index cannot be represented by `u32`.
    #[inline]
    pub fn vertex(&mut self, pos: Vec2, color: impl Into<RgbaU8>) -> u32 {
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
    /// Panics in a debug build if any index does not refer to an
    /// existing vertex. Debug-only because this is per item of a
    /// caller's build loop — a mesh of ten thousand triangles pays it
    /// ten thousand times, and a mesh is rebuilt per frame.
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
        self.cached_hash.set(None);
    }

    /// Append another mesh, offsetting its indices into this mesh's
    /// vertex space.
    ///
    /// Published surface with no in-crate caller on purpose: `vertices`
    /// and `indices` are private, so rebasing one mesh onto another is
    /// not something a consumer can write from outside. Every other
    /// builder method has a spelling a caller could reach for instead;
    /// this one is the whole of mesh composition.
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

    /// Convenience: filled triangle in a single color (`RgbaF32` or
    /// `RgbaU8`). Bbox falls out of the three known vertices —
    /// pre-cached so the first `bbox()` call is free.
    pub fn filled_triangle(a: Vec2, b: Vec2, c: Vec2, color: impl Into<RgbaU8>) -> Self {
        let color = color.into();
        let mut m = Self::with_capacity(3, 3);
        let i0 = m.vertex(a, color);
        let i1 = m.vertex(b, color);
        let i2 = m.vertex(c, color);
        m.triangle(i0, i1, i2);
        // Through `Aabb`, not a bare `min`/`max` fold: those are IEEE
        // `minNum`/`maxNum` and drop a NaN operand, which would hand
        // `is_noop` a finite box for a NaN vertex and pass it to the GPU.
        m.cached_bbox.set(Some(Aabb::of_iter([a, b, c])));
        m
    }

    /// Convenience: filled convex polygon (fan triangulation around the
    /// first vertex). For non-convex polygons the result is visually
    /// wrong — caller's responsibility. `color` accepts `RgbaF32` or
    /// `RgbaU8`. Bbox is pre-cached, so the first `bbox()` call is
    /// free.
    pub fn filled_polygon(points: &[Vec2], color: impl Into<RgbaU8>) -> Self {
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
        // Separate pass through `Aabb` rather than folded into the fan
        // above: a bare `min`/`max` fold is IEEE `minNum`/`maxNum` and
        // drops a NaN operand, which would hand `is_noop` a finite box
        // for a NaN vertex and pass it to the GPU.
        m.cached_bbox.set(Some(Aabb::of(points)));
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

// Deliberately *not* fused into the copy loops in
// `scene/shapes/lower.rs`. Fusing the AABB pass into the copy pass
// reads like the win and measures as the opposite: splitting them is
// ~3x faster past a handful of points, because each half then gets to be
// the fast version of itself — the fold vectorizes when nothing else
// shares the loop body, and the copy becomes one `memcpy` instead of
// per-point `push`es. Hence the shared `Aabb`.
fn compute_aabb(verts: &[MeshVertex]) -> Rect {
    Aabb::of_iter(verts.iter().map(|v| v.pos))
}

#[cfg(test)]
mod tests;
