//! Demonstrates the v1 text z-order limitation: all text is prepared once
//! and rendered after every quad in the frame, so a quad declared *after*
//! a text run can't visually occlude that text. See `docs/text.md` open
//! questions and `src/renderer/backend/text.rs` for the architecture.
//!
//! Each demo here pairs the same content shaped two ways. If the bug were
//! fixed, the right-hand panel would show the red box covering the label;
//! today, the label "T-shirt" floats on top of the red box.

use palantir::{Color, Configure, Frame, Panel, Sizing, Stroke, Styled, Text, Ui};

pub fn build(ui: &mut Ui) {
    Panel::vstack()
        .size((Sizing::FILL, Sizing::FILL))
        .gap(20.0)
        .padding(16.0)
        .show(ui, |ui| {
            Text::with_id(("hdr", "title"), "Text z-order — current limitation")
                .size_px(14.0)
                .color(Color::rgb(0.78, 0.82, 0.90))
                .show(ui);

            Text::with_id(
                ("hdr", "sub"),
                concat!(
                    "All text renders after every quad in the frame. A child quad ",
                    "drawn AFTER a text label cannot occlude it; the label always ",
                    "wins z-order. Fixing this requires per-group prepare/render ",
                    "in the wgpu backend (see docs/text.md)."
                ),
            )
            .size_px(12.0)
            .color(Color::rgb(0.62, 0.68, 0.78))
            .wrapping()
            .show(ui);

            // Two side-by-side cells.
            Panel::hstack_with_id("z-row")
                .gap(20.0)
                .size((Sizing::FILL, Sizing::Fixed(220.0)))
                .show(ui, |ui| {
                    cell(
                        ui,
                        "no-quad-after",
                        "Text on top of quad — OK case",
                        Color::rgb(0.20, 0.45, 0.85),
                        false,
                    );
                    cell(
                        ui,
                        "quad-after-text",
                        "Red quad declared AFTER text — should cover it (bug: doesn't)",
                        Color::rgb(0.85, 0.30, 0.30),
                        true,
                    );
                });
        });
}

/// One demo cell. `quad_after`:
/// - `false` — text painted on a colored background. Always-on-top
///   behavior is correct here.
/// - `true` — same plus a red Frame *after* the text. The red Frame
///   should occlude the label, but text-renders-last makes it float.
fn cell(ui: &mut Ui, id: &'static str, caption: &'static str, accent: Color, quad_after: bool) {
    Panel::vstack_with_id(("cell", id))
        .size((Sizing::FILL, Sizing::FILL))
        .gap(8.0)
        .show(ui, |ui| {
            // Caption above the demo box.
            Text::with_id(("caption", id), caption)
                .size_px(11.0)
                .color(Color::rgb(0.70, 0.74, 0.82))
                .wrapping()
                .show(ui);

            // The demo: ZStack of background + label + (maybe) occluder.
            Panel::zstack_with_id(("box", id))
                .size((Sizing::FILL, Sizing::FILL))
                .fill(Color::rgb(0.12, 0.14, 0.18))
                .stroke(Stroke {
                    width: 1.0,
                    color: Color::rgb(0.30, 0.34, 0.42),
                })
                .radius(6.0)
                .padding(12.0)
                .show(ui, |ui| {
                    // Background panel with accent fill.
                    Frame::with_id(("bg", id))
                        .size((Sizing::FILL, Sizing::FILL))
                        .fill(accent)
                        .radius(4.0)
                        .show(ui);

                    // The label — should be visually below the occluder when
                    // `quad_after` is true.
                    Text::with_id(("label", id), "T-shirt")
                        .size_px(28.0)
                        .color(Color::WHITE)
                        .show(ui);

                    if quad_after {
                        // Occluder declared AFTER the text. Smaller than the
                        // ZStack but big enough to cover the label.
                        Frame::with_id(("occluder", id))
                            .size((Sizing::Fixed(180.0), Sizing::Fixed(80.0)))
                            .fill(Color::rgb(0.10, 0.10, 0.10))
                            .stroke(Stroke {
                                width: 1.0,
                                color: Color::rgb(0.45, 0.45, 0.50),
                            })
                            .radius(4.0)
                            .show(ui);
                    }
                });
        });
}
