use crate::primitives::color::RgbaF32;
use crate::primitives::color::color_coords::ColorCoords;
use crate::primitives::color::color_model::ColorModel;
use crate::primitives::widget_id::WidgetId;
use crate::scene::node::configure::Configure;
use crate::ui::harness::UiHarness;
use crate::widgets::color_strip::{ColorStrip, StripPaint};
use glam::{UVec2, Vec2};

const BAR: UVec2 = UVec2::new(208, 14);

fn harness() -> UiHarness {
    UiHarness::new(BAR)
}

/// The bar writes the axis it owns and nothing else. An alpha bar that also
/// wrote the colour would undo a pick every time the opacity moved.
#[test]
fn the_alpha_bar_writes_only_alpha() {
    let id = WidgetId::from_hash("strip-alpha-writes");
    let mut h = harness();
    let mut color = RgbaF32::hex(0x4cd3ff).with_alpha(1.0);
    let before = color;
    h.frame(|ui| {
        ColorStrip::alpha(&mut color).id(id).show(ui);
    });
    h.press_at(Vec2::new(52.0, 7.0));
    h.frame(|ui| {
        ColorStrip::alpha(&mut color).id(id).show(ui);
    });
    assert_eq!(color.a, 0.25, "a quarter along the bar");
    assert_eq!((color.r, color.g, color.b), (before.r, before.g, before.b));
}

/// The hue bar moves the hue and leaves the other two axes and the model
/// where they were.
#[test]
fn the_hue_bar_writes_only_the_hue() {
    let id = WidgetId::from_hash("strip-hue-writes");
    let mut h = harness();
    let mut coords = ColorCoords::new(ColorModel::Okhsv, RgbaF32::hex(0x4cd3ff), 0.0);
    let (sat, val) = (coords.sat(), coords.val());
    h.frame(|ui| {
        ColorStrip::hue(&mut coords).id(id).show(ui);
    });
    h.press_at(Vec2::new(156.0, 7.0));
    h.frame(|ui| {
        ColorStrip::hue(&mut coords).id(id).show(ui);
    });
    assert_eq!(coords.hue(), 0.75, "three quarters along the bar");
    assert_eq!(coords.sat(), sat);
    assert_eq!(coords.val(), val);
    assert_eq!(coords.model(), ColorModel::Okhsv);
}

/// The alpha bar's texture carries **straight alpha** and lets the GPU
/// composite it over the checker, so its own texels keep the colour intact
/// from end to end. A CPU composite would bake the checker into the colour
/// and read wrong over any other ground.
#[test]
fn the_alpha_texture_is_the_colour_at_every_alpha() {
    let color = RgbaF32::hex(0x4cd3ff);
    let want = color.to_srgba_u8();
    let mut texels = Vec::new();
    StripPaint::Alpha(color).fill(&mut texels, UVec2::new(4, 2));
    for texel in texels.as_chunks::<4>().0 {
        assert_eq!(&texel[..3], &[want.r, want.g, want.b], "colour held");
    }
    // Four texels: centres at 1/8, 3/8, 5/8, 7/8 of the ramp.
    let alphas: Vec<u8> = texels
        .as_chunks::<4>()
        .0
        .iter()
        .take(4)
        .map(|t| t[3])
        .collect();
    assert_eq!(alphas, vec![32, 96, 159, 223]);
}

/// Both rows of a bar are the same row. The fill writes one and copies it,
/// which is what keeps a rebuild allocation-free.
#[test]
fn every_row_of_a_bar_is_the_first_row() {
    let mut texels = Vec::new();
    let size = UVec2::new(6, 3);
    StripPaint::Hue(ColorModel::Okhsv).fill(&mut texels, size);
    let row = size.x as usize * 4;
    assert_eq!(texels.len(), row * size.y as usize);
    assert_eq!(&texels[..row], &texels[row..row * 2]);
    assert_eq!(&texels[..row], &texels[row * 2..]);
}

/// The hue ramp shows each hue's most saturated colour, sRGB-encoded. Hue 0.5
/// of Okhsv is a cyan-green whose exact bytes the model decides — the point
/// here is that the texture agrees with it rather than with some ramp of its
/// own.
#[test]
fn the_hue_texture_follows_the_model() {
    for model in ColorModel::ALL {
        let size = UVec2::new(8, 1);
        let mut texels = Vec::new();
        StripPaint::Hue(model).fill(&mut texels, size);
        for column in 0..size.x {
            let hue = (column as f32 + 0.5) / size.x as f32;
            let want = model.slice(hue).color(1.0, 1.0).to_srgba_u8();
            let at = column as usize * 4;
            assert_eq!(
                &texels[at..at + 4],
                &[want.r, want.g, want.b, 255],
                "{model:?} at hue {hue}",
            );
        }
    }
}

/// A press writes and the release commits, whether or not the press ever
/// became a drag.
#[test]
fn a_click_commits_as_a_drag_does() {
    let id = WidgetId::from_hash("strip-click-commits");
    let mut h = harness();
    let mut color = RgbaF32::hex(0x4cd3ff).with_alpha(1.0);
    let frame = |h: &mut UiHarness, color: &mut RgbaF32| {
        h.frame_value(|ui| {
            let r = ColorStrip::alpha(color).id(id).show(ui);
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
