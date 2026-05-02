//! WrapHStack/WrapVStack showcase. Demonstrates flow-then-wrap
//! behavior: children stream along the main axis, then start a new
//! line on the cross axis when the next child wouldn't fit. The two
//! gap dimensions are independent: `.gap(g)` is within-line spacing,
//! `.line_gap(g)` is between-line spacing.

use palantir::{Color, Configure, Frame, Justify, Panel, Sizing, Stroke, Styled, Text, Ui};

pub fn build(ui: &mut Ui) {
    Panel::vstack()
        .size((Sizing::FILL, Sizing::FILL))
        .gap(20.0)
        .padding(16.0)
        .show(ui, |ui| {
            Text::with_id(("hdr", "title"), "WrapHStack / WrapVStack")
                .size_px(14.0)
                .color(Color::rgb(0.78, 0.82, 0.90))
                .show(ui);

            Text::with_id(
                ("hdr", "sub"),
                concat!(
                    "Children flow along main axis; when the next child wouldn't fit, ",
                    "wrap to a new line. `.gap` spaces siblings within a line; ",
                    "`.line_gap` spaces lines. `.justify(...)` applies per-line.",
                ),
            )
            .size_px(12.0)
            .color(Color::rgb(0.62, 0.68, 0.78))
            .wrapping()
            .show(ui);

            // Tag-cloud style — many small chips wrapping in a fixed width.
            section(
                ui,
                "tag-cloud",
                "WrapHStack: tag cloud (Justify::Start)",
                |ui| {
                    Panel::wrap_hstack_with_id("tags")
                        .size((Sizing::FILL, Sizing::Hug))
                        .gap(8.0)
                        .line_gap(8.0)
                        .show(ui, |ui| {
                            for (i, t) in TAGS.iter().enumerate() {
                                chip(ui, ("tag", i), t);
                            }
                        });
                },
            );

            // Per-line center justify.
            section(
                ui,
                "centered",
                "WrapHStack: per-line Justify::Center, equal-size badges",
                |ui| {
                    Panel::wrap_hstack_with_id("centered-row")
                        .size((Sizing::FILL, Sizing::Hug))
                        .gap(10.0)
                        .line_gap(10.0)
                        .justify(Justify::Center)
                        .show(ui, |ui| {
                            for i in 0..7 {
                                badge(ui, ("badge", i));
                            }
                        });
                },
            );

            // VStack variant: vertical column flow that overflows to the right.
            section(
                ui,
                "vwrap",
                "WrapVStack: vertical flow, wraps to new column",
                |ui| {
                    Panel::wrap_vstack_with_id("vwrap-col")
                        .size((Sizing::Hug, Sizing::Fixed(160.0)))
                        .gap(6.0)
                        .line_gap(12.0)
                        .show(ui, |ui| {
                            for i in 0..10 {
                                badge(ui, ("v", i));
                            }
                        });
                },
            );
        });
}

const TAGS: &[&str] = &[
    "rust",
    "wgpu",
    "layout",
    "intrinsic",
    "WrapHStack",
    "flexbox-ish",
    "no-grid",
    "tags",
    "demo",
    "hug",
    "fill",
    "fixed",
    "padding",
    "margin",
    "z-order",
    "sdf",
    "rounded",
    "stroke",
    "alpha",
    "linear",
];

fn chip<H: std::hash::Hash>(ui: &mut Ui, key: H, label: &'static str) {
    use palantir::WidgetId;
    let id = WidgetId::from_hash(key);
    Panel::hstack_with_id(("chip-row", id))
        .padding((10.0, 4.0))
        .fill(Color::rgb(0.22, 0.30, 0.45))
        .stroke(Stroke {
            width: 1.0,
            color: Color::rgb(0.34, 0.42, 0.58),
        })
        .radius(10.0)
        .show(ui, |ui| {
            Text::with_id(("chip-label", id), label)
                .size_px(12.0)
                .color(Color::rgb(0.86, 0.90, 0.98))
                .show(ui);
        });
}

fn badge<H: std::hash::Hash>(ui: &mut Ui, key: H) {
    use palantir::WidgetId;
    let id = WidgetId::from_hash(key);
    Frame::with_id(("badge", id))
        .size((Sizing::Fixed(80.0), Sizing::Fixed(28.0)))
        .fill(Color::rgb(0.22, 0.46, 0.84))
        .radius(4.0)
        .show(ui);
}

fn section(ui: &mut Ui, id: &'static str, title: &'static str, body: impl FnOnce(&mut Ui)) {
    Panel::vstack_with_id(id)
        .size((Sizing::FILL, Sizing::Hug))
        .gap(6.0)
        .padding(8.0)
        .fill(Color::rgb(0.16, 0.18, 0.22))
        .stroke(Stroke {
            width: 1.0,
            color: Color::rgb(0.30, 0.34, 0.42),
        })
        .radius(4.0)
        .show(ui, |ui| {
            Text::with_id(("section-title", id), title)
                .size_px(12.0)
                .color(Color::rgb(0.70, 0.74, 0.82))
                .show(ui);
            body(ui);
        });
}
