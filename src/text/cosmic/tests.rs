//! How a glyph's scaler is instanced on a variable face, including one whose `wght` axis is out
//! of order.

use crate::text::cosmic::glyph_face::GlyphFace;
use crate::text::cosmic::glyph_raster::{GlyphRaster, GlyphRequest};
use crate::text::cosmic::stem_darkening::StemDarkening;
use crate::text::font_scope::internals::MONO;
use cosmic_text::{Font, FontSystem, Weight, fontdb};
use glam::Vec2;
use std::sync::Arc;

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

/// The bundled JetBrains Mono, from a database of its own.
pub(super) fn mono() -> Arc<Font> {
    let [mono] = faces([MONO.to_vec()]);
    mono
}

/// `font`, its one face, at `weight`.
pub(super) fn mono_face(font: &Font, weight: Weight) -> GlyphFace<'_> {
    GlyphFace::new(font.data(), 0, weight).expect("the face parses")
}

/// The bundled JetBrains Mono and its inverted copy, in that order, from one database.
fn mono_and_inverted() -> [Arc<Font>; 2] {
    faces([MONO.to_vec(), with_inverted_wght(MONO)])
}

/// Each face of `fonts`, loaded into one database.
fn faces<const N: usize>(fonts: [Vec<u8>; N]) -> [Arc<Font>; N] {
    let mut font_system =
        FontSystem::new_with_locale_and_db(String::from("en-US"), fontdb::Database::new());
    fonts.map(|bytes| {
        let ids = font_system
            .db_mut()
            .load_font_source(fontdb::Source::Binary(Arc::new(bytes)));
        font_system
            .get_font(ids[0], Weight::NORMAL)
            .expect("the face loads")
    })
}

/// Coverage of `M` at `weight` and 48 ppem, drawn through `raster` as the measurer draws it,
/// undarkened.
fn render(raster: &mut GlyphRaster, font: &Font, weight: u16) -> Vec<u8> {
    let face = mono_face(font, Weight(weight));
    let image = raster
        .render(
            &face,
            GlyphRequest {
                glyph: face.glyph_of('M'),
                ppem: 48.0,
                darkening: StemDarkening::NONE,
                skew: None,
                offset: Vec2::ZERO,
            },
        )
        .expect("an outline glyph renders");
    image.data.to_vec()
}

/// An axis with inverted bounds takes no coordinate (`f32::clamp` would panic), so every weight
/// renders the default instance: the valid face at its default `wght` of 400. The valid face at
/// 800 differs, so the weight reaches the outline at all.
#[test]
fn an_inverted_weight_axis_renders_the_default_instance() {
    let [mono, inverted] = mono_and_inverted();
    let regular = render(&mut GlyphRaster::default(), &mono, 400);
    assert_ne!(render(&mut GlyphRaster::default(), &mono, 800), regular);
    for weight in [100, 400, 800] {
        assert_eq!(
            render(&mut GlyphRaster::default(), &inverted, weight),
            regular,
            "weight {weight} on the inverted axis",
        );
    }
}

/// A raster keeps its scratch between glyphs, so a glyph drawn after a bold one must not carry
/// its weight: after a bold glyph, the inverted face renders as on a fresh raster.
#[test]
fn a_glyph_without_a_weight_drops_the_last_glyphs_coordinates() {
    let [mono, inverted] = mono_and_inverted();
    let fresh = render(&mut GlyphRaster::default(), &inverted, 400);

    let mut raster = GlyphRaster::default();
    render(&mut raster, &mono, 800);
    assert_eq!(render(&mut raster, &inverted, 400), fresh);
}

/// A one-rectangle TrueType glyph over `x0..x1 × y0..y1`, in font units.
fn rect_glyph([x0, y0, x1, y1]: [i16; 4]) -> Vec<u8> {
    let mut glyph = Vec::new();
    for v in [1, x0, y0, x1, y1] {
        glyph.extend(v.to_be_bytes());
    }
    // One contour of four on-curve points; no instructions; each coordinate a 16-bit delta.
    glyph.extend(3u16.to_be_bytes());
    glyph.extend(0u16.to_be_bytes());
    glyph.extend([1u8; 4]);
    for dx in [x0, 0, x1 - x0, 0] {
        glyph.extend(dx.to_be_bytes());
    }
    for dy in [y0, y1 - y0, 0, y0 - y1] {
        glyph.extend(dy.to_be_bytes());
    }
    glyph
}

/// A face of 1000 units an em whose glyph 0 is a COLR v0 color glyph of two layers: glyph 1, the
/// left half of the em square, in palette 0's red, under glyph 2, the right half, in its
/// half-transparent blue. Built table by table, as no font of the bundle holds COLR.
pub(super) fn two_layer_color_font() -> Vec<u8> {
    color_font_with_second_layer_in(1)
}

/// [`two_layer_color_font`] with its second layer at `palette_index` of the two-color palette.
pub(super) fn color_font_with_second_layer_in(palette_index: u16) -> Vec<u8> {
    let be16 = |v: u16| v.to_be_bytes();
    let be32 = |v: u32| v.to_be_bytes();
    let glyphs = [
        rect_glyph([0, 0, 1000, 1000]),
        rect_glyph([0, 0, 500, 1000]),
        rect_glyph([500, 0, 1000, 1000]),
    ];
    let mut glyf = Vec::new();
    let mut loca = Vec::new();
    for glyph in &glyphs {
        loca.extend(be32(glyf.len() as u32));
        glyf.extend(glyph);
    }
    loca.extend(be32(glyf.len() as u32));
    let head: Vec<u8> = [
        &be32(0x0001_0000)[..],
        &be32(0x0001_0000),
        &be32(0),
        &be32(0x5F0F_3CF5),
        &be16(0),
        &be16(1000),
        &[0; 16],
        &[0, 0, 0, 0, 0x03, 0xE8, 0x03, 0xE8],
        &be16(0),
        &be16(8),
        &be16(2),
        &be16(1),
        &be16(0),
    ]
    .concat();
    let maxp = [&be32(0x0000_5000)[..], &be16(3)].concat();
    let hhea = [
        &be32(0x0001_0000)[..],
        &be16(1000),
        &[0; 4],
        &be16(1000),
        &[0; 4],
        &be16(1000),
        &be16(1),
        &[0; 14],
        &be16(3),
    ]
    .concat();
    // Each glyph's left side bearing is its `xMin`, where a TrueType scaler puts it.
    let hmtx = [0, 0, 500]
        .map(|lsb| [be16(1000), be16(lsb)].concat())
        .concat();
    let colr = [
        &be16(0)[..],
        &be16(1),
        &be32(14),
        &be32(20),
        &be16(2),
        &be16(0),
        &be16(0),
        &be16(2),
        &be16(1),
        &be16(0),
        &be16(2),
        &be16(palette_index),
    ]
    .concat();
    // Records in BGRA: opaque red, then blue at alpha 128.
    let cpal = [
        &be16(0)[..],
        &be16(2),
        &be16(1),
        &be16(2),
        &be32(14),
        &be16(0),
        &[0, 0, 255, 255],
        &[255, 0, 0, 128],
    ]
    .concat();
    // "Test" as its family name and its PostScript name, for Windows in UTF-16BE, which a font
    // database needs both of to load a face.
    let family: Vec<u8> = "Test".encode_utf16().flat_map(u16::to_be_bytes).collect();
    let record = |name_id: u16| {
        [3, 1, 0x0409, name_id, family.len() as u16, 0]
            .map(be16)
            .concat()
    };
    let name = [
        &be16(0)[..],
        &be16(2),
        &be16(30),
        &record(1),
        &record(6),
        &family,
    ]
    .concat();
    let tables: [(&[u8; 4], Vec<u8>); 9] = [
        (b"COLR", colr),
        (b"CPAL", cpal),
        (b"glyf", glyf),
        (b"head", head),
        (b"hhea", hhea),
        (b"hmtx", hmtx),
        (b"loca", loca),
        (b"maxp", maxp),
        (b"name", name),
    ];
    let mut font = [
        &be32(0x0001_0000)[..],
        &be16(9),
        &be16(128),
        &be16(3),
        &be16(16),
    ]
    .concat();
    let mut offset = font.len() + tables.len() * 16;
    let mut data = Vec::new();
    for (tag, table) in &tables {
        font.extend(tag.as_slice());
        font.extend(be32(0));
        font.extend(be32((offset + data.len()) as u32));
        font.extend(be32(table.len() as u32));
        data.extend(table);
        data.resize(data.len().next_multiple_of(4), 0);
    }
    offset += data.len();
    font.extend(data);
    assert_eq!(font.len(), offset);
    font
}
