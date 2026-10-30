//! A partial frame on a device whose pass moves its whole target paints its damage into a canvas
//! and copies it into the backbuffer; on any other device it loads the backbuffer. The two paint
//! the same pixels.

use crate::gpu::device::pass_traffic::PassTraffic;
use crate::gpu::test_gpu::headless_test_gpu;
use crate::host::offscreen::OffscreenHost;
use crate::internals::record_app::RecordApp;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::mesh::Mesh;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::layout::clip_mode::ClipMode;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::shadow::Shadow;
use crate::primitives::paint::stroke::Stroke;
use crate::shape::Shape;
use crate::text::shaper::TextShaper;
use crate::ui::Ui;
use crate::ui::frame_report::FramePaint;
use crate::widget_core::configure::Configure;
use crate::widgets::block::Block;
use crate::widgets::panel::Panel;
use crate::widgets::text::Text;
use glam::{UVec2, Vec2};
use std::cell::Cell;

/// A translucent tile whose color turns each frame over a shadowed block, a rounded clip, text,
/// a mesh and a stroke off the pixel grid, so every pipeline that reads its fragment's position
/// or the stencil paints inside the damage.
fn scene(ui: &mut Ui, tile: RgbaF32, mesh: &Mesh) {
    Panel::canvas()
        .id_salt("canvas")
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            Block::new()
                .id_salt("shadowed")
                .pos((30.0, 40.0))
                .size(16.0)
                .chrome(Chrome {
                    fill: RgbaF32::hex(0x30364a).into(),
                    corners: Corners::all(4.0),
                    shadow: Shadow::drop(
                        RgbaF32::srgba(0.0, 0.0, 0.0, 0.75),
                        Vec2::new(1.5, 2.0),
                        4.0,
                    ),
                    ..Chrome::NONE
                })
                .show(ui);
            Panel::canvas()
                .id_salt("rounded")
                .pos((52.0, 24.0))
                .size(24.0)
                .chrome(Chrome::rounded(RgbaF32::hex(0x4cd3ff), Corners::all(9.0)))
                .clip(ClipMode::Rounded)
                .show(ui, |ui| {
                    ui.add_shape(Shape::rect(Rect::new(0.0, 0.0, 24.0, 12.0)).fill(RgbaF32::BLACK));
                });
            Text::new("Canvas").pos((22.0, 26.0)).show(ui);
            ui.add_shape(Shape::mesh(mesh));
            ui.add_shape(Shape::line(
                Vec2::new(22.5, 22.25),
                Vec2::new(70.1, 55.9),
                Stroke::new(RgbaF32::hex(0xffa63d), 1.5),
            ));
            ui.add_shape(Shape::rect(Rect::new(20.0, 20.0, 60.0, 40.0)).fill(tile));
        });
}

/// Five frames whose tile turns each time: the first whole, the rest partial, the first of them
/// through the backbuffer whole, as a direct frame left it stale. Painted as a device whose pass
/// moves its whole target, they paint a canvas; as one whose pass moves what it draws, the
/// backbuffer. Every frame's pixels match.
#[test]
fn a_canvas_paints_what_the_backbuffer_paints() {
    let gpu = headless_test_gpu();
    let mut mesh = Mesh::with_capacity(3, 3);
    let a = mesh.vertex(Vec2::new(40.3, 50.7), RgbaF32::hex(0xff5e44));
    let b = mesh.vertex(Vec2::new(55.6, 33.1), RgbaF32::hex(0xfacc15));
    let c = mesh.vertex(Vec2::new(68.9, 57.45), RgbaF32::hex(0x46c46c));
    mesh.triangle(a, b, c);
    let host = |traffic| {
        let mut host = OffscreenHost::builder(gpu.handles())
            .shaper(TextShaper::test_mono())
            .swapchain_present()
            .build();
        host.ui().theme_mut().window_clear = RgbaF32::BLACK;
        host.core.backend.paint_as(traffic);
        host
    };
    let mut on_backbuffer = host(PassTraffic::Drawn);
    let mut on_canvas = host(PassTraffic::WholeTarget);
    let backbuffer_target = gpu.target("palantir.test.canvas.backbuffer", UVec2::new(160, 120));
    let canvas_target = gpu.target("palantir.test.canvas.canvas", UVec2::new(160, 120));
    let tile = Cell::new(RgbaF32::srgba(1.0, 0.0, 0.0, 0.5));
    for frame in 0..5 {
        tile.set(RgbaF32::srgba(1.0, 0.2 * frame as f32, 0.0, 0.5));
        let mut app = RecordApp::new(|ui: &mut Ui| scene(ui, tile.get(), &mesh));
        let want = if frame == 0 {
            FramePaint::Full
        } else {
            FramePaint::Partial
        };
        let backbuffer_paint = on_backbuffer.frame(backbuffer_target.as_target(), 1.0, &mut app);
        let canvas_paint = on_canvas.frame(canvas_target.as_target(), 1.0, &mut app);
        assert_eq!(backbuffer_paint.paint(), want, "frame {frame}");
        assert_eq!(canvas_paint.paint(), want, "frame {frame}");
        assert!(
            backbuffer_target.read_back() == canvas_target.read_back(),
            "frame {frame}: the canvas paints other pixels"
        );
    }
    assert!(!on_backbuffer.driver.holds_canvas());
    assert!(
        on_canvas.driver.holds_canvas(),
        "the device that moves its whole target painted a canvas"
    );
}
