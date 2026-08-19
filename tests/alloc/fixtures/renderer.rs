//! Fixtures that target the renderer frontend (encode + compose).
//! Every existing widget fixture already drives `Frontend::build`, but
//! at a tiny shape count — these scale up shape counts and exercise
//! the non-rectangle shape variants (`Polyline`, `Mesh`) so a
//! per-frame `Vec::new()` in those paths can't slip in unnoticed.

use crate::harness::audit_steady_state;
use palantir::{
    Color, Configure, Frame, Grid, IconAtlas, IconId, Mesh, Panel, PolylineColors, Shape, Sizing,
    Track,
};
use std::rc::Rc;

/// 16×16 grid of `Frame`s — 256 quads per frame. Stresses
/// `RenderCmdBuffer` and `RenderBuffer.quads` capacity reuse much
/// harder than `grid_8x8` (64 quads). A capacity-doubling regression
/// in the encoder shape vec or composer quad vec shows up here.
#[test]
fn many_rects_compose_alloc_free() {
    audit_steady_state(0, |ui| {
        Grid::new()
            .auto_id()
            .cols([Track::fill(); 16])
            .rows([Track::fill(); 16])
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
/// the window's record store at `add_shape` time, so the closure can
/// hold the `Vec` and hand `&points[..]` to the shape variant. Pins
/// the composer's polyline point/index/direction scratch reuse.
#[test]
fn polyline_static_alloc_free() {
    let points: Vec<glam::Vec2> = (0..32)
        .map(|i| glam::Vec2::new(i as f32 * 20.0, 100.0 + (i as f32).sin() * 30.0))
        .collect();
    audit_steady_state(0, move |ui| {
        Panel::vstack()
            .auto_id()
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                ui.add_shape(Shape::polyline(
                    &points,
                    PolylineColors::Single(Color::WHITE),
                    2.0,
                ));
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
    audit_steady_state(0, move |ui| {
        Panel::vstack()
            .auto_id()
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                ui.add_shape(Shape::mesh(&mesh));
            });
    });
}

/// 200 icons per frame. Every one goes record → encode → compose, so the
/// per-frame cost is a push onto `RenderBuffer.icons` and nothing else: the
/// raster and its atlas slot are resolved once, on the frame that first drew
/// the icon, and re-found by a map probe thereafter.
///
/// The set is loaded inside the scene, which is the shape an immediate-mode
/// caller writes — so this also pins that re-loading a set every frame does
/// not allocate. `IconRegistry::register` walks its table and hands back the
/// existing id; a regression that pushed a duplicate entry would show up here
/// before it showed up as unbounded growth.
#[test]
fn many_icons_compose_alloc_free() {
    const SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><rect width="16" height="16" rx="3" fill="#fff"/></svg>"##;
    let atlas = Rc::new(IconAtlas::from_svgs([("chip", SVG)]));
    let chip = IconId(0);

    audit_steady_state(0, move |ui| {
        let icons = ui.load_icons(Rc::clone(&atlas));
        Grid::new()
            .auto_id()
            .cols([Track::fill(); 20])
            .rows([Track::fill(); 10])
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                for r in 0..10u16 {
                    for c in 0..20u16 {
                        Panel::zstack()
                            .id_salt((r, c))
                            .grid_cell((r, c))
                            .show(ui, |ui| {
                                ui.add_shape(icons.shape(chip));
                            });
                    }
                }
            });
    });
}
