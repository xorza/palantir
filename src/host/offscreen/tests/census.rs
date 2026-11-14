//! The batching census of scenes the showcase lacks: the calls each one's main pass records and
//! why its groups closed, which a change to batching moves.

use crate::diagnostics::gpu_pass_stats::PassCounts;
use crate::gpu::test_gpu::headless_test_gpu;
use crate::host::offscreen::OffscreenHost;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::cubic_bezier::CubicBezier;
use crate::primitives::geometry::mesh::Mesh;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::paint::border::Border;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::stroke::Stroke;
use crate::renderer::frontend::composer::paint_order::internals::{Closes, GroupCloses};
use crate::shape::Shape;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widgets::panel::Panel;
use crate::widgets::scroll::Scroll;
use crate::widgets::text::Text;
use crate::widgets::text_edit::TextEdit;
use glam::{UVec2, Vec2};
use std::cell::Cell;

/// What a scene's settled frame recorded.
#[derive(Debug)]
struct Census {
    counts: PassCounts,
    closes: GroupCloses,
}

/// `scene` painted on a 320 × 240 target until it settles, its last frame's census.
fn census_of(mut scene: impl FnMut(&mut Ui)) -> Census {
    let gpu = headless_test_gpu();
    let mut host = OffscreenHost::builder(gpu.handles()).build();
    let target = gpu.target("palantir.test.census", UVec2::new(320, 240));
    for _ in 0..3 {
        host.frame(target.as_target(), &mut scene);
    }
    host.invalidate_target_contents();
    host.frame(target.as_target(), &mut scene);
    Census {
        counts: host.ui().gpu_pass_stats().last_pass_counts(),
        closes: host.group_closes(),
    }
}

/// A card's chrome: a rounded fill and a border.
fn card() -> Chrome {
    Chrome::rounded(RgbaF32::hex(0x30364a), Corners::all(6.0))
        .with_border(Border::new(RgbaF32::hex(0x5a6378), 1.0))
}

/// Six text fields in a column: their clips cut nothing and close no group, so the column is one
/// group, closed at the session's end, in 2 draws.
#[test]
fn a_column_of_fields() {
    let mut texts: [String; 6] = std::array::from_fn(|i| format!("field {i}"));
    let census = census_of(|ui| {
        Panel::vstack().id_salt("fields").show(ui, |ui| {
            for (i, text) in texts.iter_mut().enumerate() {
                TextEdit::new(text)
                    .id_salt(("field", i))
                    .size((Sizing::fixed(200.0), Sizing::fixed(24.0)))
                    .show(ui);
            }
        });
    });
    assert_eq!(
        census.counts,
        PassCounts {
            draws: 2,
            pipelines: 2,
            groups: 2,
            scissors: 1,
            viewports: 1,
            vertex_buffers: 2,
            index_buffers: 1,
        }
    );
    assert_eq!(
        census.closes,
        GroupCloses {
            end: Closes {
                groups: 1,
                draws_after: 0,
            },
            ..GroupCloses::default()
        }
    );
}

/// A scroll viewport of 100 px over rows of 20, scrolled by 10, so its first and last rows are
/// cut at its edges: the viewport's clip closes no group, and the rows it cuts draw under it, in 3
/// draws.
#[test]
fn a_scroll_viewport_cuts_its_first_and_last_rows() {
    let list = WidgetId::from_hash("list");
    let offset = Cell::new(Vec2::ZERO);
    let census = census_of(|ui| {
        Scroll::set_offset(ui, list, Vec2::new(0.0, 10.0));
        Scroll::vertical()
            .id(list)
            .size((Sizing::fixed(200.0), Sizing::fixed(100.0)))
            .show(ui, |ui| {
                for i in 0..12 {
                    Panel::hstack()
                        .id_salt(("row", i))
                        .size((Sizing::FILL, Sizing::fixed(20.0)))
                        .chrome(Chrome::new(RgbaF32::hex(0x262b38)))
                        .show(ui, |ui| {
                            Text::new(format!("row {i}")).id_salt(("label", i)).show(ui);
                        });
                }
            });
        offset.set(Scroll::offset(ui, list));
    });
    assert_eq!(offset.get(), Vec2::new(0.0, 10.0), "the rows sit 10 px up");
    assert_eq!(
        census.counts,
        PassCounts {
            draws: 3,
            pipelines: 2,
            groups: 2,
            scissors: 2,
            viewports: 1,
            vertex_buffers: 2,
            index_buffers: 1,
        }
    );
    assert_eq!(
        census.closes,
        GroupCloses {
            end: Closes {
                groups: 1,
                draws_after: 0,
            },
            ..GroupCloses::default()
        }
    );
}

/// A clipped canvas of a mesh, two curves and four cards with a field each, the second and third
/// cards overlapping, as a node graph draws: six closes for paint order and one at the session's
/// end, in 14 draws.
#[test]
fn a_clipped_canvas_of_cards_with_fields() {
    let mut mesh = Mesh::with_capacity(4, 6);
    let a = mesh.add_vertex(Vec2::new(10.0, 10.0), RgbaF32::hex(0x1d2230));
    let b = mesh.add_vertex(Vec2::new(290.0, 10.0), RgbaF32::hex(0x1d2230));
    let c = mesh.add_vertex(Vec2::new(290.0, 190.0), RgbaF32::hex(0x2a3142));
    let d = mesh.add_vertex(Vec2::new(10.0, 190.0), RgbaF32::hex(0x2a3142));
    mesh.add_triangle(a, b, c);
    mesh.add_triangle(a, c, d);
    let mut texts: [String; 4] = std::array::from_fn(|i| format!("value {i}"));
    let at = [(20.0, 20.0), (150.0, 30.0), (190.0, 60.0), (40.0, 120.0)];
    let census = census_of(|ui| {
        Panel::canvas()
            .id_salt("graph")
            .size((Sizing::fixed(300.0), Sizing::fixed(200.0)))
            .clip_rect()
            .show(ui, |ui| {
                ui.add_shape(Shape::mesh(&mesh), RgbaF32::WHITE);
                for (from, to) in [(0, 1), (0, 3)] {
                    let (p0, p3) = (
                        Vec2::from(at[from]) + Vec2::new(100.0, 20.0),
                        Vec2::from(at[to]) + Vec2::new(0.0, 20.0),
                    );
                    ui.add_shape(
                        Shape::cubic_bezier(CubicBezier {
                            p0,
                            p1: p0 + Vec2::new(40.0, 0.0),
                            p2: p3 - Vec2::new(40.0, 0.0),
                            p3,
                        }),
                        Stroke::new(RgbaF32::hex(0xffa63d), 2.0),
                    );
                }
                for (i, text) in texts.iter_mut().enumerate() {
                    Panel::vstack()
                        .id_salt(("card", i))
                        .at(at[i])
                        .size((Sizing::fixed(100.0), Sizing::fixed(56.0)))
                        .chrome(card())
                        .show(ui, |ui| {
                            Text::new(format!("node {i}"))
                                .id_salt(("title", i))
                                .show(ui);
                            TextEdit::new(text)
                                .id_salt(("value", i))
                                .size((Sizing::FILL, Sizing::fixed(22.0)))
                                .show(ui);
                        });
                }
            });
    });
    assert_eq!(
        census.counts,
        PassCounts {
            draws: 14,
            pipelines: 14,
            groups: 2,
            scissors: 2,
            viewports: 1,
            vertex_buffers: 15,
            index_buffers: 3,
        }
    );
    assert_eq!(
        census.closes,
        GroupCloses {
            paint_order: Closes {
                groups: 6,
                draws_after: 56,
            },
            end: Closes {
                groups: 1,
                draws_after: 0,
            },
            ..GroupCloses::default()
        }
    );
}
