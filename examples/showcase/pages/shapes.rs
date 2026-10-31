//! Filled shape primitives: triangles, meshes and
//! `Shape::owner_windowed_rect`, the inverted-fill corner mask standing in for rounded clipping without a
//! stencil pass.

use crate::scaffold::api;
use crate::support;
use crate::support::{DemoCell, Kit, Section};
use palantir::glam::Vec2;
use palantir::widget::{Mesh, Shape};
use palantir::{LinearGradient, RgbaF32, Stroke, Ui, WidgetId};
use std::f32::consts::{FRAC_PI_2, PI};

pub(crate) fn build(ui: &mut Ui, k: Kit) {
    Section::new("Triangles")
        .apis(&[api!(Shape::triangle)])
        .show(ui, k, |ui| {
            support::note(
                ui,
                k,
                "One instanced quad each: coverage, rounded corners and the stroke \
                 all come from the signed distance field.",
            );
            support::tiles(ui, |ui| {
                DemoCell::new("sharp fill").show(ui, k, |ui| sharp(ui, k));
                DemoCell::new("rounded 12 px").show(ui, k, |ui| rounded(ui, k));
                DemoCell::new("fill + inner stroke").show(ui, k, |ui| stroked(ui, k));
                DemoCell::new("outline only").show(ui, k, |ui| outline(ui, k));
                DemoCell::new("play glyph — radii 0 / 4 / 10").show(ui, k, |ui| radii(ui, k));
            });
        });

    Section::new("Meshes")
        .apis(&[api!(type Mesh), api!(Shape::mesh)])
        .show(ui, k, |ui| {
            support::note(
                ui,
                k,
                "Raw vertices and indices, sent straight to the mesh pipeline. Each \
                 mesh is built once and kept in a state row.",
            );
            support::tiles(ui, |ui| {
                DemoCell::new("single triangle").show(ui, k, |ui| mesh_triangle(ui, k));
                DemoCell::new("star — centroid fan").show(ui, k, |ui| polygon_star(ui, k));
                DemoCell::new("per-vertex gradient").show(ui, k, |ui| gradient_quad(ui, k));
            });
        });

    Section::new("Windowed rect")
        .apis(&[api!(Shape::owner_windowed_rect)])
        .show(ui, k, |ui| {
            support::note(
                ui,
                k,
                "An inverted rounded-rect fill: it paints the corner wedges and \
                 leaves the window alone, which gives rounded clipping with no \
                 stencil pass.",
            );
            support::tiles(ui, |ui| {
                DemoCell::new("corner mask over content").show(ui, k, |ui| window_mask(ui, k));
                DemoCell::new("anatomy — translucent fill").show(ui, k, |ui| window_anatomy(ui, k));
            });
        });
}

const A: Vec2 = Vec2::new(20.0, 142.0);
const B: Vec2 = Vec2::new(84.0, 24.0);
const C: Vec2 = Vec2::new(148.0, 142.0);

fn sharp(ui: &mut Ui, k: Kit) {
    ui.add_shape(Shape::triangle(A, B, C).fill(k.swatch.teal));
}

fn rounded(ui: &mut Ui, k: Kit) {
    ui.add_shape(
        Shape::triangle(A, B, C)
            .fill(k.swatch.lime)
            .radius(12.0_f32),
    );
}

fn stroked(ui: &mut Ui, k: Kit) {
    ui.add_shape(
        Shape::triangle(A, B, C)
            .fill(k.swatch.violet)
            .border(Stroke::new(RgbaF32::WHITE, 3.0))
            .radius(10.0_f32),
    );
}

fn outline(ui: &mut Ui, k: Kit) {
    ui.add_shape(
        Shape::triangle(A, B, C)
            .border(Stroke::new(k.swatch.orange, 3.0))
            .radius(8.0_f32),
    );
}

fn radii(ui: &mut Ui, k: Kit) {
    for (i, r) in [0.0_f32, 4.0, 10.0].iter().enumerate() {
        let dy = i as f32 * 46.0;
        ui.add_shape(
            Shape::triangle(
                Vec2::new(62.0, 16.0 + dy),
                Vec2::new(62.0, 54.0 + dy),
                Vec2::new(98.0, 35.0 + dy),
            )
            .fill(k.swatch.orange)
            .radius(*r),
        );
    }
}

/// Builds a cell's geometry into a retained row once: a fresh `Mesh` per frame allocates and re-hashes (it
/// memoizes its content hash), even at three vertices.
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

fn mesh_triangle(ui: &mut Ui, k: Kit) {
    retained_mesh(ui, "triangle", |m| {
        let a = m.vertex(B, k.swatch.red);
        let b = m.vertex(C, k.swatch.red);
        let c = m.vertex(A, k.swatch.red);
        m.triangle(a, b, c);
    });
}

/// 5-pointed star fanned around the centroid, since it is concave.
fn polygon_star(ui: &mut Ui, k: Kit) {
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
        let centroid = m.vertex(Vec2::new(cx, cy), k.swatch.orange);
        let first = m.vertex(point(0), k.swatch.orange);
        let mut prev = first;
        for i in 1..10 {
            let next = m.vertex(point(i), k.swatch.orange);
            m.triangle(centroid, prev, next);
            prev = next;
        }
        m.triangle(centroid, prev, first);
    });
}

fn gradient_quad(ui: &mut Ui, k: Kit) {
    retained_mesh(ui, "gradient-quad", |m| {
        let tl = m.vertex(Vec2::new(16.0, 16.0), k.swatch.red);
        let tr = m.vertex(Vec2::new(152.0, 16.0), k.swatch.lime);
        let br = m.vertex(Vec2::new(152.0, 152.0), k.swatch.teal);
        let bl = m.vertex(Vec2::new(16.0, 152.0), k.swatch.orange);
        m.triangle(tl, tr, br);
        m.triangle(tl, br, bl);
    });
}

/// The headline use: rounded-corner clipping without a stencil pass; the windowed rect fills the corner wedges
/// with the tile background and strokes the boundary over a plain unclipped gradient.
fn window_mask(ui: &mut Ui, k: Kit) {
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
            .fill(k.t.ground)
            .border(Stroke::new(k.swatch.teal, 2.0)),
    );
}

/// A translucent fill exposes the geometry: only the corner wedges outside the rounded boundary are covered.
fn window_anatomy(ui: &mut Ui, k: Kit) {
    ui.add_shape(
        Shape::owner_windowed_rect()
            .corners(28.0)
            .fill(k.swatch.orange.with_alpha(0.75))
            .border(Stroke::new(k.swatch.lime, 4.0)),
    );
}
