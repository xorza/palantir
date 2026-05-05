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
//! This test sweeps a range of widths that includes the trigger and
//! asserts `end_frame()` doesn't panic. Pre-fix this panicked at
//! several widths in the swept range; post-fix the second-pass result
//! is clamped to `new_available` and rendering proceeds.

use crate::layout::types::sizing::Sizing;
use crate::support::testing::ui_with_text;
use crate::tree::element::Configure;
use crate::widgets::button::Button;
use crate::widgets::frame::Frame;
use crate::widgets::panel::Panel;
use glam::UVec2;

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
    for w in (480u32..=900).step_by(1) {
        let mut ui = ui_with_text(UVec2::new(w, 600));
        Panel::vstack()
            .padding(12.0)
            .gap(12.0)
            .size((Sizing::FILL, Sizing::FILL))
            .show(&mut ui, |ui| {
                // Toolbar — wrap_hstack of buttons. With theme padding
                // each button is `label + 24` wide; total `> w` so the
                // wrap_hstack reflows to multiple rows. Different widths
                // produce different row counts (non-monotonic
                // height-vs-width).
                Panel::wrap_hstack()
                    .gap(6.0)
                    .line_gap(6.0)
                    .size((Sizing::FILL, Sizing::Hug))
                    .show(ui, |ui| {
                        for label in LABELS {
                            Button::new().with_id(*label).label(*label).show(ui);
                        }
                    });

                // Central panel — zstack containing the panels-showcase
                // structure (4 FILL cells, padded, with varying inner
                // hug widths) that compounds into the second-pass
                // overshoot when the toolbar consumed more height than
                // expected.
                Panel::zstack()
                    .size((Sizing::FILL, Sizing::FILL))
                    .padding(16.0)
                    .show(ui, |ui| {
                        Panel::hstack()
                            .gap(12.0)
                            .size((Sizing::FILL, Sizing::FILL))
                            .show(ui, |ui| {
                                for (id, content_w) in
                                    [("c1", 132.0), ("c2", 60.0), ("c3", 80.0), ("c4", 100.0)]
                                {
                                    Panel::vstack()
                                        .with_id(id)
                                        .size((Sizing::FILL, Sizing::FILL))
                                        .padding(12.0)
                                        .show(ui, |ui| {
                                            Frame::new()
                                                .with_id((id, "swatch"))
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
        ui.end_frame();
    }
}
