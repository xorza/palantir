//! How a glyph's scaler is instanced on a variable face, including one whose `wght` axis is out
//! of order.

use crate::text::cosmic::axis_range::AxisRange;
use crate::text::cosmic::{GLYPH_SOURCES, Hinting, ScalerSpec, WGHT_AXIS};
use crate::text::font_scope::internals::MONO;
use cosmic_text::{Font, FontSystem, Weight, fontdb};
use std::sync::Arc;
use swash::scale::{Render, ScaleContext};
use swash::zeno::Format;

fn be_u16(data: &[u8], at: usize) -> usize {
    usize::from(u16::from_be_bytes([data[at], data[at + 1]]))
}

fn be_u32(data: &[u8], at: usize) -> usize {
    usize::try_from(u32::from_be_bytes([
        data[at],
        data[at + 1],
        data[at + 2],
        data[at + 3],
    ]))
    .unwrap()
}

/// `font` with the bounds of its `wght` axis record swapped, so `min > default > max`.
fn with_inverted_wght(font: &[u8]) -> Vec<u8> {
    let mut font = font.to_vec();
    let fvar = (0..be_u16(&font, 4))
        .map(|table| 12 + table * 16)
        .find(|&record| &font[record..record + 4] == b"fvar")
        .map(|record| be_u32(&font, record + 8))
        .expect("a variable face has an `fvar` table");
    let axes = fvar + be_u16(&font, fvar + 4);
    let axis_size = be_u16(&font, fvar + 10);
    let wght = (0..be_u16(&font, fvar + 8))
        .map(|axis| axes + axis * axis_size)
        .find(|&axis| &font[axis..axis + 4] == b"wght")
        .expect("the face varies on `wght`");
    // minValue at +4, maxValue at +12, each a 4-byte Fixed.
    for byte in 0..4 {
        font.swap(wght + 4 + byte, wght + 12 + byte);
    }
    font
}

/// The bundled JetBrains Mono and its inverted copy, in that order, from one database.
fn mono_and_inverted() -> [Arc<Font>; 2] {
    let mut font_system =
        FontSystem::new_with_locale_and_db(String::from("en-US"), fontdb::Database::new());
    [MONO.to_vec(), with_inverted_wght(MONO)].map(|bytes| {
        let ids = font_system
            .db_mut()
            .load_font_source(fontdb::Source::Binary(Arc::new(bytes)));
        font_system
            .get_font(ids[0], Weight::NORMAL)
            .expect("the face loads")
    })
}

/// Coverage of `M` at `weight`, read through `ctx` as the rasterizer does.
fn render(ctx: &mut ScaleContext, font: &Font, weight: u16) -> Vec<u8> {
    let glyph = font.as_swash().charmap().map('M');
    let mut scaler = ScalerSpec {
        size: 48.0,
        hinting: Hinting::Off,
        weight: Weight(weight),
    }
    .scaler(ctx, font);
    Render::new(&GLYPH_SOURCES)
        .format(Format::Alpha)
        .render(&mut scaler, glyph)
        .expect("an outline glyph renders")
        .data
}

/// An axis with inverted bounds panicked `f32::clamp`; it now takes no coordinate, so every
/// weight renders the default instance: the valid face at its default `wght` of 400. The valid
/// face at 800 differs, so the weight reaches the outline at all.
#[test]
fn an_inverted_weight_axis_renders_the_default_instance() {
    let [mono, inverted] = mono_and_inverted();
    let axis = |font: &Font| {
        AxisRange::from(
            font.as_swash()
                .variations()
                .find_by_tag(WGHT_AXIS)
                .expect("the face varies on `wght`"),
        )
    };
    assert_eq!(axis(&mono).coordinate(400.0), Some(400.0));
    assert_eq!(axis(&inverted).coordinate(400.0), None);

    let regular = render(&mut ScaleContext::new(), &mono, 400);
    assert_ne!(render(&mut ScaleContext::new(), &mono, 800), regular);
    for weight in [100, 400, 800] {
        assert_eq!(
            render(&mut ScaleContext::new(), &inverted, weight),
            regular,
            "weight {weight} on the inverted axis",
        );
    }
}

/// The context keeps the last glyph's coordinates, so a glyph with no weight to apply must still
/// reset them: after a bold glyph, the inverted face renders as on a fresh context.
#[test]
fn a_glyph_without_a_weight_drops_the_last_glyphs_coordinates() {
    let [mono, inverted] = mono_and_inverted();
    let fresh = render(&mut ScaleContext::new(), &inverted, 400);

    let mut ctx = ScaleContext::new();
    render(&mut ctx, &mono, 800);
    assert_eq!(render(&mut ctx, &inverted, 400), fresh);
}
