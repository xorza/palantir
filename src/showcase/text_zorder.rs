//! Demonstrates per-group text z-ordering in the wgpu backend. Both
//! cells show that paint order is honored — text on top of an earlier
//! quad reads as a label on a background; a later quad correctly
//! occludes a prior text label. See `src/renderer/composer/mod.rs`
//! (group split on text→quad transition) and
//! `src/renderer/backend/text.rs` (per-group prepare/render pool).

use super::swatch::{caption_style, swatch_bg};
use crate::showcase::swatch;
use palantir::{Color, Configure, Frame, Panel, Sizing, Text, TextStyle, UiCore};

pub fn build(ui: &mut UiCore) {
    Panel::vstack()
        .auto_id()
        .size((Sizing::FILL, Sizing::FILL))
        .gap(20.0)
        .padding(16.0)
        .show(ui, |ui| {
            Text::new("Z-order — paint order honored across quads + text")
                .id_salt(("hdr", "title"))
                .style(TextStyle::default().with_font_size(14.0))
                .show(ui);

            Text::new(concat!(
                "Composer splits draw groups on every text→quad transition; ",
                "the wgpu backend keeps a pool of glyphon TextRenderers (one ",
                "per group with text).auto_id() so quads and text interleave per group ",
                "in the encoder pass."
            ))
            .id_salt(("hdr", "sub"))
            .style(caption_style())
            .wrapping()
            .show(ui);

            // Two side-by-side cells.
            Panel::hstack()
                .id_salt("z-row")
                .gap(20.0)
                .size((Sizing::FILL, Sizing::Fixed(220.0)))
                .show(ui, |ui| {
                    cell(
                        ui,
                        "label-on-bg",
                        "Text on top of background quad",
                        swatch::A,
                        false,
                    );
                    cell(
                        ui,
                        "occluder-after-label",
                        "Black quad declared AFTER text — correctly covers it",
                        swatch::B,
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
fn cell(ui: &mut UiCore, id: &'static str, caption: &'static str, accent: Color, quad_after: bool) {
    Panel::vstack()
        .id_salt(("cell", id))
        .size((Sizing::FILL, Sizing::FILL))
        .gap(8.0)
        .show(ui, |ui| {
            Text::new(caption)
                .id_salt(("caption", id))
                .style(TextStyle::default().with_font_size(11.0))
                .wrapping()
                .show(ui);

            // The demo: ZStack of background + label + (maybe) occluder.
            Panel::zstack()
                .id_salt(("box", id))
                .size((Sizing::FILL, Sizing::FILL))
                .padding(12.0)
                .show(ui, |ui| {
                    // Background panel with accent fill — required to demo
                    // "label paints on top of an earlier quad".
                    Frame::new()
                        .id_salt(("bg", id))
                        .size((Sizing::FILL, Sizing::FILL))
                        .background(swatch_bg(accent))
                        .show(ui);

                    // Label — visible on top of the background. When
                    // `quad_after` is true, the occluder declared next
                    // covers it.
                    Text::new("T-shirt")
                        .id_salt(("label", id))
                        .style(
                            TextStyle::default()
                                .with_font_size(28.0)
                                .with_color(Color::hex(0x1a1a1a)),
                        )
                        .show(ui);

                    if quad_after {
                        // Occluder declared AFTER the text. Smaller than
                        // the ZStack but big enough to cover the label.
                        Frame::new()
                            .id_salt(("occluder", id))
                            .size((Sizing::Fixed(180.0), Sizing::Fixed(80.0)))
                            .background(swatch_bg(Color::hex(0x1a1a1a)))
                            .show(ui);
                    }
                });
        });
}
