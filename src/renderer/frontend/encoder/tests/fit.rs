//! How an image or icon resolves its destination rect and UVs.

use crate::layout::types::sizing::Sizing;
use crate::primitives::rect::Rect;
use crate::renderer::frontend::capture::PaintCall;
use crate::scene::damage::region::DamageRegion;
use crate::ui::harness::UiHarness;
use crate::widgets::configure::Configure;
use crate::widgets::panel::Panel;
use glam::{UVec2, Vec2};

/// Pin: each [`ImageDownsample`] mode reaches the shader as its own flag
/// bit,
/// and `Single` as none.
///
/// The bits are how the mode survives the trip — the record is gone by the
/// time
/// the fragment shader runs, so a mode that encoded to zero would silently
/// draw
/// as the default, and two modes sharing a bit would draw as each other.
/// The
/// tap bits also have to stay clear of the filter ones, since one `flags`
/// word
/// carries both and the shader masks them apart.
#[test]
fn downsample_modes_encode_to_distinct_tap_flags() {
    use crate::primitives::image::{Image, ImageDownsample};
    use crate::renderer::render_buffer::image::{
        IMG_FLAG_MAG_NEAREST, IMG_FLAG_MIN_NEAREST, IMG_FLAG_TAPS_MEAN, IMG_FLAG_TAPS_PEAK,
    };
    use crate::shape::Shape;

    let modes = [
        ("Single", ImageDownsample::Single, 0),
        ("Mean", ImageDownsample::Mean, IMG_FLAG_TAPS_MEAN),
        ("Peak", ImageDownsample::Peak, IMG_FLAG_TAPS_PEAK),
    ];

    let mut h = UiHarness::new(UVec2::new(200, 200));
    let handle = h
        .ui()
        .load_image(&Image::from_srgba8(UVec2::new(2, 2), vec![255; 16]))
        .unwrap();
    // Three shapes on one node: they all paint the same rect, and record order
    // is what pairs each draw back up with the mode that asked for it.
    h.frame(|ui| {
        Panel::canvas()
            .auto_id()
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                for (_, mode, _) in modes {
                    ui.add_shape(Shape::image(handle.clone()).downsample(mode));
                }
            });
    });

    let cmds = h.encode_paint_for(DamageRegion::from(Rect::new(0.0, 0.0, 200.0, 200.0)));
    let flags: Vec<u32> = cmds
        .calls
        .iter()
        .filter_map(|call| match call {
            PaintCall::Image { payload, .. } => Some(payload.flags),
            _ => None,
        })
        .collect();
    assert_eq!(flags.len(), modes.len(), "one image draw per mode");
    for ((label, _, expected), actual) in modes.into_iter().zip(flags) {
        assert_eq!(actual, expected, "{label} encoded the wrong tap flags");
        assert_eq!(
            actual & (IMG_FLAG_MIN_NEAREST | IMG_FLAG_MAG_NEAREST),
            0,
            "{label} must not collide with the filter bits",
        );
    }
}

/// The cascade bounds an image by the rect the encoder draws, under every
/// fit — so an `ImageFit::None` image larger than its node, which
/// overflows it, is damaged and culled where it paints. A 200×100 image
/// in a 100×100 node at (40, 40), no transform and no clip, so the
/// cascade's screen rect and the draw rect are in one space.
#[test]
fn the_cascade_bounds_an_image_by_the_rect_the_encoder_draws() {
    use crate::primitives::image::{Image, ImageFit};
    use crate::primitives::widget_id::WidgetId;
    use crate::shape::Shape;

    let fits = [
        ImageFit::Fill,
        ImageFit::Contain,
        ImageFit::Cover,
        ImageFit::None,
        ImageFit::Tile {
            offset: Vec2::ZERO,
            scale: Vec2::splat(2.0),
        },
    ];
    // Drawn rects, hand-computed: Contain scales by min(100/200, 100/100)
    // = 0.5 → 100×50, centred at y = 40 + 25; None paints 200×100 centred,
    // x = 40 + (100 - 200)/2 = -10, y = 40.
    let node = Rect::new(40.0, 40.0, 100.0, 100.0);
    let drawn = [
        node,
        Rect::new(40.0, 65.0, 100.0, 50.0),
        node,
        Rect::new(-10.0, 40.0, 200.0, 100.0),
        node,
    ];
    let mut h = UiHarness::new(UVec2::new(300, 300));
    let handle = h
        .ui()
        .load_image(&Image::from_srgba8(
            UVec2::new(200, 100),
            vec![255; 200 * 100 * 4],
        ))
        .unwrap();
    for (fit, expected) in fits.into_iter().zip(drawn) {
        h.frame(|ui| {
            Panel::canvas()
                .id(WidgetId::from_hash("root"))
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    Panel::zstack()
                        .id(WidgetId::from_hash("frame"))
                        .position((node.min.x, node.min.y))
                        .size(node.size.w)
                        .show(ui, |ui| {
                            ui.add_shape(Shape::image(handle.clone()).fit(fit));
                        });
                });
        });
        let cmds = h.encode_paint_for(DamageRegion::from(Rect::new(0.0, 0.0, 300.0, 300.0)));
        let draws: Vec<Rect> = cmds
            .calls
            .iter()
            .filter_map(|call| match call {
                PaintCall::Image { payload, .. } => Some(payload.rect),
                _ => None,
            })
            .collect();
        assert_eq!(draws, [expected], "{fit:?} draw");
        let mut rows = Vec::new();
        h.ui.cascade().owned_paints(h.ui.forest(), &mut rows);
        let screens: Vec<Rect> = rows
            .iter()
            .filter(|row| row.owner == WidgetId::from_hash("frame"))
            .map(|row| row.screen)
            .collect();
        assert_eq!(screens, [expected], "{fit:?} cascade bound");
    }
}
