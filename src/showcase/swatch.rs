//! Shared scaffolding for showcase demos: the four-color "layout
//! swatch" palette plus a handful of helpers (backgrounds, sections,
//! text styles) that every demo would otherwise re-implement.
//!
//! The palette is demo content — colored rectangles that visualize
//! where layout puts each child — picked from the Ayu syntax-color
//! block so they harmonize with the default theme.

use palantir::{Background, Color, Configure, Corners, Panel, Sizing, Text, TextStyle, Ui};
use std::hash::Hash;

/// Teal-blue. Default swatch when one color is enough.
pub const A: Color = Color::hex(0x4cd3ff);
/// Orange. Pair with `A` for "two distinct things".
pub const B: Color = Color::hex(0xffa63d);
/// Green.
pub const C: Color = Color::hex(0xd9ff57);
/// Purple.
pub const D: Color = Color::hex(0xd897ff);

/// Standard swatch fill — colored rect with a 4 px corner radius.
pub fn swatch_bg(c: Color) -> Background {
    Background {
        fill: c.into(),
        radius: Corners::all(4.0),
        ..Default::default()
    }
}

/// 12 px caption, default color. Used for section titles and small labels.
pub fn caption_style() -> TextStyle {
    TextStyle::default().with_font_size(12.0)
}

/// Near-black text for placing on top of a bright swatch fill — a
/// legibility requirement, not decoration.
pub fn on_swatch_text() -> TextStyle {
    TextStyle::default()
        .with_font_size(13.0)
        .with_color(Color::hex(0x1a1a1a))
}

/// Title + body pair: a small caption above a child block. No card
/// decoration — the surrounding showcase panel already contains the demo.
pub fn section<T, H: Hash + Copy>(
    ui: &mut Ui<T>,
    id: H,
    title: &'static str,
    body: impl FnOnce(&mut Ui<T>),
) {
    Panel::vstack()
        .id_salt(id)
        .size((Sizing::FILL, Sizing::Hug))
        .gap(6.0)
        .show(ui, |ui| {
            Text::new(title)
                .id_salt((id, "section-title"))
                .style(caption_style())
                .show(ui);
            body(ui);
        });
}
