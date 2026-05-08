//! WrapHStack/WrapVStack showcase. Demonstrates flow-then-wrap
//! behavior: children stream along the main axis, then start a new
//! line on the cross axis when the next child wouldn't fit. The two
//! gap dimensions are independent: `.gap(g)` is within-line spacing,
//! `.line_gap(g)` is between-line spacing.

use crate::swatch;
use palantir::{
    Background, Configure, Corners, Frame, Justify, Panel, Sizing, Stroke, Text, TextStyle, Ui,
};

pub fn build(ui: &mut Ui) {
    Panel::vstack()
        .auto_id()
        .size((Sizing::FILL, Sizing::FILL))
        .gap(20.0)
        .padding(16.0)
        .show(ui, |ui| {
            Text::new("WrapHStack / WrapVStack")
                .id_salt(("hdr", "title"))
                .style(TextStyle::default().with_font_size(14.0))
                .show(ui);

            Text::new(concat!(
                "Children flow along main axis; when the next child wouldn't fit, ",
                "wrap to a new line. `.gap` spaces siblings within a line; ",
                "`.line_gap` spaces lines. `.justify(...).auto_id()` applies per-line.",
            ))
            .id_salt(("hdr", "sub"))
            .style(TextStyle::default().with_font_size(12.0))
            .wrapping()
            .show(ui);

            // Tag-cloud style — many small chips wrapping in a fixed width.
            section(
                ui,
                "tag-cloud",
                "WrapHStack: tag cloud (Justify::Start)",
                |ui| {
                    Panel::wrap_hstack()
                        .id_salt("tags")
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
                    Panel::wrap_hstack()
                        .id_salt("centered-row")
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
                    Panel::wrap_vstack()
                        .id_salt("vwrap-col")
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

/// Pill-shaped tag chip — the "chip" look IS the demo aesthetic, so a
/// bg + stroke is needed to make it read as a chip rather than bare
/// text. Uses a translucent accent so chips harmonize with the palette.
fn chip<H: std::hash::Hash>(ui: &mut Ui, key: H, label: &'static str) {
    Panel::hstack()
        .id_salt(("chip-row", &key))
        .padding((10.0, 4.0))
        .background(Background {
            fill: palantir::Color::linear_rgba(swatch::A.r, swatch::A.g, swatch::A.b, 0.20),
            stroke: Some(Stroke {
                width: 1.0,
                color: palantir::Color::linear_rgba(swatch::A.r, swatch::A.g, swatch::A.b, 0.45),
            }),
            radius: Corners::all(10.0),
        })
        .show(ui, |ui| {
            Text::new(label)
                .id_salt(("chip-label", &key))
                .style(TextStyle::default().with_font_size(12.0))
                .show(ui);
        });
}

fn badge<H: std::hash::Hash>(ui: &mut Ui, key: H) {
    Frame::new()
        .id_salt(("badge", &key))
        .size((Sizing::Fixed(80.0), Sizing::Fixed(28.0)))
        .background(Background {
            fill: swatch::A,
            radius: Corners::all(4.0),
            ..Default::default()
        })
        .show(ui);
}

/// Plain section: title + body, no card decoration.
fn section(ui: &mut Ui, id: &'static str, title: &'static str, body: impl FnOnce(&mut Ui)) {
    Panel::vstack()
        .id_salt(id)
        .size((Sizing::FILL, Sizing::Hug))
        .gap(6.0)
        .show(ui, |ui| {
            Text::new(title)
                .id_salt(("section-title", id))
                .style(TextStyle::default().with_font_size(12.0))
                .show(ui);
            body(ui);
        });
}
