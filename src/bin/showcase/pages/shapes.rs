//! Filled shape primitives: `Shape::Triangle` (SDF coverage AA, corner
//! rounding via `SDF - radius`, inner-edge strokes), `Shape::Mesh` (raw
//! per-vertex geometry, including a 5k-vertex stress grid exercising the
//! alloc-free claim and the index-buffer growth path), and
//! `Shape::windowed_rect` — the inverted-fill corner mask that stands in
//! for rounded-corner clipping without a stencil pass.

use crate::support;
use crate::support::{demo_cell, section, tiles};
use glam::Vec2;
use palantir::{Color, ColorU8, LinearGradient, Mesh, Shape, Stroke, Ui, WidgetId};
use std::f32::consts::{FRAC_PI_2, PI};

pub(crate) fn build(ui: &mut Ui) {
    section(
        ui,
        "triangles — one instanced quad each; coverage, rounding, and stroke all \
         come from the SDF",
        |ui| {
            tiles(ui, |ui| {
                demo_cell(ui, "sharp fill", sharp);
                demo_cell(ui, "rounded 12 px", rounded);
                demo_cell(ui, "fill + inner stroke", stroked);
                demo_cell(ui, "outline only", outline);
                demo_cell(ui, "play glyph — radii 0 / 4 / 10", radii);
            });
        },
    );

    section(
        ui,
        "meshes — raw vertices and indices, uploaded straight to the mesh pipeline",
        |ui| {
            tiles(ui, |ui| {
                demo_cell(ui, "single triangle", mesh_triangle);
                demo_cell(ui, "star — centroid fan", polygon_star);
                demo_cell(ui, "per-vertex gradient", gradient_quad);
                demo_cell(ui, "5 000-vertex stress grid", stress);
            });
        },
    );

    section(
        ui,
        "windowed rect — an inverted rounded-rect fill: paints the corner wedges, \
         leaves the window alone",
        |ui| {
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

/// Sharp-cornered solid fill — the aliased case a `Mesh::filled_triangle`
/// would give, now with crisp SDF coverage AA.
fn sharp(ui: &mut Ui) {
    ui.add_shape(Shape::triangle(A, B, C).fill(support::A));
}

/// Rounded corners — `SDF - radius`, no extra geometry.
fn rounded(ui: &mut Ui) {
    ui.add_shape(Shape::triangle(A, B, C).fill(support::C).radius(12.0_f32));
}

/// Fill + inner-edge stroke, rounded.
fn stroked(ui: &mut Ui) {
    ui.add_shape(
        Shape::triangle(A, B, C)
            .fill(support::D)
            .stroke(Stroke::solid(Color::WHITE, 3.0))
            .radius(10.0_f32),
    );
}

/// Stroke only (transparent fill) — a rounded triangular outline.
fn outline(ui: &mut Ui) {
    ui.add_shape(
        Shape::triangle(A, B, C)
            .stroke(Stroke::solid(support::B, 3.0))
            .radius(8.0_f32),
    );
}

/// A play triangle (▶) at three corner radii — the toolbar-glyph use
/// case, from sharp to increasingly soft.
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

fn mesh_triangle(ui: &mut Ui) {
    let mut m = Mesh::new();
    let a = m.vertex(B, support::E);
    let b = m.vertex(C, support::E);
    let c = m.vertex(A, support::E);
    m.triangle(a, b, c);
    ui.add_shape(Shape::mesh(&m));
}

/// 5-pointed star sampled as a fan around the centroid. The star is
/// concave, so fanning around the first point would clip — fanning
/// around the centroid is correct here.
fn polygon_star(ui: &mut Ui) {
    let (cx, cy) = (84.0_f32, 84.0_f32);
    let (r_outer, r_inner) = (72.0_f32, 29.0_f32);
    let mut pts = Vec::with_capacity(10);
    for i in 0..10 {
        let theta = -FRAC_PI_2 + i as f32 * PI / 5.0;
        let r = if i % 2 == 0 { r_outer } else { r_inner };
        pts.push(Vec2::new(cx + r * theta.cos(), cy + r * theta.sin()));
    }
    let mut m = Mesh::new();
    let centroid = m.vertex(Vec2::new(cx, cy), support::B);
    let first = m.vertex(pts[0], support::B);
    let mut prev = first;
    for p in &pts[1..] {
        let next = m.vertex(*p, support::B);
        m.triangle(centroid, prev, next);
        prev = next;
    }
    m.triangle(centroid, prev, first);
    ui.add_shape(Shape::mesh(&m));
}

/// Per-vertex colours create a four-corner gradient across two triangles.
fn gradient_quad(ui: &mut Ui) {
    let mut m = Mesh::new();
    let tl = m.vertex(Vec2::new(16.0, 16.0), support::E);
    let tr = m.vertex(Vec2::new(152.0, 16.0), support::C);
    let br = m.vertex(Vec2::new(152.0, 152.0), support::A);
    let bl = m.vertex(Vec2::new(16.0, 152.0), support::B);
    m.triangle(tl, tr, br);
    m.triangle(tl, br, bl);
    ui.add_shape(Shape::mesh(&m));
}

/// 2 500 verts / ~5 000 after triangle pairing. Exercises the alloc-free
/// claim and the index-buffer growth path; renders as a teal wash since
/// every vertex shares one colour.
///
/// The grid is geometry, not state: built once into a retained row and
/// redrawn from there. A fresh `Mesh` per frame would allocate ~90 KB and
/// re-hash all 2 500 vertices every frame, because `Mesh` memoizes its
/// content hash and a new one is always cold — which is the opposite of the
/// retention this page is here to demonstrate.
fn stress(ui: &mut Ui) {
    const SIDE: u32 = 50;
    const STEP: f32 = 3.0;
    let mesh_id = WidgetId::from_hash("showcase::shapes::stress-grid");
    // Lent for the draw rather than copied: `add_shape` wants `&mut Ui`, and
    // the grid stays put in the state row.
    let fresh = ui.try_state::<Mesh>(mesh_id).is_none();
    ui.with_state::<Mesh, _>(mesh_id, |ui, m| {
        if fresh {
            let teal = Color::hex(0x2fa8a8);
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
        }
        ui.add_shape(Shape::mesh(m));
    });
}

/// The headline use case: rounded-corner clipping without a stencil
/// pass. The gradient "content" is a plain unclipped rect; the windowed
/// rect on top fills the corner wedges with the tile background and
/// strokes the boundary — visually a rounded-clipped card.
fn window_mask(ui: &mut Ui) {
    ui.add_shape(
        Shape::owner_rect().fill(
            LinearGradient::builder(FRAC_PI_2)
                .stop(0.0, ColorU8::hex(0x1a1a2e))
                .stop(1.0, ColorU8::hex(0x4c5cdb)),
        ),
    );
    ui.add_shape(
        Shape::owner_windowed_rect()
            .corners(18.0)
            .fill(support::WELL)
            .stroke(Stroke::solid(support::A, 2.0)),
    );
}

/// Translucent fill exposes the geometry: the fill covers only the
/// corner wedges outside the rounded boundary, the stroke hugs the
/// boundary's inner edge, and the window interior stays untouched.
fn window_anatomy(ui: &mut Ui) {
    ui.add_shape(
        Shape::owner_windowed_rect()
            .corners(28.0)
            .fill(support::B.with_alpha(0.75))
            .stroke(Stroke::solid(support::C, 4.0)),
    );
}
