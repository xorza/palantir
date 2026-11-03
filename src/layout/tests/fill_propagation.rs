//! Pin: child-positioner layouts (ZStack, Canvas) and Hug-axis propagation must not pass `INFINITY` when the
//! parent has a finite slot, or a nested grid falls back to max-content and breaks wrapping.

use crate::internals::layout_rig::{GridTracks, LayoutRig, NodeRecorder, TextRun};
use crate::layout::tests::support::{PARAGRAPH, two_hug_cols_with_wrap};
use crate::primitives::geometry::size::Size;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::layout::track::Track;
use crate::text::wrap::TextWrap;
use crate::widget_core::configure::Configure;
use crate::widget_core::widget::Widget;

/// The paragraph wrapped at a 200 px surface: of 200, its Hug column takes `40 + (200 − 136) =
/// 104`, 13 chars of 8: four 16 px lines, the widest "the lazy dog" at 96. Max-content would
/// shape one line.
const WRAPPED_AT_200: Size = Size::new(96.0, 4.0 * 16.0);

/// A Hug grid of `[Hug, Fill]` columns over `rows`: a 14 px single-line label and a wrapping
/// value per row, the value under the id `value{row}`.
fn label_value_rows(rec: &mut NodeRecorder<'_>, grid: Widget, rows: &[(&str, &str)]) {
    let row_tracks = vec![Track::HUG; rows.len()];
    let tracks = GridTracks {
        rows: &row_tracks,
        cols: &[Track::HUG, Track::FILL],
    };
    rec.grid("grid", grid, tracks, |rec| {
        for (row, &(label, value)) in (0u16..).zip(rows) {
            let label = TextRun::new(label, 14.0, TextWrap::SingleLine);
            rec.text_leaf(
                &format!("label{row}"),
                Widget::leaf().grid_cell((row, 0)),
                label,
            );
            let value = TextRun::new(value, 14.0, TextWrap::WrapWithOverflow);
            rec.text_leaf(
                &format!("value{row}"),
                Widget::leaf().grid_cell((row, 1)),
                value,
            );
        }
    });
}

/// A constrained ZStack or Canvas, each a child-positioner layout, must pass its inner size to
/// children, not `INFINITY`, or a nested grid falls back to max-content.
#[test]
fn fill_positioners_pass_finite_avail_so_nested_grid_constrains() {
    for (label, positioner) in [("zstack", Widget::zstack()), ("canvas", Widget::canvas())] {
        let mut rig = LayoutRig::new();
        rig.run(Size::new(200.0, 400.0), |rec| {
            let outer = positioner.size((Sizing::FILL, Sizing::FILL));
            rec.node("positioner", outer, |rec| {
                two_hug_cols_with_wrap(rec, PARAGRAPH);
            });
        });
        assert_eq!(
            rig.shaped_text("wrapping").extent.size,
            WRAPPED_AT_200,
            "{label}"
        );
    }
}

/// A `Hug` ZStack containing a `Fill` child must not recursively size to it; Hug axes keep `INFINITY`.
#[test]
fn hug_zstack_does_not_recursively_size_to_fill_child() {
    let mut rig = LayoutRig::new();
    rig.run(Size::new(800.0, 600.0), |rec| {
        rec.node("row", Widget::hstack(), |rec| {
            rec.node("hug-z", Widget::zstack(), |rec| {
                rec.leaf(
                    "fill-child",
                    Widget::leaf().size((Sizing::FILL, Sizing::FILL)),
                );
                rec.leaf("fixed-child", Widget::leaf().size((60.0, 40.0)));
            });
        });
    });
    assert_eq!(rig.rect("hug-z").size, Size::new(60.0, 40.0));
}

/// A `Hug` grid's Fill column collapses to 0 at arrange; its cells get `INFINITY` width so text stays single-line
/// and row heights don't grow on horizontal resize.
#[test]
fn hug_grid_fill_col_does_not_grow_row_height_on_horizontal_resize() {
    let value_h = |surface_w: f32| {
        let mut rig = LayoutRig::new();
        rig.run(Size::new(surface_w, 400.0), |rec| {
            label_value_rows(rec, Widget::grid(), &[("Label:", PARAGRAPH)]);
        });
        rig.shaped_text("value0").extent.size.h
    };
    assert_eq!([value_h(2000.0), value_h(200.0)], [14.0; 2]);
}

/// A `Fill` grid with a `Fill` column wraps text in it: measure and arrange agree on its width.
/// "Label:" takes 6 × 7 = 42 of 200, and the 158 left hold 22 chars: "the quick brown fox",
/// "jumps over the lazy", "dog", the widest 19 × 7 = 133.
#[test]
fn fill_grid_fill_col_wraps_text_under_constrained_width() {
    let mut rig = LayoutRig::new();
    rig.run(Size::new(200.0, 400.0), |rec| {
        rec.node("column", Widget::vstack(), |rec| {
            let grid = Widget::grid().size((Sizing::FILL, Sizing::HUG));
            label_value_rows(rec, grid, &[("Label:", PARAGRAPH)]);
        });
    });
    assert_eq!(
        rig.shaped_text("value0").extent.size,
        Size::new(133.0, 3.0 * 14.0),
        "Fill grid + Fill col should wrap text to three lines under constrained width",
    );
}

/// A VStack section containing a `(Fill, Hug)` Grid with a wrapping Fill column sizes to the
/// wrapped row heights. "Title:" takes 42 of 400, and the 358 left hold 51 chars: the long
/// value wraps to three lines and the tags fit one.
#[test]
fn vstack_section_with_hug_grid_and_fill_col_wrap_does_not_collapse() {
    let mut rig = LayoutRig::new();
    rig.run(Size::new(400.0, 600.0), |rec| {
        let section = Widget::vstack().size((Sizing::FILL, Sizing::HUG));
        rec.node("section", section, |rec| {
            let grid = Widget::grid().size((Sizing::FILL, Sizing::HUG));
            let rows = [
                (
                    "Title:",
                    "the quick brown fox jumps over the lazy dog \
                     pack my box with five dozen liquor jugs how \
                     vexingly quick daft zebras jump",
                ),
                ("Tags:", "layout, grid, intrinsic, wrapping, css"),
            ];
            label_value_rows(rec, grid, &rows);
        });
    });
    // Single-line rows would total two lines; the wrapped rows hold four between them.
    assert_eq!(
        rig.rect("grid").size.h,
        4.0 * 14.0,
        "grid must size to wrapped row heights, not single-line × 2",
    );
}

/// A Hug-axis ZStack containing a Hug Grid with wrapping Fill-column cells lets the grid measure
/// under the constrained cross axis: of 400, "Label:" takes 42 and the 358 left hold 51 chars,
/// two lines of the 83-char value.
#[test]
fn hug_zstack_with_nested_grid_wrap_does_not_collapse() {
    let mut rig = LayoutRig::new();
    rig.run(Size::new(400.0, 600.0), |rec| {
        let column = Widget::vstack().size((Sizing::fixed(400.0), Sizing::HUG));
        rec.node("column", column, |rec| {
            let hug_z = Widget::zstack().size((Sizing::FILL, Sizing::HUG));
            rec.node("hug-z", hug_z, |rec| {
                let grid = Widget::grid().size((Sizing::FILL, Sizing::HUG));
                let value = "the quick brown fox jumps over the lazy dog \
                             pack my box with five dozen liquor jugs";
                label_value_rows(rec, grid, &[("Label:", value)]);
            });
        });
    });
    assert_eq!(
        rig.rect("grid").size.h,
        2.0 * 14.0,
        "ZStack must pass `INF` on Hug axes so nested grid measures \
         under the constrained cross and wraps to two lines",
    );
}
