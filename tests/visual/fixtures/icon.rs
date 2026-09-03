//! Baked-icon fixtures. Exact-pixel assertions (no goldens): every icon here
//! is a solid rectangle whose colour is written down in the SVG, so what
//! should reach the framebuffer is hand-derivable and the test pins the
//! *semantics* — the raster lands at the exact physical size, on whole pixels,
//! with the tint applied the way the icon's kind says — rather than a snapshot.

use glam::{UVec2, Vec2};
use palantir::{Color, Configure, IconFit, IconTable, Panel, Sizing, Text, TextStyle, Ui};
use std::rc::Rc;

use crate::fixtures::close;
use crate::harness::Harness;

/// Fills its whole 8x8 viewBox with one colour, so every covered pixel is
/// fully opaque and the raster's extent is exactly the icon's box. Marked
/// tintable, so the artwork colour is discarded and the shape's tint supplies
/// it — which is what makes the expected value the tint and nothing else.
const SOLID_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 8 8"><rect width="8" height="8" fill="#fff"/></svg>"##;

/// Two opaque halves, as a colour icon: the artwork's own colours must survive
/// to the framebuffer, which a mask icon's would not.
const HALVES_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 8 8"><rect width="4" height="8" fill="#e63c3c"/><rect x="4" width="4" height="8" fill="#3c78e6"/></svg>"##;

/// sRGB of the two halves, as authored.
const LEFT: [u8; 4] = [0xe6, 0x3c, 0x3c, 255];
const RIGHT: [u8; 4] = [0x3c, 0x78, 0xe6, 255];

/// The set, built once per thread. `from_svgs` derives each icon's viewBox
/// and tintability by parsing it, so the fixtures state only their artwork.
fn atlas() -> Rc<IconTable> {
    thread_local! {
        static BUILT: Rc<IconTable> =
            Rc::new(IconTable::from_svgs([("halves", HALVES_SVG), ("solid", SOLID_SVG)]));
    }
    BUILT.with(Rc::clone)
}

/// Assert a 20x20 solid pane at `at` is `tint` across its own pixels and
/// nothing beyond them.
///
/// Five interior samples and one pixel outside each side, at offsets from
/// the pane's own origin — which is what lets a case move the pane and
/// keep the derivation. The interior proves the tint reached the
/// framebuffer; the exterior proves the raster is the size of the box
/// rather than rounded up into its neighbour.
///
/// `Color::rgb` takes sRGB components and the sRGB render target encodes
/// them back on write, so the expected bytes are the authored ones — the
/// same round trip the clear-colour smoke test pins.
fn assert_solid_pane(img: &image::RgbaImage, at: Vec2, tint: [f32; 3]) {
    let (ox, oy) = (at.x as u32, at.y as u32);
    let expected = tint.map(|c| (c * 255.0f32).round() as u8);
    for (dx, dy) in [(2, 2), (10, 10), (18, 18), (1, 18), (18, 1)] {
        let (x, y) = (ox + dx, oy + dy);
        let p = img.get_pixel(x, y).0;
        for c in 0..3 {
            assert!(
                p[c].abs_diff(expected[c]) <= 4,
                "({x},{y}) = {p:?} should be the tint {expected:?}",
            );
        }
        assert_eq!(p[3], 255, "({x},{y}) must be opaque");
    }
    for (dx, dy) in [(-2, 10), (10, -2), (22, 10), (10, 22)] {
        let (x, y) = (ox.wrapping_add_signed(dx), oy.wrapping_add_signed(dy));
        assert!(
            close(img.get_pixel(x, y).0, [0, 0, 0, 255]),
            "({x},{y}) = {:?} must still be the clear colour",
            img.get_pixel(x, y).0,
        );
    }
}

/// One icon in an exactly placed pane, so the pixels it owns are known from
/// the pane's position and size alone.
fn pane(ui: &mut Ui, id: &'static str, at: Vec2, size: Vec2, name: &str, tint: Color) {
    pane_desaturated(ui, id, at, size, name, tint, false);
}

fn pane_desaturated(
    ui: &mut Ui,
    id: &'static str,
    at: Vec2,
    size: Vec2,
    name: &str,
    tint: Color,
    desaturate: bool,
) {
    let icons = ui.load_icons(atlas());
    let icon = icons.by_name(name).expect("fixture icon");
    Panel::zstack()
        .id_salt(id)
        .position(at)
        .size((Sizing::fixed(size.x), Sizing::fixed(size.y)))
        .show(ui, |ui| {
            ui.add_shape(
                icons
                    .shape(icon)
                    .fit(IconFit::Fill)
                    .tint(tint)
                    .desaturate(desaturate),
            );
        });
}

/// A tintable icon reaches the framebuffer as coverage times the shape's tint,
/// filling exactly the pixels its pane covers and none beyond.
#[test]
fn tintable_icon_fills_its_exact_pixel_box_with_the_tint() {
    let mut h = Harness::new();
    let tint = Color::rgb(0.2, 0.8, 0.4);
    let img = h.render(UVec2::new(48, 48), 1.0, Color::BLACK, |ui| {
        Panel::canvas()
            .id_salt("icon_exact")
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                pane(
                    ui,
                    "solid",
                    Vec2::new(6.0, 6.0),
                    Vec2::splat(20.0),
                    "solid",
                    tint,
                );
            });
    });

    assert_solid_pane(&img, Vec2::new(6.0, 6.0), [0.2, 0.8, 0.4]);
}

/// A colour icon keeps the artwork's own colours — the tint's RGB is ignored,
/// only its alpha applies. Drawn with a saturated red tint that a mask icon
/// would have taken on: the halves must stay red and blue regardless.
#[test]
fn colour_icon_keeps_its_own_colours_under_a_tint() {
    let mut h = Harness::new();
    let img = h.render(UVec2::new(48, 32), 1.0, Color::BLACK, |ui| {
        Panel::canvas()
            .id_salt("icon_colour")
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                pane(
                    ui,
                    "halves",
                    Vec2::new(8.0, 4.0),
                    Vec2::new(32.0, 24.0),
                    "halves",
                    Color::rgb(1.0, 0.0, 0.0),
                );
            });
    });

    // Pane spans x 8..40, so the seam is at x = 24. Sample well inside each
    // half to stay clear of the one-pixel edge the rasterizer antialiases.
    assert!(
        close(img.get_pixel(14, 16).0, LEFT),
        "left half = {:?}, expected the authored red {LEFT:?} — a colour icon \
         must not take the tint's RGB",
        img.get_pixel(14, 16).0,
    );
    assert!(
        close(img.get_pixel(34, 16).0, RIGHT),
        "right half = {:?}, expected the authored blue {RIGHT:?}",
        img.get_pixel(34, 16).0,
    );
}

/// The pixel-exactness claim, at the scale that makes it: 1.5.
///
/// A 20x20 logical pane at (4, 4) is physical 6..36 — a 30 px box, inside the
/// ladder's exact band, so the icon rasterizes at exactly 30x30 and lands on
/// whole pixels. The edges are what the test is really about: at 6 and 35 the
/// icon is present, at 5 and 36 it is not.
#[test]
fn icon_rasterizes_to_whole_physical_pixels_at_fractional_scale() {
    let mut h = Harness::new();
    let img = h.render(UVec2::new(48, 48), 1.5, Color::BLACK, |ui| {
        Panel::canvas()
            .id_salt("icon_scaled")
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                pane(
                    ui,
                    "solid",
                    Vec2::new(4.0, 4.0),
                    Vec2::splat(20.0),
                    "solid",
                    Color::WHITE,
                );
            });
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

/// `desaturate` collapses a colour icon to its own luminance — the disabled
/// look for artwork a tint cannot recolour.
///
/// Both greys are hand-computed, which is what pins the *coefficients* rather
/// than merely "something grey came out". Per half: sRGB → linear, dot with
/// Rec. 709 (0.2126, 0.7152, 0.0722), then sRGB-encode.
///
/// - `#e63c3c` → linear (0.7913, 0.0452, 0.0452) → luma 0.2038 → **125**
/// - `#3c78e6` → linear (0.0452, 0.1878, 0.7913) → luma 0.2011 → **124**
///
/// The two land a byte apart because these particular colours are very nearly
/// isoluminant — which is exactly why the test asserts the computed values and
/// not an ordering between them.
#[test]
fn desaturate_greys_a_colour_icon_by_its_luminance() {
    let mut h = Harness::new();
    let img = h.render(UVec2::new(48, 32), 1.0, Color::BLACK, |ui| {
        Panel::canvas()
            .id_salt("icon_grey")
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                pane_desaturated(
                    ui,
                    "halves",
                    Vec2::new(8.0, 4.0),
                    Vec2::new(32.0, 24.0),
                    "halves",
                    Color::WHITE,
                    true,
                );
            });
    });

    let left = img.get_pixel(14, 16).0;
    let right = img.get_pixel(34, 16).0;
    for (label, px, expected) in [("left", left, 125u8), ("right", right, 124)] {
        assert!(
            px[0] == px[1] && px[1] == px[2],
            "{label} half = {px:?} must be neutral grey after desaturation",
        );
        assert!(
            px[0].abs_diff(expected) <= 2,
            "{label} half = {} but its luminance works out to {expected}",
            px[0],
        );
        assert_eq!(px[3], 255, "{label} half keeps its alpha");
    }
    // A flat channel average would put both at 117; the artwork's own colours
    // would leave them at LEFT / RIGHT. Neither is what luminance gives.
    assert!(left != LEFT && right != RIGHT, "grey is not the artwork");
}

/// Text and icons share one pipeline, so a step of either kind that
/// follows the other rebinds nothing — see `Bound::Raster`. Both orders
/// have to land the same pixels as either kind drawn alone.
///
/// The icon is what the assertions read, because its coverage is
/// hand-derivable: a solid 8x8 viewBox filled to its pane is the tint on
/// every pixel of the box and the clear colour one pixel out. Text
/// bracketing it above and below is what forces the two transitions —
/// `admit_higher_kind` closes the open text batch for an icon, so the
/// pass runs text, icon, text.
///
/// A regression here is not subtle: a raster step that inherited the
/// wrong pipeline draws the atlas through the wrong shader, and a step
/// that lost the viewport immediate lands its quad at garbage NDC and
/// leaves the box empty.
#[test]
fn an_icon_between_two_text_runs_lands_in_its_own_pixel_box() {
    let mut h = Harness::new();
    let tint = Color::rgb(0.2, 0.8, 0.4);
    let label = |ui: &mut Ui, salt: &'static str, at: Vec2| {
        Panel::zstack()
            .id_salt(salt)
            .position(at)
            .size((Sizing::fixed(40.0), Sizing::fixed(14.0)))
            .show(ui, |ui| {
                Text::new("Ag")
                    .id_salt(salt)
                    .style(
                        &TextStyle::default()
                            .with_font_size(12.0)
                            .with_color(Color::WHITE),
                    )
                    .show(ui);
            });
    };
    let img = h.render(UVec2::new(48, 72), 1.0, Color::BLACK, |ui| {
        Panel::canvas()
            .id_salt("raster_order")
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                label(ui, "before", Vec2::new(6.0, 0.0));
                pane(
                    ui,
                    "solid",
                    Vec2::new(6.0, 26.0),
                    Vec2::splat(20.0),
                    "solid",
                    tint,
                );
                label(ui, "after", Vec2::new(6.0, 54.0));
            });
    });

    // Exactly what the icon has to land at when it is drawn alone, which
    // is what makes the two orders comparable.
    assert_solid_pane(&img, Vec2::new(6.0, 26.0), [0.2, 0.8, 0.4]);

    // Both runs actually drew, or the icon proved nothing about a
    // transition that never happened. Counting lit pixels per band
    // rather than naming one: glyph coverage moves with the face.
    let lit_in = |rows: std::ops::Range<u32>| {
        rows.flat_map(|y| (0..48).map(move |x| (x, y)))
            .filter(|&(x, y)| img.get_pixel(x, y).0[0] > 32)
            .count()
    };
    assert!(lit_in(0..14) > 0, "the run before the icon must have drawn");
    assert!(lit_in(54..68) > 0, "the run after the icon must have drawn");
}
