use crate::internals::harness::UiHarness;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::color_coords::ColorCoords;
use crate::primitives::paint::color::color_model::ColorModel;
use crate::primitives::paint::image::Image;
use crate::widget_core::configure::Configure;
use crate::widgets::color_strip::{ColorStrip, StripPaint};
use glam::{UVec2, Vec2};

const BAR: UVec2 = UVec2::new(208, 14);

fn harness() -> UiHarness {
    UiHarness::new(BAR)
}

/// The bar writes only its own axis; an alpha bar that wrote the colour would undo picks.
#[test]
fn the_alpha_bar_writes_only_alpha() {
    let id = WidgetId::from_hash("strip-alpha-writes");
    let mut h = harness();
    let mut color = RgbaF32::hex(0x4cd3ff).with_alpha(1.0);
    let before = color;
    h.frame(|ui| {
        ColorStrip::for_alpha(&mut color).id(id).show(ui);
    });
    h.press_at(Vec2::new(52.0, 7.0));
    h.frame(|ui| {
        ColorStrip::for_alpha(&mut color).id(id).show(ui);
    });
    assert_eq!(color.a, 0.25, "a quarter along the bar");
    assert_eq!((color.r, color.g, color.b), (before.r, before.g, before.b));
}

#[test]
fn the_hue_bar_writes_only_the_hue() {
    let id = WidgetId::from_hash("strip-hue-writes");
    let mut h = harness();
    let mut coords = ColorCoords::new(ColorModel::Okhsv, RgbaF32::hex(0x4cd3ff), 0.0);
    let (sat, val) = (coords.saturation(), coords.value());
    h.frame(|ui| {
        ColorStrip::for_hue(&mut coords).id(id).show(ui);
    });
    h.press_at(Vec2::new(156.0, 7.0));
    h.frame(|ui| {
        ColorStrip::for_hue(&mut coords).id(id).show(ui);
    });
    assert_eq!(coords.hue(), 0.75, "three quarters along the bar");
    assert_eq!(coords.saturation(), sat);
    assert_eq!(coords.value(), val);
    assert_eq!(coords.model(), ColorModel::Okhsv);
}

/// The alpha bar's texture keeps straight alpha for the GPU to composite over the checker.
#[test]
fn the_alpha_texture_is_the_colour_at_every_alpha() {
    let color = RgbaF32::hex(0x4cd3ff);
    let want = color.to_srgba_u8();
    let mut image = Image::blank(UVec2::new(4, 2));
    StripPaint::Alpha(color).fill(&mut image);
    let texels = image.texels();
    for texel in texels {
        assert_eq!(
            (texel.r, texel.g, texel.b),
            (want.r, want.g, want.b),
            "colour held"
        );
    }
    let alphas: Vec<u8> = texels.iter().take(4).map(|t| t.a).collect();
    assert_eq!(alphas, vec![32, 96, 159, 223]);
}

#[test]
fn every_row_of_a_bar_is_the_first_row() {
    let size = UVec2::new(6, 3);
    let mut image = Image::blank(size);
    StripPaint::Hue(ColorModel::Okhsv).fill(&mut image);
    let texels = image.texels();
    let row = size.x as usize;
    assert_eq!(texels.len(), row * size.y as usize);
    assert_eq!(&texels[..row], &texels[row..row * 2]);
    assert_eq!(&texels[..row], &texels[row * 2..]);
}

/// The hue ramp is each hue's most saturated colour, sRGB-encoded; the texture must match Okhsv's bytes at hue 0.5.
#[test]
fn the_hue_texture_follows_the_model() {
    for model in ColorModel::ALL {
        let size = UVec2::new(8, 1);
        let mut image = Image::blank(size);
        StripPaint::Hue(model).fill(&mut image);
        let texels = image.texels();
        for column in 0..size.x {
            let hue = (column as f32 + 0.5) / size.x as f32;
            let want = model.slice(hue).color(1.0, 1.0).to_srgba_u8();
            assert_eq!(texels[column as usize], want, "{model:?} at hue {hue}");
        }
    }
}

#[test]
fn a_click_commits_as_a_drag_does() {
    let id = WidgetId::from_hash("strip-click-commits");
    let mut h = harness();
    let mut color = RgbaF32::hex(0x4cd3ff).with_alpha(1.0);
    let frame = |h: &mut UiHarness, color: &mut RgbaF32| {
        h.frame_value(|ui| {
            let r = ColorStrip::for_alpha(color).id(id).show(ui);
            (r.changed, r.committed)
        })
    };
    frame(&mut h, &mut color);
    h.press_at(Vec2::new(52.0, 7.0));
    let (changed, committed) = frame(&mut h, &mut color);
    assert!(changed && !committed, "the press writes, not commits");
    h.release();
    let (_, committed) = frame(&mut h, &mut color);
    assert!(committed, "the release is the edit");
}

/// The hue bar's right end is hue 1, not 0: a drag past the edge and End store 1, Home 0; steps past either end wrap.
#[test]
fn the_hue_bar_keeps_both_ends() {
    use crate::input::keyboard::key::Key;

    let id = WidgetId::from_hash("strip-hue-ends");
    let mut h = harness();
    let mut coords = ColorCoords::new(ColorModel::Okhsv, RgbaF32::hex(0x4cd3ff), 0.0);
    let show = |h: &mut UiHarness, coords: &mut ColorCoords| {
        h.frame(|ui| {
            ColorStrip::for_hue(coords).id(id).show(ui);
        });
    };
    show(&mut h, &mut coords);
    h.press_at(Vec2::new(BAR.x as f32 - 8.0, 7.0));
    show(&mut h, &mut coords);
    h.drag_to(Vec2::new(BAR.x as f32 + 40.0, 7.0));
    show(&mut h, &mut coords);
    h.release();
    show(&mut h, &mut coords);
    assert_eq!(coords.hue(), 1.0, "past the right edge");

    h.set_focus(id);
    for (key, hue) in [(Key::Home, 0.0), (Key::End, 1.0)] {
        h.key(key);
        show(&mut h, &mut coords);
        assert_eq!(coords.hue(), hue, "{key:?}");
    }
    h.key(Key::ArrowRight);
    show(&mut h, &mut coords);
    assert_eq!(
        coords.hue(),
        (1.0f32 + 0.005).rem_euclid(1.0),
        "a step wraps"
    );
    h.key(Key::PageDown);
    show(&mut h, &mut coords);
    let paged = (1.0f32 + 0.005).rem_euclid(1.0) - 0.1;
    assert_eq!(coords.hue(), paged.rem_euclid(1.0), "so does a page");
}

/// PageUp/PageDown step an alpha bar by 0.1, clamping at the ends.
#[test]
fn page_keys_step_the_alpha_bar() {
    use crate::input::keyboard::key::Key;

    let id = WidgetId::from_hash("strip-alpha-page");
    let mut h = harness();
    let mut color = RgbaF32::hex(0x4cd3ff).with_alpha(0.5);
    let show = |h: &mut UiHarness, color: &mut RgbaF32| {
        h.frame(|ui| {
            ColorStrip::for_alpha(color).id(id).show(ui);
        });
    };
    show(&mut h, &mut color);
    h.set_focus(id);
    let steps = [
        (Key::PageUp, 0.5 + 0.1),
        (Key::PageDown, 0.5 + 0.1 - 0.1),
        (Key::End, 1.0),
        (Key::PageUp, 1.0),
        (Key::Home, 0.0),
        (Key::PageDown, 0.0),
    ];
    for (key, alpha) in steps {
        h.key(key);
        show(&mut h, &mut color);
        assert_eq!(color.a, alpha, "{key:?}");
    }
}

/// The bar's texture and backdrop image live on its own id and leave with it.
#[test]
fn the_surface_leaves_with_the_bar() {
    use crate::widgets::color_surface::ColorSurface;

    let id = WidgetId::from_hash("leaving-bar");
    let mut coords = ColorCoords::default();
    let mut h = harness();
    h.frame(|ui| {
        ColorStrip::for_hue(&mut coords).id(id).show(ui);
    });
    let surface = |h: &UiHarness| h.ui.state::<ColorSurface<StripPaint>>(id).is_some();
    assert!(surface(&h), "built while the bar records");
    h.frame(|_| {});
    assert!(!surface(&h), "swept with the bar");
}
