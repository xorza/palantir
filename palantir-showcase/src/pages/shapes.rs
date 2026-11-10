//! Filled shape primitives: triangles and meshes.
#![expect(
    clippy::disallowed_methods,
    reason = "a demo of the crate, whose frames owe no two systems the same bits"
)]

use crate::scaffold::api;
use crate::support;
use crate::support::{DemoCell, Kit, Knob, Section};
use palantir::Paint;
use palantir::glam::Vec2;
use palantir::widget::{Mesh, Shape};
use palantir::{Border, RgbaF32, Ui, WidgetId};
use std::f32::consts::{FRAC_PI_2, PI};

pub(crate) fn build(ui: &mut Ui, k: Kit) {
    live(ui, k);
    Section::new("Triangles")
        .apis(&[api!(Shape::triangle)])
        .show(ui, k, |ui| {
            support::note(
                ui,
                k,
                "One instanced quad each: coverage, rounded corners and the border \
                 all come from the signed distance field.",
            );
            support::tiles(ui, |ui| {
                DemoCell::new("sharp fill").show(ui, k, |ui| sharp(ui, k));
                DemoCell::new("rounded 12 px").show(ui, k, |ui| rounded(ui, k));
                DemoCell::new("fill + border").show(ui, k, |ui| bordered(ui, k));
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
}

const A: Vec2 = Vec2::new(20.0, 142.0);
const B: Vec2 = Vec2::new(84.0, 24.0);
const C: Vec2 = Vec2::new(148.0, 142.0);

fn sharp(ui: &mut Ui, k: Kit) {
    ui.add_shape(Shape::triangle(A, B, C), k.swatch.teal);
}

fn rounded(ui: &mut Ui, k: Kit) {
    ui.add_shape(Shape::triangle(A, B, C).radius(12.0_f32), k.swatch.lime);
}

fn bordered(ui: &mut Ui, k: Kit) {
    ui.add_shape(
        Shape::triangle(A, B, C).radius(10.0_f32),
        Paint::from(k.swatch.violet).with_border(Border::new(RgbaF32::WHITE, 3.0)),
    );
}

fn outline(ui: &mut Ui, k: Kit) {
    ui.add_shape(
        Shape::triangle(A, B, C).radius(8.0_f32),
        Paint::NONE.with_border(Border::new(k.swatch.orange, 3.0)),
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
            .radius(*r),
            k.swatch.orange,
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
        ui.add_shape(Shape::mesh(m), RgbaF32::WHITE);
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

/// The live triangle's knobs.
#[derive(Debug)]
struct LiveTriangle {
    radius: f32,
    border: f32,
}

impl Default for LiveTriangle {
    fn default() -> Self {
        Self {
            radius: 12.0,
            border: 3.0,
        }
    }
}

/// One triangle, its corner radius and its border's width from the knobs, all from one SDF.
fn live(ui: &mut Ui, k: Kit) {
    Section::new("Live triangle")
        .apis(&[api!(Shape::triangle)])
        .show(ui, k, |ui| {
            support::note(
                ui,
                k,
                "Round the corners and widen the border: the same instanced quad draws every \
                 value, its coverage from the signed distance field.",
            );
            let id = WidgetId::from_hash("showcase::shapes::live");
            ui.with_state::<LiveTriangle, _>(id, |ui, l| {
                Knob {
                    label: "radius",
                    range: 0.0..=40.0,
                    step: 1.0,
                    unit: " px",
                }
                .show(ui, k, &mut l.radius);
                Knob {
                    label: "border",
                    range: 0.0..=12.0,
                    step: 0.5,
                    unit: " px",
                }
                .show(ui, k, &mut l.border);
                let (radius, border) = (l.radius, l.border);
                DemoCell::new("the knobs' triangle").show(ui, k, |ui| {
                    ui.add_shape(
                        Shape::triangle(A, B, C).radius(radius),
                        Paint::from(k.swatch.violet).with_border(Border::new(k.t.ink, border)),
                    );
                });
            });
        });
}
