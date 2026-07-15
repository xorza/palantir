//! Filled shape primitives: `Shape::Triangle` (SDF coverage AA, corner
//! rounding via `SDF - radius`, inner-edge strokes), `Shape::Mesh`
//! (raw per-vertex geometry incl. a 5k-vertex stress grid exercising
//! the alloc-free claim and the index-buffer growth path), and
//! `Shape::WindowedRect` (inverted-fill corner mask — the cheap
//! stand-in for rounded-corner clipping).

use crate::support;
use crate::support::{cell_row, demo_cell};
use aperture::{Brush, Color, ColorU8, Corners, LinearGradient, Mesh, Shape, Stroke, Ui};
use glam::Vec2;
use std::f32::consts::{FRAC_PI_2, PI};

pub(crate) fn build(ui: &mut Ui) {
    support::page(ui, |ui| {
        cell_row(ui, "row1", |ui| {
            demo_cell(ui, "triangle — sharp fill", sharp);
            demo_cell(ui, "triangle — rounded 12 px", rounded);
            demo_cell(ui, "triangle — fill + stroke", stroked);
            demo_cell(ui, "triangle — outline only", outline);
            demo_cell(ui, "play-glyph radii 0 / 4 / 10", radii);
        });
        cell_row(ui, "row2", |ui| {
            demo_cell(ui, "mesh — single triangle", mesh_triangle);
            demo_cell(ui, "mesh — star (centroid fan)", polygon_star);
            demo_cell(ui, "mesh — per-vertex gradient", gradient_quad);
            demo_cell(ui, "mesh — 5k-vertex stress", stress);
        });
        cell_row(ui, "row3", |ui| {
            demo_cell(ui, "windowed rect — corner mask", window_mask);
            demo_cell(ui, "windowed rect — anatomy", window_anatomy);
        });
    });
}

const A: Vec2 = Vec2::new(15.0, 100.0);
const B: Vec2 = Vec2::new(60.0, 15.0);
const C: Vec2 = Vec2::new(105.0, 100.0);

/// Sharp-cornered solid fill — the aliased case a `Mesh::filled_triangle`
/// would give, now with crisp SDF coverage AA.
fn sharp(ui: &mut Ui) {
    ui.add_shape(Shape::triangle(A, B, C).fill(Color::rgb(0.2, 0.9, 1.0)));
}

/// Rounded corners — `SDF - radius`, no extra geometry.
fn rounded(ui: &mut Ui) {
    ui.add_shape(
        Shape::triangle(A, B, C)
            .fill(Color::rgb(0.4, 1.0, 0.5))
            .radius(12.0),
    );
}

/// Fill + inner-edge stroke, rounded.
fn stroked(ui: &mut Ui) {
    ui.add_shape(
        Shape::triangle(A, B, C)
            .fill(Color::rgb(0.2, 0.5, 1.0))
            .stroke(Stroke::solid(Color::WHITE, 3.0))
            .radius(10.0),
    );
}

/// Stroke only (transparent fill) — a rounded triangular outline.
fn outline(ui: &mut Ui) {
    ui.add_shape(
        Shape::triangle(A, B, C)
            .stroke(Stroke::solid(Color::rgb(1.0, 0.85, 0.2), 3.0))
            .radius(8.0),
    );
}

/// A play-triangle (▶) at three corner radii — the toolbar-glyph use case,
/// from sharp to increasingly soft.
fn radii(ui: &mut Ui) {
    for (i, r) in [0.0_f32, 4.0, 10.0].iter().enumerate() {
        let dx = i as f32 * 40.0;
        ui.add_shape(
            Shape::triangle(
                Vec2::new(10.0 + dx, 20.0),
                Vec2::new(10.0 + dx, 60.0),
                Vec2::new(38.0 + dx, 40.0),
            )
            .fill(Color::rgb(1.0, 0.6, 0.3))
            .radius(*r),
        );
    }
}

/// The headline `WindowedRect` use case: fake rounded-corner clipping
/// without a stencil pass. The gradient "content" is a plain unclipped
/// rect; the windowed rect on top fills the corner wedges with the cell
/// background and strokes the boundary — visually a rounded-clipped card.
fn window_mask(ui: &mut Ui) {
    ui.add_shape(Shape::RoundedRect {
        local_rect: None,
        corners: Corners::ZERO,
        fill: Brush::Linear(LinearGradient::two_stop(
            FRAC_PI_2,
            ColorU8::hex(0x1a1a2e),
            ColorU8::hex(0x4c5cdb),
        )),
        stroke: Stroke::ZERO,
    });
    ui.add_shape(Shape::WindowedRect {
        local_rect: None,
        corners: Corners::all(18.0),
        // Matches `support::panel_bg` so the wedges vanish into the cell.
        fill: Color::hex(0x252525).into(),
        stroke: Stroke::solid(Color::rgb(0.65, 0.8, 1.0), 2.0),
    });
}

/// Translucent fill exposes the geometry: the fill covers only the
/// corner wedges outside the rounded boundary, the stroke hugs the
/// boundary's inner edge, and the window interior stays untouched.
fn window_anatomy(ui: &mut Ui) {
    ui.add_shape(Shape::WindowedRect {
        local_rect: None,
        corners: Corners::all(28.0),
        fill: Color::rgba(1.0, 0.6, 0.3, 0.75).into(),
        stroke: Stroke::solid(Color::rgb(1.0, 0.85, 0.2), 4.0),
    });
}

fn mesh_triangle(ui: &mut Ui) {
    let mut m = Mesh::new();
    let red = Color::rgb(1.0, 0.2, 0.2);
    let a = m.vertex(Vec2::new(60.0, 10.0), red);
    let b = m.vertex(Vec2::new(110.0, 100.0), red);
    let c = m.vertex(Vec2::new(10.0, 100.0), red);
    m.triangle(a, b, c);
    ui.add_shape(Shape::mesh(&m));
}

fn polygon_star(ui: &mut Ui) {
    // 5-pointed star sampled as a fan around the centroid.
    let cx = 60.0_f32;
    let cy = 60.0_f32;
    let r_outer = 55.0_f32;
    let r_inner = 22.0_f32;
    let mut pts = Vec::with_capacity(10);
    for i in 0..10 {
        let theta = -FRAC_PI_2 + i as f32 * PI / 5.0;
        let r = if i % 2 == 0 { r_outer } else { r_inner };
        pts.push(Vec2::new(cx + r * theta.cos(), cy + r * theta.sin()));
    }
    // Fan triangulation around centroid — star is concave; fan-around-
    // first-point would clip; fan-around-centroid is correct here.
    let mut m = Mesh::new();
    let yellow = Color::rgb(1.0, 0.85, 0.2);
    let centroid = m.vertex(Vec2::new(cx, cy), yellow);
    let first = m.vertex(pts[0], yellow);
    let mut prev = first;
    for p in &pts[1..] {
        let next = m.vertex(*p, yellow);
        m.triangle(centroid, prev, next);
        prev = next;
    }
    m.triangle(centroid, prev, first);
    ui.add_shape(Shape::mesh(&m));
}

fn gradient_quad(ui: &mut Ui) {
    // Per-vertex colors create a 4-corner gradient.
    let mut m = Mesh::new();
    let tl = m.vertex(Vec2::new(10.0, 10.0), Color::rgb(1.0, 0.0, 0.0));
    let tr = m.vertex(Vec2::new(110.0, 10.0), Color::rgb(0.0, 1.0, 0.0));
    let br = m.vertex(Vec2::new(110.0, 110.0), Color::rgb(0.0, 0.0, 1.0));
    let bl = m.vertex(Vec2::new(10.0, 110.0), Color::rgb(1.0, 1.0, 0.0));
    m.triangle(tl, tr, br);
    m.triangle(tl, br, bl);
    // White tint — pass-through, exercises the tint multiply path.
    ui.add_shape(Shape::mesh(&m));
}

fn stress(ui: &mut Ui) {
    // 5000-vertex grid of triangles. Exercises the alloc-free claim
    // and the index buffer growth path. The grid renders as a teal
    // wash since every vertex shares the same color.
    const SIDE: u32 = 50; // 50x50 verts = 2500; pair-triangles = ~5000 verts after duplication.
    let teal = Color::rgb(0.2, 0.7, 0.7);
    let mut m = Mesh::with_capacity((SIDE as usize).pow(2), (SIDE as usize - 1).pow(2) * 6);
    let step = 2.0_f32;
    for j in 0..SIDE {
        for i in 0..SIDE {
            m.vertex(Vec2::new(i as f32 * step, j as f32 * step), teal);
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
    ui.add_shape(Shape::mesh(&m));
}
