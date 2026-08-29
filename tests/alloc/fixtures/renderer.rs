//! Fixtures that push shape *count* and shape *variety* through the
//! record path, where the other widget fixtures push structure.
//!
//! What they pin is everything a shape costs between `add_shape` and
//! damage: the record store's per-frame copies (a polyline's points, a
//! mesh's vertex and index bytes), the tree's shape arena, the cascade's
//! paint rows, and the icon registry's re-load. A per-frame `Vec::new()`
//! in any of those shows up here rather than in a structural fixture,
//! which carries one or two shapes per node.
//!
//! **Not encode or compose.** `Audit::run` drives a `UiHarness`, and
//! `Ui::frame` stops at damage — the frontend needs a device. The two
//! gates next door are where those passes are measured, and
//! `gates::scale_ramp_rasterizes_at_a_flat_cost_per_frame` is the one
//! that measures them with full damage on every frame.

use crate::harness::Audit;
use palantir::{
    Color, Configure, Frame, Grid, IconAtlas, IconId, IconSet, Mesh, Panel, PolylineColors, Shape,
    Sizing, Track,
};
use std::rc::Rc;

/// 16×16 grid of `Frame`s — 256 quads per frame. Stresses
/// `RenderCmdBuffer` and `RenderBuffer.quads` capacity reuse much
/// harder than `grid_8x8` (64 quads). A capacity-doubling regression
/// in the encoder shape vec or composer quad vec shows up here.
#[test]
fn many_rects_compose_alloc_free() {
    Audit::new().run(|ui| {
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
    Audit::new().run(move |ui| {
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
    Audit::new().run(move |ui| {
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
/// The set is re-loaded inside the scene, which is the shape an
/// immediate-mode caller writes — so this also pins that re-loading a set
/// every frame is a refcount bump. `IconRegistry::register` finds the live
/// `IconSet` over that allocation and hands back a clone of it; a
/// regression that took a second slot would show up here before it showed
/// up as unbounded growth.
///
/// The set is *parked* across frames, which is the contract: an `IconSet`
/// owns its parses and its atlas rasters, so a scene that dropped the one
/// it loaded would unload them at every submit and rasterize afresh at
/// every frame. Keeping it is what the `#[must_use]` on `load_icons` is
/// telling the caller to do.
#[test]
fn many_icons_compose_alloc_free() {
    const SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><rect width="16" height="16" rx="3" fill="#fff"/></svg>"##;
    let atlas = Rc::new(IconAtlas::from_svgs([("chip", SVG)]));
    let chip = IconId(0);
    let mut held: Option<IconSet> = None;

    Audit::new().run(move |ui| {
        // `insert` drops last frame's clone *after* this frame's exists,
        // so the shared owner never reaches zero and nothing is released.
        let icons = held.insert(ui.load_icons(Rc::clone(&atlas)));
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
