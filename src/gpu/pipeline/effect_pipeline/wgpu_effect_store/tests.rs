use std::time::Duration;

use glam::{UVec2, Vec2};

use crate::animation::animation_spec::AnimationSpec;
use crate::effect::effect_source::EffectSource;
use crate::gpu::device::gpu::Gpu;
use crate::gpu::pipeline::clip_variant::Clipping;
use crate::gpu::surface::owned_target::OwnedTarget;
use crate::gpu::test_gpu::{headless_test_gpu, isolated_headless_test_gpu};
use crate::gpu::texel::TargetFormat;
use crate::host::offscreen::{OffscreenHost, OffscreenTime};
use crate::primitives::effect::effect_name::EffectName;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::layout::clip_mode::ClipMode;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::srgba_u8::SrgbaU8;
use crate::primitives::paint::material::Material;
use crate::primitives::paint::material::lens::Lens;
use crate::renderer::render_buffer::shape_form::ShapeForm;
use crate::shape::Shape;
use crate::shape::effect_paint::EffectPaint;
use crate::text::shaper::TextShaper;
use crate::theme::Theme;
use crate::theme::color_scheme::ColorScheme;
use crate::theme::look::press_effect::PressEffect;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widgets::panel::Panel;

const BLUE: EffectName<()> = EffectName::named("blue").expect("the name fits");
const RED: EffectName<()> = EffectName::named("red").expect("the name fits");

fn solid(rgb: &str) -> String {
    format!(
        "fn effect(input: EffectInput) -> EffectOutput {{
            return effect_over(vec4<f32>({rgb}, 1.0));
        }}"
    )
}

fn host() -> (OffscreenHost, OwnedTarget) {
    let gpu = headless_test_gpu();
    let host = OffscreenHost::builder(gpu.handles())
        .shaper(TextShaper::test_mono())
        .time(OffscreenTime::Manual)
        .build();
    let target = OwnedTarget::window(
        &gpu,
        "palantir.effect.builds",
        UVec2::new(8, 8),
        TargetFormat::new(wgpu::TextureFormat::Rgba8UnormSrgb),
    );
    (host, target)
}

/// An effect builds where a use first wants it, and stays built: the stock ripples (ids 0
/// and 1), the glass (3) and `OnUse` blue (4) build nothing over a frame that draws none of
/// them, `Now` red (5) builds at that frame, and so does the frost (2), which the stock themes'
/// floats wear; blue builds at the frame that draws it, red staying built. A reload of blue as
/// green builds again, so the next frame paints green. A material builds at the frame that
/// draws it, through the one stage its effect draws every use with: the glass for a glass box,
/// while a blurred box draws through the frost, built already. A triangle of blue builds
/// blue's triangle form alone, at the frame that draws it, beside its box.
#[test]
fn an_effect_builds_where_a_use_wants_it() {
    let (mut host, target) = host();
    host.ui()
        .load_effect(BLUE, EffectSource::new(solid("0.0, 0.0, 1.0")))
        .unwrap();
    host.ui()
        .load_effect(
            RED,
            EffectSource::new(solid("1.0, 0.0, 0.0")).prebuild(true),
        )
        .unwrap();
    host.frame(target.as_target(), &mut |ui: &mut Ui| {
        ui.add_shape(Shape::rect(), RgbaF32::WHITE);
    });
    let store = |host: &OffscreenHost| {
        host.effect_store()
            .built_effects(ShapeForm::Box, Clipping::Off)
    };
    assert_eq!(store(&host), [false, false, true, false, false, true]);

    let mut blue = |ui: &mut Ui| {
        ui.add_shape(Shape::rect(), EffectPaint::new(BLUE, ()));
    };
    host.frame(target.as_target(), &mut blue);
    assert_eq!(store(&host), [false, false, true, false, true, true]);
    assert_eq!(
        target.read_back().texel(UVec2::new(4, 4)),
        SrgbaU8::rgb(0, 0, 255)
    );

    host.ui()
        .load_effect(BLUE, EffectSource::new(solid("0.0, 1.0, 0.0")))
        .unwrap();
    host.frame(target.as_target(), &mut blue);
    assert_eq!(
        target.read_back().texel(UVec2::new(4, 4)),
        SrgbaU8::rgb(0, 255, 0)
    );

    host.frame(target.as_target(), &mut |ui: &mut Ui| {
        ui.add_shape(Shape::rect(), RgbaF32::WHITE);
        ui.add_shape(Shape::rect(), Material::blur(3.0));
    });
    assert_eq!(store(&host), [false, false, true, false, true, true]);
    host.frame(target.as_target(), &mut |ui: &mut Ui| {
        ui.add_shape(Shape::rect(), RgbaF32::WHITE);
        ui.add_shape(Shape::rect(), Material::glass(Lens::WINDOW_GLASS));
    });
    assert_eq!(store(&host), [false, false, true, true, true, true]);

    let triangles = |host: &OffscreenHost| {
        host.effect_store()
            .built_effects(ShapeForm::Triangle, Clipping::Off)
    };
    assert_eq!(triangles(&host), [false; 6]);
    host.frame(target.as_target(), &mut |ui: &mut Ui| {
        ui.add_shape(
            Shape::triangle(Vec2::ZERO, Vec2::new(8.0, 0.0), Vec2::new(0.0, 8.0)),
            EffectPaint::new(BLUE, ()),
        );
    });
    assert_eq!(triangles(&host), [false, false, false, false, true, false]);
    assert_eq!(store(&host), [false, false, true, true, true, true]);
    assert_eq!(
        target.read_back().texel(UVec2::new(1, 1)),
        SrgbaU8::rgb(0, 255, 0)
    );
}

/// A want builds a form's clipless half alone; the clipped half waits for the first frame
/// whose draws name a rounded clip, then builds for every wanted form at once: blue (id 4),
/// which that frame draws inside the clip and paints, and the frost (2) and `Now` red (5), which
/// a theme and a load wanted and no frame draws.
#[test]
fn the_clipped_half_waits_for_a_frame_with_a_rounded_clip() {
    let (mut host, target) = host();
    host.ui()
        .load_effect(BLUE, EffectSource::new(solid("0.0, 0.0, 1.0")))
        .unwrap();
    host.ui()
        .load_effect(
            RED,
            EffectSource::new(solid("1.0, 0.0, 0.0")).prebuild(true),
        )
        .unwrap();
    let built = |host: &OffscreenHost, clipping| {
        host.effect_store().built_effects(ShapeForm::Box, clipping)
    };
    let wanted = [false, false, true, false, true, true];
    host.frame(target.as_target(), &mut |ui: &mut Ui| {
        ui.add_shape(Shape::rect(), EffectPaint::new(BLUE, ()));
    });
    assert_eq!(built(&host, Clipping::Off), wanted);
    assert_eq!(built(&host, Clipping::On), [false; 6]);

    host.frame(target.as_target(), &mut |ui: &mut Ui| {
        Panel::canvas()
            .id_salt("rounded")
            .size(8.0)
            .chrome(Chrome::rounded(RgbaF32::WHITE, Corners::all(3.0)))
            .clip(ClipMode::Rounded)
            .show(ui, |ui| {
                ui.add_shape(Shape::rect(), EffectPaint::new(BLUE, ()));
            });
    });
    assert_eq!(built(&host, Clipping::On), wanted);
    assert_eq!(
        target.read_back().texel(UVec2::new(4, 4)),
        SrgbaU8::rgb(0, 0, 255)
    );
}

/// The press effects of a window's themes build at the next frame that draws, for both
/// schemes, each with the alternative it names: a sparkle theme for the scheme not shown
/// builds nothing before that frame, then the sparkle (id 1) and the flat ripple it saves
/// power with (0). A theme another set replaced before the frame builds nothing.
#[test]
fn a_window_theme_builds_its_press_effects_at_the_next_record() {
    let sparkle = || {
        let mut theme = Theme::stock(ColorScheme::Light);
        theme.transition = AnimationSpec::SNAP;
        theme.button.defaults.press = Some(PressEffect {
            effect: EffectName::SPARKLE,
            color: RgbaF32::WHITE,
            highlight: RgbaF32::WHITE,
            enter: Duration::ZERO,
            exit: Duration::ZERO,
        });
        theme
    };
    let mut nothing = |_: &mut Ui| {};
    let mut fill = |ui: &mut Ui| {
        ui.add_shape(Shape::rect(), RgbaF32::WHITE);
    };

    let (mut host, target) = host();
    host.ui_mut()
        .set_scheme_theme(ColorScheme::Dark, sparkle())
        .unwrap();
    host.ui_mut()
        .set_scheme_theme(ColorScheme::Dark, Theme::stock(ColorScheme::Dark))
        .unwrap();
    let ripples = |host: &OffscreenHost| {
        host.effect_store()
            .built_effects(ShapeForm::Box, Clipping::Off)[..2]
            .to_vec()
    };
    host.frame(target.as_target(), &mut nothing);
    assert_eq!(ripples(&host), [false, false]);

    let shown = host.ui().color_scheme();
    let hidden = match shown {
        ColorScheme::Light => ColorScheme::Dark,
        ColorScheme::Dark => ColorScheme::Light,
    };
    host.ui_mut().set_scheme_theme(hidden, sparkle()).unwrap();
    assert_eq!(ripples(&host), [false, false]);
    host.frame(target.as_target(), &mut fill);
    assert_eq!(ripples(&host), [true, true]);
}

/// A store lives as long as its device: a load, a loss and a recovery onto another device, in
/// that order, build again what the registry wants before the recovered host's first frame
/// draws, the frost (2) its theme names and `Now` red (5), with blue (4), which that frame
/// draws; a reload of blue after the recovery paints its new module.
#[test]
fn a_recovery_builds_again_what_the_registry_wants() {
    let lost = isolated_headless_test_gpu();
    let mut host = OffscreenHost::builder(lost.handles())
        .shaper(TextShaper::test_mono())
        .time(OffscreenTime::Manual)
        .build();
    host.ui()
        .load_effect(BLUE, EffectSource::new(solid("0.0, 0.0, 1.0")))
        .unwrap();
    host.ui()
        .load_effect(
            RED,
            EffectSource::new(solid("1.0, 0.0, 0.0")).prebuild(true),
        )
        .unwrap();
    let mut blue = |ui: &mut Ui| {
        ui.add_shape(Shape::rect(), EffectPaint::new(BLUE, ()));
    };
    let window = |gpu: &Gpu| {
        OwnedTarget::window(
            gpu,
            "palantir.effect.recovery",
            UVec2::new(8, 8),
            TargetFormat::new(wgpu::TextureFormat::Rgba8UnormSrgb),
        )
    };
    host.frame(window(&lost).as_target(), &mut blue);
    lost.lose();

    let gpu = headless_test_gpu();
    host.recover(gpu.handles());
    let target = window(&gpu);
    host.frame(target.as_target(), &mut blue);
    assert_eq!(
        host.effect_store()
            .built_effects(ShapeForm::Box, Clipping::Off),
        [false, false, true, false, true, true]
    );
    assert_eq!(
        target.read_back().texel(UVec2::new(4, 4)),
        SrgbaU8::rgb(0, 0, 255)
    );

    host.ui()
        .load_effect(BLUE, EffectSource::new(solid("0.0, 1.0, 0.0")))
        .unwrap();
    host.frame(target.as_target(), &mut blue);
    assert_eq!(
        target.read_back().texel(UVec2::new(4, 4)),
        SrgbaU8::rgb(0, 255, 0)
    );
}

/// A target of a new format builds every loaded effect for it before its frame: the same
/// effect paints opaque blue into an RGBA target, then into a BGRA one, each read back as RGBA.
#[test]
fn a_new_format_builds_the_loaded_effects() {
    let gpu = headless_test_gpu();
    let mut host = OffscreenHost::builder(gpu.handles())
        .shaper(TextShaper::test_mono())
        .time(OffscreenTime::Manual)
        .build();
    let wgsl = "fn effect(input: EffectInput) -> EffectOutput {
        return effect_over(vec4<f32>(0.0, 0.0, 1.0, 1.0));
    }";
    host.ui()
        .load_effect(BLUE, EffectSource::new(wgsl))
        .unwrap();
    let mut app = |ui: &mut Ui| {
        ui.add_shape(Shape::rect(), RgbaF32::new(0.2, 0.4, 0.6, 1.0));
        ui.add_shape(Shape::rect(), EffectPaint::new(BLUE, ()));
    };
    for format in [
        wgpu::TextureFormat::Rgba8UnormSrgb,
        wgpu::TextureFormat::Bgra8UnormSrgb,
    ] {
        let target = OwnedTarget::window(
            &gpu,
            "palantir.effect.format",
            UVec2::new(8, 8),
            TargetFormat::new(format),
        );
        host.frame(target.as_target(), &mut app);
        assert_eq!(
            target.read_back().texel(UVec2::new(4, 4)),
            SrgbaU8::rgb(0, 0, 255),
            "{format:?}"
        );
    }
}
