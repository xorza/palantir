//! How an image or icon resolves its destination rect and UVs.

use crate::layout::types::sizing::Sizing;
use crate::primitives::rect::Rect;
use crate::renderer::frontend::capture::PaintCall;
use crate::scene::damage::region::DamageRegion;
use crate::scene::node::Configure;
use crate::ui::harness::UiHarness;
use crate::widgets::panel::Panel;
use glam::{UVec2, Vec2};

/// Pin: image `fit` resolution. A 100×50 image painted into a 200×200
/// rect produces a different paint rect for each fit mode:
/// - `Fill` keeps the full 200×200 rect (image stretched).
/// - `Contain` scales by min(200/100, 200/50)=2 → 200×100, centered.
/// - `Cover` scales by max(200/100, 200/50)=4 → 400×200 conceptually,
///   but rendered at full 200×200 with UV-cropped to (0.5..1.0)
///   vertical band of the texture (`uv_size.y = 50/200 = 0.25`).
/// - `None` paints at intrinsic 100×50 centered.
#[test]
fn image_fit_modes_resolve_to_expected_rects_and_uv() {
    use crate::ImageFit;
    use crate::renderer::frontend::encoder::resolve_fit;
    use glam::Vec2;

    let base = Rect::new(0.0, 0.0, 200.0, 200.0);
    let img = Vec2::new(100.0, 50.0);

    let r = resolve_fit(base, img, ImageFit::Fill);
    assert_eq!(r.rect, base);
    assert_eq!(r.uv_min, Vec2::ZERO);
    assert_eq!(r.uv_size, Vec2::ONE);

    let r = resolve_fit(base, img, ImageFit::Contain);
    assert_eq!(r.rect, Rect::new(0.0, 50.0, 200.0, 100.0));
    assert_eq!(r.uv_size, Vec2::ONE);

    let r = resolve_fit(base, img, ImageFit::Cover);
    assert_eq!(r.rect, base);
    // 200×200 paint rect over a 400×200 scaled image → keep 0.5 of the
    // width centered; full height. UVs sample the centered band.
    assert!((r.uv_size.x - 0.5).abs() < 1e-5);
    assert!((r.uv_size.y - 1.0).abs() < 1e-5);
    assert!((r.uv_min.x - 0.25).abs() < 1e-5);
    assert!((r.uv_min.y - 0.0).abs() < 1e-5);

    let r = resolve_fit(base, img, ImageFit::None);
    assert_eq!(r.rect, Rect::new(50.0, 75.0, 100.0, 50.0));
    assert_eq!(r.uv_size, Vec2::ONE);

    // Missing registry entry → falls through to base + full UV.
    let r = resolve_fit(base, Vec2::ZERO, ImageFit::Contain);
    assert_eq!(r.rect, base);
    assert_eq!(r.uv_size, Vec2::ONE);

    // Tile: raw caller UV, full rect, intrinsic size ignored. `scale`
    // (3×2 repeats) → uv_size; `offset` (0.5, 0.25) → uv_min.
    let r = resolve_fit(
        base,
        img,
        ImageFit::Tile {
            offset: Vec2::new(0.5, 0.25),
            scale: Vec2::new(3.0, 2.0),
        },
    );
    assert_eq!(r.rect, base);
    assert_eq!(r.uv_min, Vec2::new(0.5, 0.25));
    assert_eq!(r.uv_size, Vec2::new(3.0, 2.0));
}

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
        .register_image(Image::from_rgba8(2, 2, vec![255; 16]))
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

/// `IconFit` picks a rasterization box, so every mode is a rect and the
/// numbers are hand-checkable. A 24x12 artwork in a 100x100 rect:
/// `Contain` scales by min(100/24, 100/12) = 4.166.., giving 100x50 centred
/// vertically; `Fill` takes the rect whole; `None` paints 24x12 centred.
#[test]
fn icon_fit_resolves_to_hand_computed_rects() {
    use crate::renderer::frontend::encoder::resolve_icon_fit;
    use crate::shape::icon::IconFit;

    let base = Rect::new(10.0, 20.0, 100.0, 100.0);
    let art = Vec2::new(24.0, 12.0);

    // scale = 100/24 = 4.1666667 → 100 x 50, dy = (100 - 50)/2 = 25.
    let contained = resolve_icon_fit(base, art, IconFit::Contain);
    assert_eq!(contained.min, Vec2::new(10.0, 45.0));
    assert_eq!((contained.size.w, contained.size.h), (100.0, 50.0));

    assert_eq!(resolve_icon_fit(base, art, IconFit::Fill), base);

    // Intrinsic px, centred: dx = (100-24)/2 = 38, dy = (100-12)/2 = 44.
    let intrinsic = resolve_icon_fit(base, art, IconFit::None);
    assert_eq!(intrinsic.min, Vec2::new(48.0, 64.0));
    assert_eq!((intrinsic.size.w, intrinsic.size.h), (24.0, 12.0));

    // A square artwork in a square rect is the same rect under every mode
    // that preserves aspect — the case that would hide an axis mix-up.
    let square = Rect::new(0.0, 0.0, 32.0, 32.0);
    assert_eq!(
        resolve_icon_fit(square, Vec2::splat(16.0), IconFit::Contain),
        square,
    );

    // A degenerate viewBox falls through to the base rect rather than
    // dividing by zero — the same fail-safe the image path takes.
    assert_eq!(resolve_icon_fit(base, Vec2::ZERO, IconFit::Contain), base);
}
