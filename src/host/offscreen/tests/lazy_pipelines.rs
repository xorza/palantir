//! A format's pipelines, built on the first frame that draws through each.

use crate::gpu::pipeline::clip_variant::Clipping;
use crate::gpu::pipeline::quad_pipeline::cutout_plan::ShadowEntry;
use crate::gpu::pipeline::quad_pipeline::quad_draw::QuadDraw;
use crate::gpu::surface::render_target::TargetFormat;
use crate::gpu::test_gpu::headless_test_gpu;
use crate::host::offscreen::OffscreenHost;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::paint::Paint;
use crate::primitives::paint::shadow::Shadow;
use crate::renderer::render_buffer::fill_form::FillForm;
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
                .chrome(Chrome::new(fill))
                .show(ui);
        }
        Scene::Shadowed => {
            Block::new()
                .id_salt("shadowed")
                .at((10.0, 10.0))
                .size(20.0)
                .chrome(Chrome {
                    paint: Paint {
                        fill: fill.into(),
                        shadow: Shadow::drop(RgbaF32::BLACK, Vec2::new(1.0, 2.0), 3.0),
                        ..Chrome::NONE.paint
                    },
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
                        .chrome(Chrome::new(RgbaF32::WHITE))
                        .show(ui);
                });
        }
    }
}

/// A plain frame builds no shadow form and no half that takes rounded clips; the first shadowed
/// frame builds the plain shadow form it draws, still no clipping half; the first frame whose
/// draws name a rounded clip builds the solid quad's clipping half, and no shadow's, as it draws no
/// shadow.
#[test]
fn a_pipeline_waits_for_the_first_frame_that_draws_through_it() {
    let gpu = headless_test_gpu();
    let mut host = OffscreenHost::builder(gpu.handles())
        .shaper(TextShaper::test_mono())
        .build();
    let target = gpu.target("palantir.test.lazy_pipelines", UVec2::new(80, 80));
    let solid = QuadDraw::Fill(FillForm::BOX_SOLID);
    let shadow = QuadDraw::Shadow(ShadowEntry::General);
    let built = |host: &OffscreenHost, draw: QuadDraw, clipping: Clipping| {
        host.device
            .backend()
            .format_pipelines(TargetFormat::RGBA8_SRGB)
            .quad
            .is_built(draw, clipping)
    };

    host.frame(target.as_target(), &mut |ui: &mut Ui| {
        record(ui, Scene::Plain);
    });
    assert!(built(&host, solid, Clipping::Off));
    assert!(!built(&host, solid, Clipping::On), "no clipping half");
    assert!(!built(&host, shadow, Clipping::Off), "no shadow form");

    host.frame(target.as_target(), &mut |ui: &mut Ui| {
        record(ui, Scene::Shadowed);
    });
    let shadowed = [
        ShadowEntry::General,
        ShadowEntry::Tables,
        ShadowEntry::TablesWide,
    ]
    .into_iter()
    .any(|entry| built(&host, QuadDraw::Shadow(entry), Clipping::Off));
    assert!(shadowed, "the shadow form it drew");
    assert!(!built(&host, solid, Clipping::On), "still no clipping half");

    host.frame(target.as_target(), &mut |ui: &mut Ui| {
        record(ui, Scene::Clipped);
    });
    assert!(
        built(&host, solid, Clipping::On),
        "the solid's clipping half"
    );
    assert!(
        !built(&host, shadow, Clipping::On),
        "no shadow's clipping half"
    );
}
