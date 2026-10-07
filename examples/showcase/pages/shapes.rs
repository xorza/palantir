//! Filled shape primitives: triangles, meshes (including a 2 500-vertex stress grid) and
//! `Shape::owner_windowed_rect`, the inverted-fill corner mask standing in for rounded clipping without a
//! stencil pass.

use crate::support;
use crate::support::{api, demo_cell, note, section, tiles};
use palantir::widget::{Mesh, Shape};
use palantir::{LinearGradient, RgbaF32, Stroke, Ui, Vec2, WidgetId};
use std::f32::consts::{FRAC_PI_2, PI};

pub(crate) fn build(ui: &mut Ui) {
    section(ui, "Triangles", &[api!(Shape::triangle)], |ui| {
        note(
            ui,
            "One instanced quad each: coverage, rounded corners and the stroke \
                 all come from the signed distance field.",
        );
        tiles(ui, |ui| {
            demo_cell(ui, "sharp fill", sharp);
            demo_cell(ui, "rounded 12 px", rounded);
            demo_cell(ui, "fill + inner stroke", stroked);
            demo_cell(ui, "outline only", outline);
            demo_cell(ui, "play glyph — radii 0 / 4 / 10", radii);
        });
    });

    section(ui, "Meshes", &[api!(type Mesh), api!(Shape::mesh)], |ui| {
        note(
            ui,
            "Raw vertices and indices, sent straight to the mesh pipeline. Each \
                 mesh is built once and kept in a state row.",
        );
        tiles(ui, |ui| {
            demo_cell(ui, "single triangle", mesh_triangle);
            demo_cell(ui, "star — centroid fan", polygon_star);
            demo_cell(ui, "per-vertex gradient", gradient_quad);
            demo_cell(ui, "5 000-vertex stress grid", stress);
        });
    });

    section(
        ui,
        "Windowed rect",
        &[api!(Shape::owner_windowed_rect)],
        |ui| {
            note(
                ui,
                "An inverted rounded-rect fill: it paints the corner wedges and \
                 leaves the window alone, which gives rounded clipping with no \
                 stencil pass.",
            );
            tiles(ui, |ui| {
                demo_cell(ui, "corner mask over content", window_mask);
                demo_cell(ui, "anatomy — translucent fill", window_anatomy);
            });
        },
    );
}

const A: Vec2 = Vec2::new(20.0, 142.0);
const B: Vec2 = Vec2::new(84.0, 24.0);
const C: Vec2 = Vec2::new(148.0, 142.0);

fn sharp(ui: &mut Ui) {
    ui.add_shape(Shape::triangle(A, B, C).fill(support::A));
}

fn rounded(ui: &mut Ui) {
    ui.add_shape(Shape::triangle(A, B, C).fill(support::C).radius(12.0_f32));
}

fn stroked(ui: &mut Ui) {
    ui.add_shape(
        Shape::triangle(A, B, C)
            .fill(support::D)
            .border(Stroke::new(RgbaF32::WHITE, 3.0))
            .radius(10.0_f32),
    );
}

fn outline(ui: &mut Ui) {
    ui.add_shape(
        Shape::triangle(A, B, C)
            .border(Stroke::new(support::B, 3.0))
            .radius(8.0_f32),
    );
}

fn radii(ui: &mut Ui) {
    for (i, r) in [0.0_f32, 4.0, 10.0].iter().enumerate() {
        let dy = i as f32 * 46.0;
        ui.add_shape(
            Shape::triangle(
                Vec2::new(62.0, 16.0 + dy),
                Vec2::new(62.0, 54.0 + dy),
                Vec2::new(98.0, 35.0 + dy),
            )
            .fill(support::B)
            .radius(*r),
        );
    }
}

/// Builds a cell's geometry into a retained row once: a fresh `Mesh` per frame allocates and re-hashes (it
/// memoizes its content hash), worst for the stress grid but true at three vertices too.
fn retained_mesh(ui: &mut Ui, key: &'static str, build: impl FnOnce(&mut Mesh)) {
    let id = WidgetId::from_hash(("showcase::shapes::mesh", key));
    ui.with_state::<Option<Mesh>, _>(id, |ui, m| {
        let m = m.get_or_insert_with(|| {
            let mut m = Mesh::new();
            build(&mut m);
            m
        });
        ui.add_shape(Shape::mesh(m));
    });
}

fn mesh_triangle(ui: &mut Ui) {
    retained_mesh(ui, "triangle", |m| {
        let a = m.vertex(B, support::E);
        let b = m.vertex(C, support::E);
        let c = m.vertex(A, support::E);
        m.triangle(a, b, c);
    });
}

/// 5-pointed star fanned around the centroid, since it is concave.
fn polygon_star(ui: &mut Ui) {
    retained_mesh(ui, "star", |m| {
        let (cx, cy) = (84.0_f32, 84.0_f32);
        let (r_outer, r_inner) = (72.0_f32, 29.0_f32);
        let point = |i: usize| {
            let theta = -FRAC_PI_2 + i as f32 * PI / 5.0;
            let r = if i.is_multiple_of(2) {
                r_outer
            } else {
                r_inner
            };
            Vec2::new(cx + r * theta.cos(), cy + r * theta.sin())
        };
        let centroid = m.vertex(Vec2::new(cx, cy), support::B);
        let first = m.vertex(point(0), support::B);
        let mut prev = first;
        for i in 1..10 {
            let next = m.vertex(point(i), support::B);
            m.triangle(centroid, prev, next);
            prev = next;
        }
        m.triangle(centroid, prev, first);
    });
}

fn gradient_quad(ui: &mut Ui) {
    retained_mesh(ui, "gradient-quad", |m| {
        let tl = m.vertex(Vec2::new(16.0, 16.0), support::E);
        let tr = m.vertex(Vec2::new(152.0, 16.0), support::C);
        let br = m.vertex(Vec2::new(152.0, 152.0), support::A);
        let bl = m.vertex(Vec2::new(16.0, 152.0), support::B);
        m.triangle(tl, tr, br);
        m.triangle(tl, br, bl);
    });
}

/// 2 500 vertices, ~5 000 triangles: exercises the alloc-free claim and index-buffer growth.
fn stress(ui: &mut Ui) {
    const SIDE: u32 = 50;
    const STEP: f32 = 3.0;
    retained_mesh(ui, "stress-grid", |m| {
        let teal = RgbaF32::hex(0x2fa8a8);
        *m = Mesh::with_capacity((SIDE as usize).pow(2), (SIDE as usize - 1).pow(2) * 6);
        for j in 0..SIDE {
            for i in 0..SIDE {
                m.vertex(
                    Vec2::new(10.0 + i as f32 * STEP, 10.0 + j as f32 * STEP),
                    teal,
                );
            }
        }
        for j in 0..SIDE - 1 {
            for i in 0..SIDE - 1 {
                let a = j * SIDE + i;
                let b = a + 1;
                let c = a + SIDE;
                let d = c + 1;
                m.triangle(a, b, d);
                m.triangle(a, d, c);
            }
        }
    });
}

/// The headline use: rounded-corner clipping without a stencil pass; the windowed rect fills the corner wedges
/// with the tile background and strokes the boundary over a plain unclipped gradient.
fn window_mask(ui: &mut Ui) {
    ui.add_shape(
        Shape::owner_rect().fill(
            LinearGradient::builder(FRAC_PI_2)
                .stop(0.0, RgbaF32::hex(0x1a1a2e))
                .stop(1.0, RgbaF32::hex(0x4c5cdb)),
        ),
    );
    ui.add_shape(
        Shape::owner_windowed_rect()
            .corners(18.0)
            .fill(support::WELL)
            .border(Stroke::new(support::A, 2.0)),
    );
}

/// A translucent fill exposes the geometry: only the corner wedges outside the rounded boundary are covered.
fn window_anatomy(ui: &mut Ui) {
    ui.add_shape(
        Shape::owner_windowed_rect()
            .corners(28.0)
            .fill(support::B.with_alpha(0.75))
            .border(Stroke::new(support::C, 4.0)),
    );
}
