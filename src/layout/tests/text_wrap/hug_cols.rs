//! Non-wrapping labels in hug columns, which must never shrink below their
//! full width.

use crate::internals::layout_rig::{GridTracks, LayoutRig, TextRun};
use crate::layout::intrinsic::len_req::LenReq;
use crate::layout::tests::support::PARAGRAPH;
use crate::primitives::geometry::size::Size;
use crate::primitives::layout::axis::Axis;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::layout::track::Track;
use crate::text::wrap::TextWrap;
use crate::widget_core::configure::Configure;
use crate::widget_core::widget::Widget;

const HUG_COLS: GridTracks<'static> = GridTracks {
    rows: &[Track::HUG],
    cols: &[Track::HUG, Track::HUG],
};

/// "right column" at 14 px, 12 × 7 = 84 wide, never wrapping.
const LABEL: TextRun<'static> = TextRun::new("right column", 14.0, TextWrap::SingleLine);

/// Repro for the showcase "text layouts" section: a Hug+Hug grid with a wrapping paragraph in col 0 and a non-wrapping label in col 1, under FILL panels. As the surface narrows, the grid must clamp at its intrinsic floor, never shrinking col 1 below the label's natural width.
#[test]
fn two_hug_cols_nonwrapping_label_floors_at_full_width() {
    const FILL: (Sizing, Sizing) = (Sizing::FILL, Sizing::FILL);
    let widths_at = |surface_w: f32| {
        let mut rig = LayoutRig::new();
        rig.run(Size::new(surface_w, 400.0), |rec| {
            rec.node("outer", Widget::vstack().padding(12.0).size(FILL), |rec| {
                rec.node("panel", Widget::zstack().padding(16.0).size(FILL), |rec| {
                    rec.node("body", Widget::vstack().size(FILL), |rec| {
                        let section = Widget::vstack().size((Sizing::FILL, Sizing::HUG)).gap(6.0);
                        rec.node("section", section, |rec| {
                            let title = TextRun::new(
                                "two Hug columns: paragraph wraps to fit, label stays natural",
                                12.0,
                                TextWrap::SingleLine,
                            );
                            rec.text_leaf("section-title", Widget::leaf(), title);
                            rec.grid("grid", Widget::grid(), HUG_COLS, |rec| {
                                let paragraph =
                                    TextRun::new(PARAGRAPH, 14.0, TextWrap::WrapWithOverflow);
                                rec.text_leaf(
                                    "paragraph",
                                    Widget::leaf().grid_cell((0, 0)),
                                    paragraph,
                                );
                                rec.text_leaf("label", Widget::leaf().grid_cell((0, 1)), LABEL);
                            });
                        });
                    });
                });
            });
        });
        [rig.rect("section").size.w, rig.rect("grid").size.w]
    };

    // Once the section panel stops shrinking (floored by its title text), the Hug grid inside must fill the section's committed cross extent, not the smaller surface-derived `available`.
    // At 500 the section fills 500 − 2·12 − 2·16 = 444, and the grid hugs its max-content, the 43-char paragraph and the label, 301 + 84 = 385. Below that the section floors at its single-line title, 60 chars of 6 px, 360, and the grid fills it.
    for (surface_w, section_w, grid_w) in [
        (500.0, 444.0, 385.0),
        (400.0, 360.0, 360.0),
        (300.0, 360.0, 360.0),
        (200.0, 360.0, 360.0),
        (150.0, 360.0, 360.0),
    ] {
        assert_eq!(
            widths_at(surface_w),
            [section_w, grid_w],
            "surface_w={surface_w}"
        );
    }
}

/// Pin: a non-wrapping text reports MinContent on X equal to its full width; wrapping text reports the longest word. A Hug+Hug grid must floor the label column there, or slack distribution shrinks it and the paint overflows its cell.
#[test]
fn nonwrapping_text_min_content_equals_full_width() {
    let mut rig = LayoutRig::new();
    rig.run(Size::new(400.0, 200.0), |rec| {
        rec.text_leaf("label", Widget::leaf(), LABEL);
    });
    let max_w = rig.intrinsic("label", Axis::X, LenReq::MaxContent);
    let min_w = rig.intrinsic("label", Axis::X, LenReq::MinContent);
    assert_eq!(
        [min_w, max_w],
        [84.0; 2],
        "non-wrapping MinContent must equal MaxContent (full width)"
    );
}

/// Pin: in a Hug+Hug grid too narrow for both max-content widths, the wrapping paragraph absorbs the squeeze and the label cell keeps its full 84 px; a MinContent of 0 would clip "right column" to "right col".
#[test]
fn two_hug_cols_label_cell_never_shrinks_below_label_full_width() {
    const LONG: &str =
        "the quick brown fox jumps over the lazy dog. pack my box with five dozen liquor jugs";
    // Between the grid's floor and the paragraph's max-content, slack distribution applies; the label cell still gets its full natural width.
    for (paragraph, surface_ws) in [
        (PARAGRAPH, [400.0, 300.0, 250.0, 200.0]),
        (LONG, [600.0, 500.0, 400.0, 300.0]),
    ] {
        for surface_w in surface_ws {
            let mut rig = LayoutRig::new();
            rig.run(Size::new(surface_w, 400.0), |rec| {
                let grid = Widget::grid().size((Sizing::FILL, Sizing::HUG));
                rec.grid("grid", grid, HUG_COLS, |rec| {
                    let run = TextRun::new(paragraph, 14.0, TextWrap::WrapWithOverflow);
                    rec.text_leaf("paragraph", Widget::leaf().grid_cell((0, 0)), run);
                    rec.text_leaf("label", Widget::leaf().grid_cell((0, 1)), LABEL);
                });
            });
            assert_eq!(
                rig.rect("label").size.w,
                84.0,
                "label cell shrank below the label's natural width — \
                 non-wrapping text would visually overflow its column. \
                 surface_w={surface_w}",
            );
        }
    }
}
