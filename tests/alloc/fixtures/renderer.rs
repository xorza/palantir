//! Fixtures that target the renderer frontend (encode + compose).
//! Every existing widget fixture already drives `Frontend::build`, but
//! at a tiny shape count — these scale up shape counts and exercise
//! the non-`RoundedRect` shape variants (`Polyline`, `Mesh`) so a
//! per-frame `Vec::new()` in those paths can't slip in unnoticed.

use crate::harness::audit_steady_state;
use palantir::{
    Color, Configure, Frame, Grid, LineCap, LineJoin, Mesh, Panel, PolylineColors, Shape, Sizing,
    Track,
};
use std::rc::Rc;

/// 16×16 grid of `Frame`s — 256 quads per frame. Stresses
/// `RenderCmdBuffer` and `RenderBuffer.quads` capacity reuse much
/// harder than `grid_8x8` (64 quads). A capacity-doubling regression
/// in the encoder shape vec or composer quad vec shows up here.
#[test]
fn many_rects_compose_alloc_free() {
    let cols: Rc<[Track]> = Rc::from([Track::fill(); 16]);
    let rows: Rc<[Track]> = Rc::from([Track::fill(); 16]);
    audit_steady_state("many_rects_compose", 0, move |ui| {
        Grid::new()
            .auto_id()
            .cols(Rc::clone(&cols))
            .rows(Rc::clone(&rows))
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                for r in 0..16u16 {
                    for c in 0..16u16 {
                        Frame::new()
                            .id_salt((r, c))
                            .background(palantir::Background {
                                fill: Color::WHITE.into(),
                                ..Default::default()
                            })
                            .grid_cell((r, c))
                            .show(ui);
                    }
                }
            });
    });
}

/// Static polyline pushed every frame. Slice borrows are copied into
/// the tree's per-frame arena at `add_shape` time, so the closure can
/// hold the `Vec` and hand `&points[..]` to the shape variant. Pins
/// the polyline tessellator's scratch reuse.
#[test]
fn polyline_static_alloc_free() {
    let points: Vec<glam::Vec2> = (0..32)
        .map(|i| glam::Vec2::new(i as f32 * 20.0, 100.0 + (i as f32).sin() * 30.0))
        .collect();
    audit_steady_state("polyline_static", 0, move |ui| {
        Panel::vstack()
            .auto_id()
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                ui.add_shape(Shape::Polyline {
                    points: &points,
                    colors: PolylineColors::Single(Color::WHITE),
                    width: 2.0,
                    cap: LineCap::Butt,
                    join: LineJoin::Miter,
                });
            });
    });
}

/// Static `Mesh` pushed every frame via `Ui::add_shape`. Vertex / index
/// bytes are copied into the tree's mesh arena at `add_shape` time,
/// so the mesh built once outside the closure is reused as-is. Pins
/// that the mesh-encoding command path doesn't allocate at steady
/// state.
#[test]
fn mesh_static_alloc_free() {
    let mesh = {
        let mut m = Mesh::with_capacity(3, 3);
        let a = m.vertex(glam::Vec2::new(0.0, 0.0), Color::WHITE);
        let b = m.vertex(glam::Vec2::new(100.0, 0.0), Color::WHITE);
        let c = m.vertex(glam::Vec2::new(50.0, 100.0), Color::WHITE);
        m.triangle(a, b, c);
        m
    };
    audit_steady_state("mesh_static", 0, move |ui| {
        Panel::vstack()
            .auto_id()
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                ui.add_shape(Shape::Mesh {
                    mesh: &mesh,
                    local_rect: None,
                    tint: Color::WHITE.into(),
                });
            });
    });
}
