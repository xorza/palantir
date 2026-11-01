//! A hugging grid: what an empty fill track collapses to, and what a nested
//! floor pushes back.

use crate::host::harness::Harness;
use crate::layout::intrinsic::len_req::LenReq;
use crate::primitives::geometry::size::Size;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::axis::Axis;
use crate::primitives::layout::grid_cell::GridCell;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::layout::track::Track;
use crate::scene::layer::Layer;
use crate::widget_core::configure::Configure;
use crate::widgets::{block::Block, grid::Grid, panel::Panel};
use glam::UVec2;

#[test]
fn grid_hug_grid_collapses_empty_fill_tracks() {
    let mut h = Harness::mono(UVec2::new(400, 200));
    h.frame(|ui| {
        Panel::hstack()
            .auto_id()
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                Grid::new()
                    .id(WidgetId::from_hash("hug-grid"))
                    .cols([Track::fixed(80.0), Track::FILL])
                    .rows([Track::fixed(40.0)])
                    .size((Sizing::HUG, Sizing::HUG))
                    .show(ui, |ui| {
                        Block::new()
                            .id(WidgetId::from_hash("a"))
                            .grid_cell((0, 0))
                            .show(ui);
                        Block::new()
                            .id(WidgetId::from_hash("b"))
                            .grid_cell((0, 1))
                            .show(ui);
                    });
            });
    });
    let r = h.arranged(WidgetId::from_hash("hug-grid"));
    assert_eq!(r.size.w, 80.0, "empty Fill col contributes no floor");
    assert_eq!(r.size.h, 40.0);
}

#[test]
fn hug_grid_fill_track_contributes_nested_rigid_floor() {
    let mut h = Harness::mono(UVec2::new(400, 100));
    let root = h.frame_value(|ui| {
        Grid::new()
            .auto_id()
            .cols([Track::FILL])
            .rows([Track::fixed(20.0)])
            .size((Sizing::HUG, Sizing::HUG))
            .show(ui, |ui| {
                Panel::hstack()
                    .id(WidgetId::from_hash("cell"))
                    .grid_cell((0, 0))
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui, |ui| {
                        Block::new()
                            .id(WidgetId::from_hash("rigid"))
                            .size((Sizing::fixed(120.0), Sizing::fixed(20.0)))
                            .show(ui);
                    });
            })
            .response
            .node()
    });

    let grid = h.ui().arranged_rect(Layer::Main, root);
    let cell = h.main_child_rects(root)[0];
    let rigid = h.arranged(WidgetId::from_hash("rigid"));
    assert_eq!(grid.size, Size::new(120.0, 20.0));
    assert_eq!(cell.size, Size::new(120.0, 20.0));
    assert_eq!(rigid.size, Size::new(120.0, 20.0));
}

#[test]
fn stack_fill_sibling_yields_to_grid_fill_track_rigid_floor() {
    let mut h = Harness::mono(UVec2::new(300, 40));
    let root = h.frame_value(|ui| {
        Panel::hstack()
            .auto_id()
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                Grid::new()
                    .id(WidgetId::from_hash("grid"))
                    .cols([Track::FILL])
                    .rows([Track::FILL])
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui, |ui| {
                        Panel::hstack()
                            .id(WidgetId::from_hash("cell"))
                            .grid_cell((0, 0))
                            .size((Sizing::FILL, Sizing::FILL))
                            .show(ui, |ui| {
                                Block::new()
                                    .id(WidgetId::from_hash("rigid"))
                                    .size((Sizing::fixed(200.0), Sizing::FILL))
                                    .show(ui);
                            });
                    });
                Block::new()
                    .id(WidgetId::from_hash("shrinkable"))
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui);
            })
            .response
            .node()
    });

    let siblings = h.main_child_rects(root);
    let rigid = h.arranged(WidgetId::from_hash("rigid"));
    assert_eq!(siblings[0].size.w, 200.0);
    assert_eq!(siblings[1].min.x, 200.0);
    assert_eq!(siblings[1].size.w, 100.0);
    assert_eq!(rigid.size.w, 200.0);
}

/// A hugging grid's Fill column takes its cells' max-content width, as a Stack's Fill child does:
/// the wrapping label keeps one line, and the row that measured it at one line holds it. Before,
/// the column shrank to the longest word, and the label wrapped out of its row.
#[test]
fn hug_grid_fill_column_holds_its_wrapping_text() {
    use crate::text::wrap::TextWrap;
    use crate::widgets::text::Text;

    let mut h = Harness::new(UVec2::new(600, 200));
    h.frame(|ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Grid::new()
                .id(WidgetId::from_hash("hug-grid"))
                .cols([Track::fixed(40.0), Track::FILL])
                .rows([Track::HUG])
                .size((Sizing::HUG, Sizing::HUG))
                .show(ui, |ui| {
                    Text::new("several words that fit on one line")
                        .id(WidgetId::from_hash("label"))
                        .grid_cell((0, 1))
                        .text_wrap(TextWrap::WrapWithOverflow)
                        .show(ui);
                });
        });
    });
    let grid = h.arranged(WidgetId::from_hash("hug-grid"));
    let label = h.arranged(WidgetId::from_hash("label"));
    let shaped = h.ui().layout(Layer::Main).text_shapes[0].extent.size;
    assert_eq!(
        label.size.w, shaped.w,
        "the column is the label's one-line width"
    );
    assert_eq!(grid.size.w, 40.0 + shaped.w);
    assert_eq!(grid.size.h, shaped.h, "the row holds the label it measured");
    assert_eq!(label.size.h, shaped.h);
}

/// A hugging grid's Fill columns each take at least their cells' width, sized by CSS Grid's
/// flexible fraction for indefinite space (§12.7.1): `fr = max(content / weight)`, each column
/// `fr · weight`. Cells 200 and 20 wide in weights 1 and 1 make `fr = 200` and two 200-wide
/// columns, 400 in all; weights 1 and 3 with cells 200 and 300 make `fr = max(200, 100) = 200`,
/// 200 and 600. A cell spanning both columns of weights 1 and 1, 300 wide, makes `fr = 150`. A
/// column's `min` counts as its extent: a 250 floor beside a 20-wide cell makes `fr = 250`.
/// The grid's max-content width is the same. Before, the columns split the cells' summed width by
/// weight, so a wide cell covered its neighbor.
#[test]
fn hug_grid_fill_columns_each_hold_their_cells() {
    /// A cell at column `col`, `span` columns wide, `w` wide itself.
    type Cell = (u16, u16, f32);
    let mut h = Harness::mono(UVec2::new(1200, 200));
    let fill = Track::fill(1.0);
    let cases: [([Track; 2], &[Cell], [f32; 2]); 4] = [
        ([fill, fill], &[(0, 1, 200.0), (1, 1, 20.0)], [200.0, 200.0]),
        (
            [fill, Track::fill(3.0)],
            &[(0, 1, 200.0), (1, 1, 300.0)],
            [200.0, 600.0],
        ),
        ([fill, fill], &[(0, 2, 300.0)], [150.0, 150.0]),
        (
            [fill.with_min(250.0), fill],
            &[(1, 1, 20.0)],
            [250.0, 250.0],
        ),
    ];
    for (tracks, cells, want) in cases {
        let weights = tracks.map(|t| t.sizing().fill_weight());
        h.frame(|ui| {
            Panel::hstack().auto_id().show(ui, |ui| {
                Grid::new()
                    .id(WidgetId::from_hash("hug-grid"))
                    .cols(tracks)
                    .rows([Track::fixed(20.0)])
                    .size((Sizing::HUG, Sizing::HUG))
                    .show(ui, |ui| {
                        for &(col, span, w) in cells {
                            Block::new()
                                .id(WidgetId::from_hash(("cell", col)))
                                .grid_cell(GridCell::at(0, col).with_span(1, span))
                                .size((Sizing::fixed(w), Sizing::fixed(20.0)))
                                .show(ui);
                        }
                    });
            });
        });
        let grid = h.arranged(WidgetId::from_hash("hug-grid"));
        assert_eq!(grid.size.w, want[0] + want[1], "{weights:?} {cells:?}");
        let node = h.node_for_widget_id(WidgetId::from_hash("hug-grid"));
        assert_eq!(
            h.intrinsic(node, Axis::X, LenReq::MaxContent),
            want[0] + want[1],
            "{weights:?} {cells:?}: the max-content width is the hugged one"
        );
        for &(col, span, w) in cells {
            let cell = h.arranged(WidgetId::from_hash(("cell", col)));
            let left: f32 = want[..usize::from(col)].iter().sum();
            assert_eq!(cell.min.x, left, "{weights:?} cell {col}");
            let room: f32 = want[usize::from(col)..usize::from(col + span)].iter().sum();
            assert!(cell.size.w <= room, "{weights:?} cell {col}: {w} in {room}");
        }
    }
}
