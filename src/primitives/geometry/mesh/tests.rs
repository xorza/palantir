use crate::internals::panic_probe;
use crate::primitives::geometry::mesh::*;
use crate::primitives::math::domain;

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
    assert_eq!(checked_vertex_idx(u32::MAX as usize), u32::MAX);
    if let Some(overflow) = (u32::MAX as usize).checked_add(1) {
        panic_probe::assert_panics_with("mesh vertex index exceeds u32 range", || {
            checked_vertex_idx(overflow)
        });
    }

    assert_eq!(checked_rebased_idx(u32::MAX - 1, 1), u32::MAX);
    panic_probe::assert_panics_with("appended mesh index exceeds u32 range", || {
        checked_rebased_idx(u32::MAX, 1)
    });
}

/// An index past the last vertex is refused before either push, leaving no partial run.
#[test]
fn triangle_validates_each_index_before_mutating() {
    fn mesh_with_vertices(count: usize) -> Mesh {
        let mut mesh = Mesh::with_capacity(count, 0);
        for idx in 0..count {
            mesh.vertex(Vec2::new(idx as f32, 0.0), RgbaF32::WHITE);
        }
        mesh
    }

    #[derive(Debug)]
    struct Case {
        label: &'static str,
        idxs: [u32; 3],
    }

    for case in [
        Case {
            label: "first",
            idxs: [3, 1, 2],
        },
        Case {
            label: "second",
            idxs: [0, 3, 2],
        },
        Case {
            label: "third",
            idxs: [0, 1, 3],
        },
    ] {
        let mut mesh = mesh_with_vertices(3);
        let [a, b, c] = case.idxs;
        panic_probe::assert_panics_with("exceed vertex count 3", || mesh.triangle(a, b, c));
        assert!(
            mesh.idxs.is_empty(),
            "{} failure must not partially append indices",
            case.label,
        );
    }

    let mut mesh = mesh_with_vertices(3);
    mesh.triangle(2, 1, 0);
    assert_eq!(mesh.idxs, [2, 1, 0]);
}

#[test]
fn triangle_indices_offset_in_append() {
    let mut a = Mesh::filled_polygon(&[Vec2::ZERO, Vec2::X, Vec2::Y], RgbaF32::default());
    let b = Mesh::filled_polygon(&[Vec2::ZERO, Vec2::X, Vec2::Y], RgbaF32::default());
    a.append(&b);
    assert_eq!(a.vertices.len(), 6);
    assert_eq!(a.idxs, vec![0, 1, 2, 3, 4, 5]);
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
    assert!(!a.is_noop());
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
    assert_eq!(m.idxs, vec![0, 1, 2, 0, 2, 3]);
}

fn red_tri() -> Mesh {
    let red = RgbaF32 {
        r: 1.0,
        g: 0.0,
        b: 0.0,
        a: 1.0,
    };
    Mesh::filled_polygon(&[Vec2::ZERO, Vec2::X, Vec2::Y], red)
}

#[test]
fn content_hash_stable_for_identical_input() {
    let a = red_tri();
    let b = red_tri();
    assert_eq!(a.content_hash(), b.content_hash());

    let make = |first| Mesh::filled_polygon(&[first, Vec2::X, Vec2::Y], RgbaF32::WHITE);
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
    let wound = |[a, b, c]: [u32; 3]| {
        let mut m = Mesh::new();
        for pos in [Vec2::ZERO, Vec2::X, Vec2::Y] {
            m.vertex(pos, RgbaF32::default());
        }
        m.triangle(a, b, c);
        m
    };
    assert_ne!(
        wound([0, 1, 2]).content_hash(),
        wound([0, 2, 1]).content_hash()
    );
}

/// A filled polygon's bbox spans its points: a triangle reaching left of the origin, then a
/// rectangle at it.
#[test]
fn filled_polygon_bbox_spans_its_points() {
    let cases: [(&[Vec2], Rect); 2] = [
        (
            &[
                Vec2::new(-1.0, 2.0),
                Vec2::new(4.0, 2.0),
                Vec2::new(0.0, 7.0),
            ],
            Rect::new(-1.0, 2.0, 5.0, 5.0),
        ),
        (
            &[
                Vec2::new(0.0, 0.0),
                Vec2::new(3.0, 0.0),
                Vec2::new(3.0, 2.0),
                Vec2::new(0.0, 2.0),
            ],
            Rect::new(0.0, 0.0, 3.0, 2.0),
        ),
    ];
    for (corners, bbox) in cases {
        let m = Mesh::filled_polygon(corners, RgbaF32::default());
        assert_eq!(m.bbox(), bbox, "{corners:?}");
    }
}

#[test]
fn bbox_empty_mesh_is_zero() {
    assert_eq!(Mesh::new().bbox(), Rect::ZERO);
}

/// `points` in order, then `triangles`.
fn built(points: &[(Vec2, SrgbaU8)], triangles: &[[u32; 3]]) -> Mesh {
    let mut m = Mesh::new();
    for &(pos, color) in points {
        m.vertex(pos, color);
    }
    for &[a, b, c] in triangles {
        m.triangle(a, b, c);
    }
    m
}

/// Each mutation leaves the hash and the bbox of the same content built at once, vertex by vertex
/// and triangle by triangle: `red_tri` is the unit triangle at the origin, and `far` one at
/// (10, 10). Pushing (2, 3) grows the bbox to 2 × 3; a triangle moves no vertex; appending `far`
/// reaches (11, 11); appending an empty mesh changes nothing; clearing leaves the empty mesh. A
/// clone keeps both.
#[test]
fn mutations_match_the_content_built_at_once() {
    type Mutate = fn(&mut Mesh, &Mesh);
    let far_points = [
        Vec2::new(10.0, 10.0),
        Vec2::new(11.0, 10.0),
        Vec2::new(10.0, 11.0),
    ];
    let far = Mesh::filled_polygon(&far_points, RgbaF32::default());
    let red = red_tri().vertices[0].color;
    let unit = [Vec2::ZERO, Vec2::X, Vec2::Y];
    let reds = unit.map(|pos| (pos, red));
    let black = RgbaF32::default().to_srgba8();
    let cases: [(&str, Mutate, Mesh, Rect); 5] = [
        (
            "vertex",
            |m, _| {
                m.vertex(Vec2::new(2.0, 3.0), RgbaF32::default());
            },
            built(
                &[reds[0], reds[1], reds[2], (Vec2::new(2.0, 3.0), black)],
                &[[0, 1, 2]],
            ),
            Rect::new(0.0, 0.0, 2.0, 3.0),
        ),
        (
            "triangle",
            |m, _| m.triangle(0, 1, 2),
            built(&reds, &[[0, 1, 2], [0, 1, 2]]),
            Rect::new(0.0, 0.0, 1.0, 1.0),
        ),
        (
            "append",
            |m, o| m.append(o),
            built(
                &[
                    reds[0],
                    reds[1],
                    reds[2],
                    (far_points[0], black),
                    (far_points[1], black),
                    (far_points[2], black),
                ],
                &[[0, 1, 2], [3, 4, 5]],
            ),
            Rect::new(0.0, 0.0, 11.0, 11.0),
        ),
        (
            "append empty",
            |m, _| m.append(&Mesh::new()),
            built(&reds, &[[0, 1, 2]]),
            Rect::new(0.0, 0.0, 1.0, 1.0),
        ),
        ("clear", |m, _| m.clear(), Mesh::new(), Rect::ZERO),
    ];
    let mut cleared = red_tri();
    cleared.clear();
    cleared.vertex(Vec2::new(5.0, 6.0), RgbaF32::default());
    assert_eq!(
        cleared.bbox(),
        Rect::new(5.0, 6.0, 0.0, 0.0),
        "a clear forgets the old bounds"
    );
    assert_eq!(
        cleared.content_hash(),
        built(&[(Vec2::new(5.0, 6.0), black)], &[]).content_hash(),
        "a clear forgets the old hash"
    );
    for (label, mutate, expected, bbox) in cases {
        let mut m = red_tri();
        let copy = m.clone();
        mutate(&mut m, &far);
        assert_eq!(m.bbox(), bbox, "{label}: bbox");
        assert_eq!(expected.bbox(), bbox, "{label}: bbox built at once");
        assert_eq!(m.content_hash(), expected.content_hash(), "{label}: hash");
        assert_eq!(
            (copy.content_hash(), copy.bbox()),
            (red_tri().content_hash(), Rect::new(0.0, 0.0, 1.0, 1.0)),
            "{label}: the clone keeps the content before",
        );
    }
    assert_ne!(
        red_tri().content_hash(),
        Mesh::new().content_hash(),
        "content changes the hash"
    );
}

/// `vertex` refuses a non-finite position, the one door a vertex enters by, so a mesh never holds
/// one and `is_noop` needs no NaN scan.
#[test]
fn vertex_refuses_a_non_finite_position() {
    for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        for pos in [Vec2::new(bad, 0.0), Vec2::new(0.0, bad)] {
            panic_probe::assert_panics_with(domain::OFFSET_RULE, || {
                Mesh::new().vertex(pos, RgbaF32::WHITE)
            });
        }
    }
}
