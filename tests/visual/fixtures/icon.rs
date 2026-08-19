//! Baked-icon fixtures. Exact-pixel assertions (no goldens): every icon here
//! is a solid rectangle whose colour is written down in the SVG, so what
//! should reach the framebuffer is hand-derivable and the test pins the
//! *semantics* — the raster lands at the exact physical size, on whole pixels,
//! with the tint applied the way the icon's kind says — rather than a snapshot.

use glam::{UVec2, Vec2};
use palantir::{Color, Configure, IconAtlas, IconFit, Panel, Sizing, Ui};
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
fn atlas() -> Rc<IconAtlas> {
    thread_local! {
        static BUILT: Rc<IconAtlas> =
            Rc::new(IconAtlas::from_svgs([("halves", HALVES_SVG), ("solid", SOLID_SVG)]));
    }
    BUILT.with(Rc::clone)
}

/// One icon in an exactly placed pane, so the pixels it owns are known from
/// the pane's position and size alone.
fn pane(ui: &mut Ui, id: &'static str, at: Vec2, size: Vec2, name: &str, tint: Color) {
    let icons = ui.load_icons(atlas());
    let icon = icons.by_name(name).expect("fixture icon");
    Panel::zstack()
        .id_salt(id)
        .position(at)
        .size((Sizing::fixed(size.x), Sizing::fixed(size.y)))
        .show(ui, |ui| {
            ui.add_shape(icons.shape(icon).fit(IconFit::Fill).tint(tint));
        });
}

/// A tintable icon reaches the framebuffer as coverage times the shape's tint,
/// filling exactly the pixels its pane covers and none beyond.
///
/// At scale 1.0 a 20x20 pane at (6, 6) is physical pixels 6..26 on both axes.
/// Interior pixels must be the tint exactly; the pixel just outside must still
/// be the clear colour, which is what proves the raster is the size of the box
/// rather than rounded up into its neighbour.
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

    // `Color::rgb` takes sRGB components, and the sRGB render target encodes
    // them back on write, so the framebuffer holds the authored values — the
    // same round trip the clear-colour smoke test pins.
    let expected = [
        (0.2 * 255.0f32).round() as u8,
        (0.8 * 255.0f32).round() as u8,
        (0.4 * 255.0f32).round() as u8,
        255,
    ];
    for (x, y) in [(8, 8), (16, 16), (24, 24), (7, 24), (24, 7)] {
        let p = img.get_pixel(x, y).0;
        for c in 0..3 {
            assert!(
                p[c].abs_diff(expected[c]) <= 4,
                "({x},{y}) = {p:?} should be the tint {expected:?}",
            );
        }
        assert_eq!(p[3], 255, "({x},{y}) must be opaque");
    }

    // One pixel outside the pane on each side: still the clear colour, so the
    // raster did not spill past the box it was sized for.
    for (x, y) in [(4, 16), (16, 4), (28, 16), (16, 28)] {
        assert!(
            close(img.get_pixel(x, y).0, [0, 0, 0, 255]),
            "({x},{y}) = {:?} must still be the clear colour",
            img.get_pixel(x, y).0,
        );
    }
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
