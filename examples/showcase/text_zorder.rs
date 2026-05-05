//! Demonstrates per-group text z-ordering in the wgpu backend. Both
//! cells show that paint order is honored — text on top of an earlier
//! quad reads as a label on a background; a later quad correctly
//! occludes a prior text label. See `src/renderer/composer/mod.rs`
//! (group split on text→quad transition) and
//! `src/renderer/backend/text.rs` (per-group prepare/render pool).

use palantir::{
    Background, Color, Configure, Corners, Frame, Panel, Sizing, Stroke, Text, TextStyle, Ui,
};

pub fn build(ui: &mut Ui) {
    Panel::vstack()
        .size((Sizing::FILL, Sizing::FILL))
        .gap(20.0)
        .padding(16.0)
        .show(ui, |ui| {
            Text::new("Z-order — paint order honored across quads + text")
                .with_id(("hdr", "title"))
                .style(
                    TextStyle::default()
                        .with_font_size(14.0)
                        .with_color(Color::rgb(0.78, 0.82, 0.90)),
                )
                .show(ui);

            Text::new(concat!(
                "Composer splits draw groups on every text→quad transition; ",
                "the wgpu backend keeps a pool of glyphon TextRenderers (one ",
                "per group with text) so quads and text interleave per group ",
                "in the encoder pass."
            ))
            .with_id(("hdr", "sub"))
            .style(
                TextStyle::default()
                    .with_font_size(12.0)
                    .with_color(Color::rgb(0.62, 0.68, 0.78)),
            )
            .wrapping()
            .show(ui);

            // Two side-by-side cells.
            Panel::hstack()
                .with_id("z-row")
                .gap(20.0)
                .size((Sizing::FILL, Sizing::Fixed(220.0)))
                .show(ui, |ui| {
                    cell(
                        ui,
                        "label-on-bg",
                        "Text on top of background quad",
                        Color::rgb(0.20, 0.45, 0.85),
                        false,
                    );
                    cell(
                        ui,
                        "occluder-after-label",
                        "Black quad declared AFTER text — correctly covers it",
                        Color::rgb(0.85, 0.30, 0.30),
                        true,
                    );
                });
        });
}

/// One demo cell. `quad_after`:
/// - `false` — label painted on a colored background. Label is on top
///   (paint order: bg, label).
/// - `true` — same plus a black Frame *after* the text. Paint order is
///   (bg, label, occluder); the occluder correctly covers the label.
fn cell(ui: &mut Ui, id: &'static str, caption: &'static str, accent: Color, quad_after: bool) {
    Panel::vstack()
        .with_id(("cell", id))
        .size((Sizing::FILL, Sizing::FILL))
        .gap(8.0)
        .show(ui, |ui| {
            // Caption above the demo box.
            Text::new(caption)
                .with_id(("caption", id))
                .style(
                    TextStyle::default()
                        .with_font_size(11.0)
                        .with_color(Color::rgb(0.70, 0.74, 0.82)),
                )
                .wrapping()
                .show(ui);

            // The demo: ZStack of background + label + (maybe) occluder.
            Panel::zstack()
                .with_id(("box", id))
                .size((Sizing::FILL, Sizing::FILL))
                .background(Background {
                    fill: Color::rgb(0.12, 0.14, 0.18),
                    stroke: Some(Stroke {
                        width: 1.0,
                        color: Color::rgb(0.30, 0.34, 0.42),
                    }),
                    radius: Corners::all(6.0),
                })
                .padding(12.0)
                .show(ui, |ui| {
                    // Background panel with accent fill.
                    Frame::new()
                        .with_id(("bg", id))
                        .size((Sizing::FILL, Sizing::FILL))
                        .background(Background {
                            fill: accent,
                            radius: Corners::all(4.0),
                            ..Default::default()
                        })
                        .show(ui);

                    // Label — visible on top of the background. When
                    // `quad_after` is true, the occluder declared next
                    // covers it.
                    Text::new("T-shirt")
                        .with_id(("label", id))
                        .style(
                            TextStyle::default()
                                .with_font_size(28.0)
                                .with_color(Color::WHITE),
                        )
                        .show(ui);

                    if quad_after {
                        // Occluder declared AFTER the text. Smaller than
                        // the ZStack but big enough to cover the label.
                        Frame::new()
                            .with_id(("occluder", id))
                            .size((Sizing::Fixed(180.0), Sizing::Fixed(80.0)))
                            .background(Background {
                                fill: Color::rgb(0.10, 0.10, 0.10),
                                stroke: Some(Stroke {
                                    width: 1.0,
                                    color: Color::rgb(0.45, 0.45, 0.50),
                                }),
                                radius: Corners::all(4.0),
                            })
                            .show(ui);
                    }
                });
        });
}
