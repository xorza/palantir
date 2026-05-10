use crate::primitives::color::Color;
use bytemuck::{Pod, Zeroable};
use glam::Vec2;

/// One vertex of a user-supplied mesh. 24 B (pos 8 + color 16), no
/// padding — directly castable into a wgpu vertex buffer.
///
/// `pos` is in **owner-local logical px** (origin = the shape's
/// owner-rect top-left, after `local_rect.min` offset if set). The
/// composer bakes the accumulated transform + DPI scale into a
/// physical-px copy at compose time.
///
/// `color` is **linear RGBA, premultiplied** — matches `Quad.fill`
/// and the wgpu blend state. No sRGB surprises.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct MeshVertex {
    pub pos: Vec2,
    pub color: Color,
}

impl MeshVertex {
    pub const fn new(pos: Vec2, color: Color) -> Self {
        Self { pos, color }
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
#[derive(Default, Clone, Debug, PartialEq)]
pub struct Mesh {
    pub vertices: Vec<MeshVertex>,
    pub indices: Vec<u16>,
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
        }
    }

    #[inline]
    pub fn clear(&mut self) {
        self.vertices.clear();
        self.indices.clear();
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.vertices.is_empty() || self.indices.is_empty()
    }

    /// Push a vertex; returns its `u16` index for use in [`Self::triangle`].
    /// Panics if the mesh already holds 65 535 vertices.
    #[inline]
    pub fn vertex(&mut self, pos: Vec2, color: Color) -> u16 {
        let idx = self.vertices.len();
        assert!(idx < u16::MAX as usize, "Mesh exceeds u16 vertex limit");
        self.vertices.push(MeshVertex { pos, color });
        idx as u16
    }

    /// Push three indices (CCW by convention).
    #[inline]
    pub fn triangle(&mut self, a: u16, b: u16, c: u16) {
        self.indices.push(a);
        self.indices.push(b);
        self.indices.push(c);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mesh_vertex_is_24_bytes_no_padding() {
        assert_eq!(std::mem::size_of::<MeshVertex>(), 24);
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
}
