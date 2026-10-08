//! Baked-icon fixtures with exact-pixel assertions (no goldens). Every icon is a
//! solid rectangle whose colour is written in the SVG, so the result is
//! hand-derivable and the tests pin the *semantics*: the raster lands at the exact
//! physical size, on whole pixels, tinted as the icon's kind says.

#![expect(
    clippy::cast_sign_loss,
    reason = "test fixtures cast non-negative sizes, coordinates, indices and colour channels"
)]

use glam::{UVec2, Vec2};
use palantir::golden::image::RgbaImage;
use palantir::widget::{IconFit, IconShape};
use palantir::{Configure, IconSet, IconTable, Panel, RgbaF32, Sizing, Text, TextStyle, Ui};
use std::rc::Rc;

use crate::fixtures::{SRGB_ROUND_TRIP, assert_px, canvas};
use crate::harness::Harness;
use std::ops;

/// Fills its 8x8 viewBox with one colour, so every covered pixel is opaque. Marked
/// tintable, so the expected value is the tint alone.
const SOLID_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 8 8"><rect width="8" height="8" fill="#fff"/></svg>"##;

/// Two opaque halves, as a colour icon whose own colours must survive.
const HALVES_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 8 8"><rect width="4" height="8" fill="#e63c3c"/><rect x="4" width="4" height="8" fill="#3c78e6"/></svg>"##;

const LEFT: [u8; 4] = [0xe6, 0x3c, 0x3c, 255];
const RIGHT: [u8; 4] = [0x3c, 0x78, 0xe6, 255];

/// The fixture set: `halves` and `solid`.
pub(crate) fn atlas() -> Rc<IconTable> {
    Rc::new(IconTable::from_svgs([("halves", HALVES_SVG), ("solid", SOLID_SVG)]).unwrap())
}

/// Assert every interior pixel of a 20x20 solid pane at `at` is `tint`: the whole
/// interior, because glyph coverage drawn *over* a pane moves with the face,
/// stopping a pixel short of each edge to leave the rasterizer's boundary row out.
/// `RgbaF32::srgb` is encoded back on write, so the expected bytes are the authored
/// ones.
fn assert_pane_interior(img: &RgbaImage, at: Vec2, tint: [f32; 3]) {
    let (ox, oy) = (at.x as u32, at.y as u32);
    let expected = tint.map(|c| (c * 255.0f32).round() as u8);
    for dy in 1..19 {
        for dx in 1..19 {
            let (x, y) = (ox + dx, oy + dy);
            let [r, g, b] = expected;
            assert_px(
                img.get_pixel(x, y).0,
                [r, g, b, 255],
                SRGB_ROUND_TRIP,
                format_args!("({x},{y}) is the tint, opaque"),
            );
        }
    }
}

/// [`assert_pane_interior`] plus one pixel outside each side, proving the raster is
/// the box's size; only for panes with nothing near them.
fn assert_solid_pane(img: &RgbaImage, at: Vec2, tint: [f32; 3]) {
    assert_pane_interior(img, at, tint);
    let (ox, oy) = (at.x as u32, at.y as u32);
    for (dx, dy) in [(-2, 10), (10, -2), (22, 10), (10, 22)] {
        let (x, y) = (ox.wrapping_add_signed(dx), oy.wrapping_add_signed(dy));
        assert_px(
            img.get_pixel(x, y).0,
            [0, 0, 0, 255],
            SRGB_ROUND_TRIP,
            format_args!(
                "({x},{y}) = {:?} must still be the clear colour",
                img.get_pixel(x, y).0
            ),
        );
    }
}

/// One frame on black at `scale`, with `scene` on a full-surface canvas and the [`atlas`] loaded.
fn render(size: UVec2, scale: f32, mut scene: impl FnMut(&mut Ui, &IconSet)) -> RgbaImage {
    let atlas = atlas();
    Harness::new()
        .size(size)
        .scale(scale)
        .clear(RgbaF32::BLACK)
        .frame(|ui| {
            let icons = ui.load_icons(Rc::clone(&atlas));
            canvas(ui, |ui| scene(ui, &icons));
        })
        .image
}

/// Icon `name` of `icons`, filling its pane.
fn icon(icons: &IconSet, name: &str) -> IconShape {
    icons
        .shape(icons.by_name(name).expect("fixture icon"))
        .fit(IconFit::Fill)
}

/// `icon` in a pane `size` at `at`.
fn pane(ui: &mut Ui, at: Vec2, size: Vec2, icon: IconShape) {
    Panel::zstack()
        .id_salt(("pane", at.x as i32, at.y as i32))
        .position(at)
        .size((Sizing::fixed(size.x), Sizing::fixed(size.y)))
        .show(ui, |ui| ui.add_shape(icon));
}

/// A tintable icon reaches the framebuffer as coverage times the tint, filling
/// exactly its pane's pixels.
#[test]
fn tintable_icon_fills_its_exact_pixel_box_with_the_tint() {
    let tint = RgbaF32::srgb(0.2, 0.8, 0.4);
    let img = render(UVec2::new(48, 48), 1.0, |ui, icons| {
        pane(
            ui,
            Vec2::new(6.0, 6.0),
            Vec2::splat(20.0),
            icon(icons, "solid").tint(tint),
        );
    });

    assert_solid_pane(&img, Vec2::new(6.0, 6.0), [0.2, 0.8, 0.4]);
}

/// A colour icon keeps the artwork's colours; the tint's RGB is ignored and only
/// its alpha applies, so a saturated red tint must leave the halves red and blue.
#[test]
fn colour_icon_keeps_its_own_colours_under_a_tint() {
    let img = render(UVec2::new(48, 32), 1.0, |ui, icons| {
        pane(
            ui,
            Vec2::new(8.0, 4.0),
            Vec2::new(32.0, 24.0),
            icon(icons, "halves").tint(RgbaF32::srgb(1.0, 0.0, 0.0)),
        );
    });

    // Pane spans x 8..40, so the seam is x = 24; sample inside each half.
    for (x, half) in [(14, LEFT), (34, RIGHT)] {
        assert_px(
            img.get_pixel(x, 16).0,
            half,
            SRGB_ROUND_TRIP,
            format_args!(
                "({x}, 16) is the authored colour: a colour icon must not take the tint's RGB"
            ),
        );
    }
}

/// The pixel-exactness claim at scale 1.5: a 20x20 logical pane at (4, 4) is
/// physical 6..36, a 30 px box inside the ladder's exact band, so the icon is
/// present at 6 and 35 and absent at 5 and 36.
#[test]
fn icon_rasterizes_to_whole_physical_pixels_at_fractional_scale() {
    let img = render(UVec2::new(48, 48), 1.5, |ui, icons| {
        pane(
            ui,
            Vec2::new(4.0, 4.0),
            Vec2::splat(20.0),
            icon(icons, "solid").tint(RgbaF32::WHITE),
        );
    });

    let lit = |x: u32, y: u32| img.get_pixel(x, y).0[0] > 200;
    let dark = |x: u32, y: u32| img.get_pixel(x, y).0[0] < 40;

    assert!(lit(6, 20), "left edge pixel 6 must be covered");
    assert!(lit(35, 20), "right edge pixel 35 must be covered");
    assert!(lit(20, 6), "top edge pixel 6 must be covered");
    assert!(lit(20, 35), "bottom edge pixel 35 must be covered");
    assert!(dark(5, 20), "pixel 5 is outside the 30 px box");
    assert!(dark(36, 20), "pixel 36 is outside the 30 px box");
    assert!(dark(20, 5), "pixel 5 is outside the 30 px box");
    assert!(dark(20, 36), "pixel 36 is outside the 30 px box");
}

/// `desaturate` collapses a colour icon to its own luminance (the disabled look).
/// The greys are hand-computed to pin the *coefficients*: sRGB to linear, dot with
/// Rec. 709 (0.2126, 0.7152, 0.0722), then sRGB-encode.
///
/// - `#e63c3c` → linear (0.7913, 0.0452, 0.0452) → luma 0.2038 → **125**
/// - `#3c78e6` → linear (0.0452, 0.1878, 0.7913) → luma 0.2011 → **124**
///
/// They land a byte apart as the colours are nearly isoluminant, so the test
/// asserts values, not an ordering.
#[test]
fn desaturate_greys_a_colour_icon_by_its_luminance() {
    let img = render(UVec2::new(48, 32), 1.0, |ui, icons| {
        pane(
            ui,
            Vec2::new(8.0, 4.0),
            Vec2::new(32.0, 24.0),
            icon(icons, "halves").tint(RgbaF32::WHITE).desaturate(true),
        );
    });

    let left = img.get_pixel(14, 16).0;
    let right = img.get_pixel(34, 16).0;
    for (label, px, expected) in [("left", left, 125u8), ("right", right, 124)] {
        assert!(
            px[0] == px[1] && px[1] == px[2],
            "{label} half = {px:?} must be neutral grey after desaturation",
        );
        assert_px(
            px,
            [expected, expected, expected, 255],
            SRGB_ROUND_TRIP,
            format_args!("{label} half is its luminance, opaque"),
        );
    }
    // A flat channel average would give 117 and the artwork would stay LEFT /
    // RIGHT.
    assert!(left != LEFT && right != RIGHT, "grey is not the artwork");
}

/// Paint order across the raster tiers: a label, an icon recorded *over* it, and a
/// second label in a clipped panel. The icon closes the first label's batch, so the
/// pass runs text, icon, text; text and icons share one pipeline, so neither
/// transition rebinds anything (see `Bound::Raster`).
///
/// **The icon box:** a solid 8x8 viewBox filled to its pane is the tint on every
/// pixel, and the first label's glyphs run through it, so any non-tint pixel is a
/// glyph that reordered above the icon (as a batch left open would, draining in the
/// clipped panel's group). **The two bands:** the label must have drawn or the box
/// proves nothing; lit pixels are counted either side of the icon.
#[test]
fn an_icon_recorded_over_a_label_stays_on_top_of_it() {
    let tint = RgbaF32::srgb(0.2, 0.8, 0.4);
    let label = |ui: &mut Ui, salt: &'static str, at: Vec2| {
        Panel::zstack()
            .id_salt(salt)
            .position(at)
            .size((Sizing::fixed(60.0), Sizing::fixed(20.0)))
            .show(ui, |ui| {
                Text::new("AgAgAg")
                    .id_salt(salt)
                    .style(
                        &TextStyle::default()
                            .with_font_size(12.0)
                            .with_color(RgbaF32::WHITE),
                    )
                    .show(ui);
            });
    };
    let img = render(UVec2::new(96, 72), 1.0, |ui, icons| {
        label(ui, "under", Vec2::new(6.0, 6.0));
        pane(
            ui,
            Vec2::new(20.0, 6.0),
            Vec2::splat(20.0),
            icon(icons, "solid").tint(tint),
        );
        // A rect clip changes the scissor without changing the stencil
        // chain: it flushes the group but leaves an open batch open,
        // giving it somewhere later to drain.
        Panel::zstack()
            .id_salt("panel")
            .clip_rect()
            .position(Vec2::new(0.0, 44.0))
            .size((Sizing::fixed(96.0), Sizing::fixed(28.0)))
            .show(ui, |ui| {
                label(ui, "after", Vec2::new(6.0, 46.0));
            });
    });

    assert_pane_interior(&img, Vec2::new(20.0, 6.0), [0.2, 0.8, 0.4]);

    // This run measures x 6..=51, y 9..=20, so it shows either side of the icon's
    // 20..40 box.
    let any_lit = |xs: ops::Range<u32>, ys: ops::Range<u32>| {
        ys.flat_map(|y| xs.clone().map(move |x| (x, y)))
            .any(|(x, y)| img.get_pixel(x, y).0[0] > 32)
    };
    assert!(any_lit(6..20, 6..26), "the label left of the icon");
    assert!(any_lit(40..52, 6..26), "the label right of the icon");
    assert!(any_lit(0..96, 44..72), "the label in the clipped panel");
}

/// Past the 512 px raster cap an icon still fills its box: the capped raster is
/// resampled up to it. A 300 logical px pane at (20, 20), scale 2 is physical
/// 40..640 while the raster is 512 (at its own size it would sit at 84..596).
#[test]
fn an_icon_past_the_raster_cap_fills_its_box() {
    let capped = |name| {
        render(UVec2::new(680, 680), 2.0, |ui, icons| {
            pane(
                ui,
                Vec2::new(20.0, 20.0),
                Vec2::splat(300.0),
                icon(icons, name).tint(RgbaF32::WHITE),
            );
        })
    };
    let img = capped("solid");
    let lit = |x: u32, y: u32| img.get_pixel(x, y).0[0] > 200;
    let dark = |x: u32, y: u32| img.get_pixel(x, y).0[0] < 40;
    assert!(lit(41, 340) && lit(638, 340), "the left and right edges");
    assert!(lit(340, 41) && lit(340, 638), "the top and bottom edges");
    assert!(dark(38, 340) && dark(642, 340), "nothing past the box");

    // A colour icon takes the same path through the colour atlas; each half keeps
    // its colour, filtered only along the seam.
    let img = capped("halves");
    for (x, half) in [(60, LEFT), (620, RIGHT)] {
        assert_px(
            img.get_pixel(x, 340).0,
            half,
            SRGB_ROUND_TRIP,
            format_args!("({x}, 340) is its half's colour"),
        );
    }
}
