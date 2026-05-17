//! Regression: `LayoutEngine::measure`'s second-pass convergence path
//! used to assert `final_desired <= new_available`. That assumption
//! breaks when a descendant subtree contains non-monotonic measure —
//! e.g. a `wrap_hstack` whose row-pack changes shape under different
//! available widths, combined with sibling `Fill` cells that hug to
//! padded content. Specific trigger from the showcase: a vstack root
//! with a 18-button toolbar `wrap_hstack` plus a central zstack
//! holding `panels::build`'s 4-cell hstack. At certain window widths
//! the second-pass measure produces a desired ~10 px wider than the
//! grown `new_available`, which used to panic.
//!
//! Sweeps a width range and asserts the frame doesn't panic.

use crate::Ui;
use crate::forest::element::Configure;
use crate::forest::Layer;
use crate::forest::tree::{NodeId};
use crate::layout::types::sizing::Sizing;
use crate::widgets::button::Button;
use crate::widgets::frame::Frame;
use crate::widgets::panel::Panel;
use glam::UVec2;

/// Z-order showcase repro: two FILL/FILL cells side-by-side in an
/// HStack. The right cell has a Fixed(180×80) descendant; the left
/// cell has no rigid descendants (its only child is FILL/FILL +
/// Text). When the window is too narrow for both cells to fit at the
/// right cell's min-content floor (204 = 180 + 24 padding), the right
/// cell overflows the HStack — its arranged rect extends past the
/// HStack's right edge.
///
/// Under correct flex-shrink semantics with min-content awareness,
/// FILL siblings should split available proportionally to *shrink
/// budget* (`available - intrinsic_min`), not weight alone — so the
/// left cell (with shrink budget = full FILL share) absorbs the
/// squeeze before the right cell (with no shrink budget below 204).
///
/// This pin asserts: at any window width where the HStack's
/// available is >= sum of children's intrinsic_min, no child rect
/// extends past the HStack's right edge.
#[test]
fn fill_siblings_with_unequal_min_content_do_not_overflow_parent() {
    for outer_w in (260u32..=600).step_by(10) {
        let mut ui = crate::Ui::for_test();
        let mut left_node = None;
        let mut right_node = None;
        let mut row_node = NodeId(0);
        ui.run_at(UVec2::new(outer_w, 400), |ui| {
            row_node = Panel::hstack()
                .auto_id()
                .gap(12.0)
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    // Left: FILL/FILL with a FILL/FILL child (no rigid
                    // descendant). intrinsic_min ≈ 0 — fully shrinkable.
                    left_node = Some(
                        Panel::vstack()
                            .id_salt("left")
                            .size((Sizing::FILL, Sizing::FILL))
                            .padding(12.0)
                            .show(ui, |ui| {
                                Frame::new()
                                    .id_salt("left-bg")
                                    .size((Sizing::FILL, Sizing::FILL))
                                    .show(ui);
                            })
                            .node(ui),
                    );
                    // Right: FILL/FILL with a Fixed(180×80) descendant.
                    // intrinsic_min = 180 + 24 padding = 204 — rigid below.
                    right_node = Some(
                        Panel::vstack()
                            .id_salt("right")
                            .size((Sizing::FILL, Sizing::FILL))
                            .padding(12.0)
                            .show(ui, |ui| {
                                Panel::zstack()
                                    .id_salt("right-z")
                                    .size((Sizing::FILL, Sizing::FILL))
                                    .show(ui, |ui| {
                                        Frame::new()
                                            .id_salt("right-bg")
                                            .size((Sizing::FILL, Sizing::FILL))
                                            .show(ui);
                                        Frame::new()
                                            .id_salt("right-fixed")
                                            .size((Sizing::Fixed(180.0), Sizing::Fixed(80.0)))
                                            .show(ui);
                                    });
                            })
                            .node(ui),
                    );
                })
                .node(ui);
        });

        let row = ui.layout[Layer::Main].rect[row_node.index()];
        let left = ui.layout[Layer::Main].rect[left_node.unwrap().index()];
        let right = ui.layout[Layer::Main].rect[right_node.unwrap().index()];

        // The right cell's intrinsic_min along X is the Fixed
        // descendant's 180 + the cell's 24 padding = 204. When the
        // HStack has enough room for that floor (outer_w >= 204 + 12
        // gap + something for the left cell), FILL distribution
        // should give the right cell at least 204 — letting the left
        // cell absorb the squeeze instead. This is CSS Flexbox's
        // default "items at min-content stop shrinking, others
        // continue."
        if outer_w >= 260 {
            assert!(
                right.size.w >= 204.0 - 0.5,
                "outer_w={outer_w}: right cell shrunk below its 204 min-content floor; \
                 left.w={} right.w={}",
                left.size.w,
                right.size.w,
            );
        }
        // And in all cases the row's children should be contained: no
        // sibling reaches past the HStack's right edge.
        let row_right_edge = row.min.x + row.size.w;
        let right_right_edge = right.min.x + right.size.w;
        assert!(
            right_right_edge <= row_right_edge + 0.5,
            "outer_w={outer_w}: right cell overflows HStack",
        );
    }
}

#[test]
fn second_pass_grow_then_overshoot_does_not_panic() {
    const LABELS: &[&str] = &[
        "text",
        "text layouts",
        "text edit",
        "z-order",
        "panels",
        "scroll",
        "wrap",
        "grid",
        "sizing",
        "alignment",
        "justify",
        "clip",
        "transform",
        "visibility",
        "disabled",
        "gap",
        "spacing",
        "buttons",
    ];
    // Sweep widths around the trigger zone (~620–700 wide on the live
    // showcase) plus a wider band so a future regression in either
    // direction shows up. Step 1 px to guarantee we hit whatever
    // discrete width tips the toolbar's wrap count past a threshold.
    //
    // Reuse one `Ui` across sweep — recreating it would re-load fonts (~120 ms each).
    let mut ui = Ui::for_test();
    for w in (480u32..=900).step_by(1) {
        ui.run_at(UVec2::new(w, 600), |ui| {
            Panel::vstack()
                .auto_id()
                .padding(12.0)
                .gap(12.0)
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    // Toolbar — wrap_hstack of buttons. With theme padding
                    // each button is `label + 24` wide; total `> w` so the
                    // wrap_hstack reflows to multiple rows. Different widths
                    // produce different row counts (non-monotonic
                    // height-vs-width).
                    Panel::wrap_hstack()
                        .auto_id()
                        .gap(6.0)
                        .line_gap(6.0)
                        .size((Sizing::FILL, Sizing::Hug))
                        .show(ui, |ui| {
                            for label in LABELS {
                                Button::new().id_salt(*label).label(*label).show(ui);
                            }
                        });

                    // Central panel — zstack containing the panels-showcase
                    // structure (4 FILL cells, padded, with varying inner
                    // hug widths) that compounds into the second-pass
                    // overshoot when the toolbar consumed more height than
                    // expected.
                    Panel::zstack()
                        .auto_id()
                        .size((Sizing::FILL, Sizing::FILL))
                        .padding(16.0)
                        .show(ui, |ui| {
                            Panel::hstack()
                                .auto_id()
                                .gap(12.0)
                                .size((Sizing::FILL, Sizing::FILL))
                                .show(ui, |ui| {
                                    for (id, content_w) in
                                        [("c1", 132.0), ("c2", 60.0), ("c3", 80.0), ("c4", 100.0)]
                                    {
                                        Panel::vstack()
                                            .id_salt(id)
                                            .size((Sizing::FILL, Sizing::FILL))
                                            .padding(12.0)
                                            .show(ui, |ui| {
                                                Frame::new()
                                                    .id_salt((id, "swatch"))
                                                    .size((
                                                        Sizing::Fixed(content_w),
                                                        Sizing::Fixed(40.0),
                                                    ))
                                                    .show(ui);
                                            });
                                    }
                                });
                        });
                });
        });
    }
}
