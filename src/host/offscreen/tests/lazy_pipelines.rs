//! A format's pipelines, built on the first frame that draws through each.

use crate::gpu::pipeline::quad_pipeline::cutout_plan::ShadowEntry;
use crate::gpu::pipeline::quad_pipeline::quad_draw::QuadDraw;
use crate::gpu::pipeline::quad_pipeline::quad_form::QuadForm;
use crate::gpu::pipeline::stencil_variant::StencilMode;
use crate::gpu::surface::render_target::TargetFormat;
use crate::gpu::test_gpu::headless_test_gpu;
use crate::host::offscreen::OffscreenHost;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::shadow::Shadow;
use crate::text::shaper::TextShaper;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widgets::block::Block;
use crate::widgets::panel::Panel;
use glam::{UVec2, Vec2};

/// What a frame draws: a plain block, then a shadowed one, then a block inside a rounded clip.
#[derive(Clone, Copy, Debug)]
enum Scene {
    Plain,
    Shadowed,
    Clipped,
}

fn record(ui: &mut Ui, scene: Scene) {
    let fill = RgbaF32::hex(0x30364a);
    match scene {
        Scene::Plain => {
            Block::new()
                .id_salt("plain")
                .size(20.0)
                .chrome(Chrome::fill(fill))
                .show(ui);
        }
        Scene::Shadowed => {
            Block::new()
                .id_salt("shadowed")
                .at((10.0, 10.0))
                .size(20.0)
                .chrome(Chrome {
                    fill: fill.into(),
                    shadow: Shadow::drop(RgbaF32::BLACK, Vec2::new(1.0, 2.0), 3.0),
                    ..Chrome::NONE
                })
                .show(ui);
        }
        Scene::Clipped => {
            Panel::zstack()
                .id_salt("clip")
                .size(40.0)
                .chrome(Chrome::rounded(fill, Corners::all(8.0)))
                .clip_rounded()
                .show(ui, |ui| {
                    Block::new()
                        .id_salt("inside")
                        .size(60.0)
                        .chrome(Chrome::fill(RgbaF32::WHITE))
                        .show(ui);
                });
        }
    }
}

/// A plain frame builds no shadow form, no stencil twin and no mask; the first shadowed frame
/// builds the plain shadow form it draws, still no twin; the first frame with a rounded clip
/// builds the solid quad's twin and the masks, and no shadow twin, as it draws no shadow.
#[test]
fn a_pipeline_waits_for_the_first_frame_that_draws_through_it() {
    let gpu = headless_test_gpu();
    let mut host = OffscreenHost::builder(gpu.handles())
        .shaper(TextShaper::test_mono())
        .build();
    let target = gpu.target("palantir.test.lazy_pipelines", UVec2::new(80, 80));
    let solid = QuadDraw::Form(QuadForm::Solid);
    let shadow = QuadDraw::Shadow(ShadowEntry::General);
    let built = |host: &OffscreenHost, draw: QuadDraw, mode: StencilMode| {
        host.core
            .backend()
            .format_pipelines(TargetFormat::RGBA8_SRGB)
            .quad
            .of(draw)
            .is_built(mode)
    };
    let masks = |host: &OffscreenHost| {
        host.core
            .backend()
            .format_pipelines(TargetFormat::RGBA8_SRGB)
            .quad
            .has_masks()
    };

    host.frame(target.as_target(), &mut |ui: &mut Ui| {
        record(ui, Scene::Plain);
    });
    assert!(built(&host, solid, StencilMode::Off));
    assert!(!built(&host, solid, StencilMode::Test), "no twin");
    assert!(!built(&host, shadow, StencilMode::Off), "no shadow form");
    assert!(!masks(&host), "no mask");

    host.frame(target.as_target(), &mut |ui: &mut Ui| {
        record(ui, Scene::Shadowed);
    });
    let shadowed = [
        ShadowEntry::General,
        ShadowEntry::Tables,
        ShadowEntry::TablesWide,
    ]
    .into_iter()
    .any(|entry| built(&host, QuadDraw::Shadow(entry), StencilMode::Off));
    assert!(shadowed, "the shadow form it drew");
    assert!(!built(&host, solid, StencilMode::Test), "still no twin");

    host.frame(target.as_target(), &mut |ui: &mut Ui| {
        record(ui, Scene::Clipped);
    });
    assert!(built(&host, solid, StencilMode::Test), "the solid twin");
    assert!(masks(&host), "the masks");
    assert!(!built(&host, shadow, StencilMode::Test), "no shadow twin");
}
