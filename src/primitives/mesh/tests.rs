use crate::primitives::approx;
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
