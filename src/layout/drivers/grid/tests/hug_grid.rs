//! A hugging grid: what an empty fill track collapses to, and what a nested
//! floor pushes back.

use crate::internals::layout_rig::{GridTracks, LayoutRig, NodeRecorder, TextRun};
use crate::layout::intrinsic::len_req::LenReq;
use crate::primitives::geometry::size::Size;
use crate::primitives::layout::axis::Axis;
use crate::primitives::layout::grid_cell::GridCell;
use crate::primitives::layout::sizing::{SizeSpec, Sizing};
use crate::primitives::layout::track::Track;
use crate::text::wrap::TextWrap;
use crate::widget_core::configure::Configure;
use crate::widget_core::widget::Widget;
use std::num::NonZeroU16;

#[test]
fn grid_hug_grid_collapses_empty_fill_tracks() {
    let mut rig = LayoutRig::new();
    rig.run(Size::new(400.0, 200.0), |rec| {
        rec.node("row", Widget::hstack().size(SizeSpec::FILL), |rec| {
            let tracks = GridTracks {
                rows: &[Track::fixed(40.0)],
                cols: &[Track::fixed(80.0), Track::FILL],
            };
            rec.grid("hug-grid", Widget::grid(), tracks, |rec| {
                rec.leaf("a", Widget::leaf().grid_cell((0, 0)));
                rec.leaf("b", Widget::leaf().grid_cell((0, 1)));
            });
        });
    });
    assert_eq!(
        rig.rect("hug-grid").size,
        Size::new(80.0, 40.0),
        "an empty Fill column contributes no floor"
    );
}

#[test]
fn hug_grid_fill_track_contributes_nested_rigid_floor() {
    let mut rig = LayoutRig::new();
    rig.run(Size::new(400.0, 100.0), |rec| {
        let tracks = GridTracks {
            rows: &[Track::fixed(20.0)],
            cols: &[Track::FILL],
        };
        rec.grid("grid", Widget::grid(), tracks, |rec| {
            let cell = Widget::hstack().grid_cell((0, 0)).size(SizeSpec::FILL);
            rec.node("cell", cell, |rec| {
                rec.leaf("rigid", Widget::leaf().size((120.0, 20.0)));
            });
        });
    });
    for id in ["grid", "cell", "rigid"] {
        assert_eq!(rig.rect(id).size, Size::new(120.0, 20.0), "{id}");
    }
}

#[test]
fn stack_fill_sibling_yields_to_grid_fill_track_rigid_floor() {
    let mut rig = LayoutRig::new();
    rig.run(Size::new(300.0, 40.0), |rec| {
        rec.node("row", Widget::hstack().size(SizeSpec::FILL), |rec| {
            let tracks = GridTracks {
                rows: &[Track::FILL],
                cols: &[Track::FILL],
            };
            let grid = Widget::grid().size(SizeSpec::FILL);
            rec.grid("grid", grid, tracks, |rec| {
                let cell = Widget::hstack().grid_cell((0, 0)).size(SizeSpec::FILL);
                rec.node("cell", cell, |rec| {
                    rec.leaf(
                        "rigid",
                        Widget::leaf().size((Sizing::fixed(200.0), Sizing::FILL)),
                    );
                });
            });
            rec.leaf("shrinkable", Widget::leaf().size(SizeSpec::FILL));
        });
    });
    assert_eq!(rig.rect("grid").size.w, 200.0);
    let shrinkable = rig.rect("shrinkable");
    assert_eq!((shrinkable.min.x, shrinkable.size.w), (200.0, 100.0));
    assert_eq!(rig.rect("rigid").size.w, 200.0);
}

/// A hugging grid's Fill column takes its cells' max-content width, as a Stack's Fill child does:
/// the wrapping label keeps one line, and the row that measured it at one line holds it. A column
/// at the longest word would wrap the label out of its row.
#[test]
fn hug_grid_fill_column_holds_its_wrapping_text() {
    let mut rig = LayoutRig::new();
    rig.run(Size::new(600.0, 200.0), |rec| {
        rec.node("row", Widget::hstack(), |rec| {
            let tracks = GridTracks {
                rows: &[Track::HUG],
                cols: &[Track::fixed(40.0), Track::FILL],
            };
            rec.grid("hug-grid", Widget::grid(), tracks, |rec| {
                rec.text_leaf(
                    "label",
                    Widget::leaf().grid_cell((0, 1)),
                    TextRun::new(
                        "several words that fit on one line",
                        14.0,
                        TextWrap::WrapWithOverflow,
                    ),
                );
            });
        });
    });
    let shaped = rig.shaped_text("label").extent.size;
    let label = rig.rect("label");
    assert_eq!(
        label.size.w, shaped.w,
        "the column is the label's one-line width"
    );
    assert_eq!(rig.rect("hug-grid").size.w, 40.0 + shaped.w);
    assert_eq!(
        rig.rect("hug-grid").size.h,
        shaped.h,
        "the row holds the label it measured"
    );
    assert_eq!(label.size.h, shaped.h);
}

/// A hugging grid's Fill columns each take at least their cells' width, sized by CSS Grid's
/// flexible fraction for indefinite space (§12.7.1): `fr = max(content / weight)`, each column
/// `fr · weight`. Cells 200 and 20 wide in weights 1 and 1 make `fr = 200` and two 200-wide
/// columns, 400 in all; weights 1 and 3 with cells 200 and 300 make `fr = max(200, 100) = 200`,
/// 200 and 600. A cell spanning both columns of weights 1 and 1, 300 wide, makes `fr = 150`. A
/// column's `min` counts as its extent: a 250 floor beside a 20-wide cell makes `fr = 250`.
/// The grid's max-content width is the same.
#[test]
fn hug_grid_fill_columns_each_hold_their_cells() {
    /// A cell at column `col`, `span` columns wide, `w` wide itself.
    type Cell = (u16, u16, f32);
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
    let mut rig = LayoutRig::new();
    for (tracks, cells, want) in cases {
        let weights = tracks.map(|t| t.sizing().fill_weight());
        rig.run(Size::new(1200.0, 200.0), |rec| {
            rec.node("row", Widget::hstack(), |rec| {
                let grid_tracks = GridTracks {
                    rows: &[Track::fixed(20.0)],
                    cols: &tracks,
                };
                rec.grid("hug-grid", Widget::grid(), grid_tracks, |rec| {
                    for &(col, span, w) in cells {
                        rec.leaf(
                            &format!("cell{col}"),
                            Widget::leaf()
                                .grid_cell(
                                    GridCell::at(0, col)
                                        .with_span(NonZeroU16::MIN, NonZeroU16::new(span).unwrap()),
                                )
                                .size((w, 20.0)),
                        );
                    }
                });
            });
        });
        assert_eq!(
            rig.rect("hug-grid").size.w,
            want[0] + want[1],
            "{weights:?} {cells:?}"
        );
        assert_eq!(
            rig.intrinsic("hug-grid", Axis::X, LenReq::MaxContent),
            want[0] + want[1],
            "{weights:?} {cells:?}: the max-content width is the hugged one"
        );
        for &(col, span, w) in cells {
            let cell = rig.rect(&format!("cell{col}"));
            let left: f32 = want[..usize::from(col)].iter().sum();
            assert_eq!(cell.min.x, left, "{weights:?} cell {col}");
            let room: f32 = want[usize::from(col)..usize::from(col + span)].iter().sum();
            assert!(cell.size.w <= room, "{weights:?} cell {col}: {w} in {room}");
        }
    }
}

/// A Fill grid offered a finite width under a parent that hugs it reports what its Fill track
/// holds, so the parent hugs the cells it shaped. 39 mono glyphs at 10 px are 390 px: at the
/// 300 px Fill width the run wraps to "aaaa … ffff" (29 glyphs, 290 px) and "gggg hhhh", so the
/// grid, its parent and the cell are all 290 wide. Reporting no Fill extent let the parent hug
/// the 40 px floor, the widest word, and the text shaped for 300 overflowed it.
#[test]
fn a_fill_grid_under_a_hugging_parent_holds_the_cells_it_shaped() {
    let mut rig = LayoutRig::new();
    rig.run(Size::new(300.0, 200.0), |rec| {
        rec.node("popup", Widget::vstack(), |rec| {
            let tracks = GridTracks {
                rows: &[Track::HUG],
                cols: &[Track::FILL],
            };
            let grid = Widget::grid().size(SizeSpec::FILL_W);
            rec.grid("grid", grid, tracks, |rec| {
                rec.text_leaf(
                    "label",
                    Widget::leaf().grid_cell((0, 0)).size(SizeSpec::FILL_W),
                    TextRun::new(
                        "aaaa bbbb cccc dddd eeee ffff gggg hhhh",
                        20.0,
                        TextWrap::Wrap,
                    ),
                );
            });
        });
    });
    for id in ["popup", "grid", "label"] {
        assert_eq!(rig.rect(id).size.w, 290.0, "{id}");
    }
}

/// A Fill grid's height under a hugging parent follows its offer once the Fill rows' cap binds,
/// so the measure cache must not serve a taller offer's answer to a shorter one. Two Fill rows,
/// the first holding a 100 px cell: the shared fraction holds 100 a row, 200 in all. At 300 the
/// rows resolve to 150 each and hold the 200; at 150 they resolve to 100 and 50, and the grid
/// caps at 150.
#[test]
fn a_fill_grid_height_capped_by_a_shorter_offer_is_measured_again() {
    let record = |rec: &mut NodeRecorder<'_>| {
        rec.node("popup", Widget::vstack(), |rec| {
            let tracks = GridTracks {
                rows: &[Track::FILL, Track::FILL],
                cols: &[Track::fixed(40.0)],
            };
            let grid = Widget::grid().size(SizeSpec::FILL_H);
            rec.grid("grid", grid, tracks, |rec| {
                rec.leaf(
                    "tall",
                    Widget::leaf()
                        .grid_cell((0, 0))
                        .size((Sizing::fixed(40.0), Sizing::fixed(100.0))),
                );
            });
        });
    };
    let mut rig = LayoutRig::new();
    for (surface_h, want) in [(300.0, 200.0), (150.0, 150.0), (300.0, 200.0)] {
        rig.run(Size::new(100.0, surface_h), record);
        assert_eq!(rig.rect("popup").size.h, want, "offered {surface_h}");
    }
}
