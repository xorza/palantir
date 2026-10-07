use crate::internals::panic_probe;
use crate::primitives::geometry::mesh::*;
use crate::primitives::math::domain;
use crate::primitives::paint::color::RgbaF32;

#[test]
fn mesh_vertex_pod_roundtrip() {
    let v = MeshVertex::new(
        Vec2::new(1.0, 2.0),
        RgbaF32 {
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
        panic_probe::assert_panics_with("mesh vertex index exceeds u32 range", || {
            checked_vertex_index(overflow)
        });
    }

    assert_eq!(checked_rebased_index(u32::MAX - 1, 1), u32::MAX);
    panic_probe::assert_panics_with("appended mesh index exceeds u32 range", || {
        checked_rebased_index(u32::MAX, 1)
    });
}

/// An index past the last vertex is refused before either push, leaving no partial run. Debug-only: it runs per item of a build loop.
#[cfg(debug_assertions)]
#[test]
fn triangle_validates_each_index_before_mutating() {
    fn mesh_with_vertices(count: usize) -> Mesh {
        let mut mesh = Mesh::with_capacity(count, 0);
        for index in 0..count {
            mesh.vertex(Vec2::new(index as f32, 0.0), RgbaF32::WHITE);
        }
        mesh
    }

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
        panic_probe::assert_panics_with("exceed vertex count 3", || mesh.triangle(a, b, c));
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
    let mut a = Mesh::filled_triangle(Vec2::ZERO, Vec2::X, Vec2::Y, RgbaF32::default());
    let b = Mesh::filled_triangle(Vec2::ZERO, Vec2::X, Vec2::Y, RgbaF32::default());
    a.append(&b);
    assert_eq!(a.vertices.len(), 6);
    assert_eq!(a.indices, vec![0, 1, 2, 3, 4, 5]);
    assert_eq!(a.bbox(), Rect::new(0.0, 0.0, 1.0, 1.0));

    let mut expected = Mesh::with_capacity(6, 6);
    for _ in 0..2 {
        let i0 = expected.vertex(Vec2::ZERO, RgbaF32::default());
        let i1 = expected.vertex(Vec2::X, RgbaF32::default());
        let i2 = expected.vertex(Vec2::Y, RgbaF32::default());
        expected.triangle(i0, i1, i2);
    }
    assert_eq!(a.vertices, expected.vertices);
    assert_eq!(a.content_hash(), expected.content_hash());
    assert_eq!(a.max_index, 5);
    assert!(!a.is_noop());
}

/// An out-of-range index trips `triangle`'s debug assert; release pushes it, so the state is built by hand and the screen must drop the mesh. The control row has the last index at 2.
#[test]
fn an_index_past_the_last_vertex_is_a_noop() {
    for (last, noop) in [(2, false), (3, true)] {
        let mut mesh = Mesh::with_capacity(3, 3);
        for x in 0..3 {
            mesh.vertex(Vec2::new(x as f32, x as f32 * 2.0), RgbaF32::WHITE);
        }
        mesh.indices.extend([0, 1, last]);
        mesh.max_index = last;
        assert_eq!(mesh.is_noop(), noop, "last index {last} of 3 vertices");
    }
}

#[test]
fn polygon_fan_indices_share_pivot() {
    let pts = [
        Vec2::new(0.0, 0.0),
        Vec2::new(1.0, 0.0),
        Vec2::new(1.0, 1.0),
        Vec2::new(0.0, 1.0),
    ];
    let m = Mesh::filled_polygon(&pts, RgbaF32::default());
    assert_eq!(m.vertices.len(), 4);
    assert_eq!(m.indices, vec![0, 1, 2, 0, 2, 3]);
}

fn red_tri() -> Mesh {
    let red = RgbaF32 {
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

    let make = |first| Mesh::filled_triangle(first, Vec2::X, Vec2::Y, RgbaF32::WHITE);
    assert_eq!(
        make(Vec2::ZERO).content_hash(),
        make(Vec2::new(domain::EPS * 0.5, -domain::EPS * 0.5)).content_hash(),
    );
    assert_ne!(
        make(Vec2::ZERO).content_hash(),
        make(Vec2::new(domain::EPS * 2.0, 0.0)).content_hash(),
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
fn filled_triangle_precaches_bbox() {
    let m = Mesh::filled_triangle(
        Vec2::new(-1.0, 2.0),
        Vec2::new(4.0, 2.0),
        Vec2::new(0.0, 7.0),
        RgbaF32::default(),
    );
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
    let m = Mesh::filled_polygon(&pts, RgbaF32::default());
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
        RgbaF32::default(),
    );
    let b = m.bbox();
    assert_eq!(b.min, Vec2::new(-1.0, 2.0));
    assert_eq!(b.size.w, 5.0);
    assert_eq!(b.size.h, 5.0);
}

/// Which caches each mutation drops, from a mesh with both primed: vertex, append and clear drop both; a triangle drops only the hash; appending an empty mesh and cloning keep both.
#[test]
fn mutations_drop_exactly_the_caches_they_stale() {
    type Mutate = fn(&mut Mesh, &Mesh);
    let far = Mesh::filled_triangle(
        Vec2::new(10.0, 10.0),
        Vec2::new(11.0, 10.0),
        Vec2::new(10.0, 11.0),
        RgbaF32::default(),
    );
    let unit = Rect::new(0.0, 0.0, 1.0, 1.0);
    let cases: [(&str, Mutate, bool, bool, Rect); 5] = [
        (
            "vertex",
            |m, _| {
                m.vertex(Vec2::new(2.0, 3.0), RgbaF32::default());
            },
            false,
            false,
            Rect::new(0.0, 0.0, 2.0, 3.0),
        ),
        ("triangle", |m, _| m.triangle(0, 1, 2), false, true, unit),
        (
            "append",
            |m, o| m.append(o),
            false,
            false,
            Rect::new(0.0, 0.0, 11.0, 11.0),
        ),
        (
            "append empty",
            |m, _| m.append(&Mesh::new()),
            true,
            true,
            unit,
        ),
        ("clear", |m, _| m.clear(), false, false, Rect::ZERO),
    ];
    for (label, mutate, hash_kept, bbox_kept, bbox) in cases {
        let mut m = red_tri();
        let (h0, b0) = (m.content_hash(), m.bbox());
        assert_eq!(b0, unit);
        let copy = m.clone();
        assert_eq!(
            (copy.cached_hash.get(), copy.cached_bbox.get()),
            (Some(h0), Some(b0)),
            "a clone carries both caches",
        );

        mutate(&mut m, &far);
        assert_eq!(
            m.cached_hash.get(),
            hash_kept.then_some(h0),
            "{label}: hash cache"
        );
        assert_eq!(
            m.cached_bbox.get(),
            bbox_kept.then_some(b0),
            "{label}: bbox cache"
        );
        assert_eq!(m.bbox(), bbox, "{label}: bbox");
        assert_eq!(m.content_hash() == h0, hash_kept, "{label}: hash value");
    }
}
