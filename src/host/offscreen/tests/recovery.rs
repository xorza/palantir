//! A lost device and the recovery onto another: every GPU object the host holds comes back, as a
//! fresh host builds it. The loss is real: `Device::destroy` on an isolated device, as wgpu's own
//! device-loss tests make it.

#![expect(
    clippy::disallowed_types,
    reason = "the view paints through wgpu, which only the gpu module names elsewhere"
)]

use crate::app::App;
use crate::gpu::device::gpu_frame_ctx::GpuFrameCtx;
use crate::gpu::device::gpu_init_ctx::GpuInitCtx;
use crate::gpu::surface::owned_target::OwnedTarget;
use crate::gpu::test_gpu::{
    HeadlessTestGpuLease, headless_test_gpu, isolated_headless_test_gpu,
    isolated_headless_test_gpu_capped,
};
use crate::host::offscreen::{OffscreenHost, OffscreenTime};
use crate::host::window_driver::CpuFrame;
use crate::icons::icon_set::IconSet;
use crate::icons::icon_table::IconTable;
use crate::icons::internals::{ONE_COLOR, TWO_COLORS};
use crate::image_registry::image_handle::ImageHandle;
use crate::internals::record_app::RecordApp;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::mesh::Mesh;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::layout::clip_mode::ClipMode;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::paint::backdrop_filter::BackdropFilter;
use crate::primitives::paint::brush::gradient::linear_geometry::LinearGradient;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::srgba_u8::SrgbaU8;
use crate::primitives::paint::image::Image;
use crate::primitives::paint::shadow::Shadow;
use crate::primitives::paint::stroke::Stroke;
use crate::renderer::gpu_paint::GpuPaint;
use crate::shape::Shape;
use crate::text::font_scope::FontScope;
use crate::text::shaper::TextShaper;
use crate::ui::Ui;
use crate::ui::frame_report::FramePaint;
use crate::widget_core::configure::Configure;
use crate::widgets::block::Block;
use crate::widgets::gpu_view::GpuView;
use crate::widgets::panel::Panel;
use crate::widgets::text::Text;
use crate::window::window_token::WindowToken;
use glam::{UVec2, Vec2};
use image::{RgbaImage, imageops};
use std::cell::RefCell;
use std::rc::Rc;

const SIZE: UVec2 = UVec2::new(160, 120);

/// A view that clears its target to one color, and keeps the device its `init` ran on.
#[derive(Debug, Default)]
struct ClearView {
    inits: u32,
    device: Option<wgpu::Device>,
}

impl GpuPaint for ClearView {
    fn init(&mut self, ctx: &GpuInitCtx<'_>) {
        self.inits += 1;
        self.device = Some(ctx.device.clone());
    }

    fn paint(&mut self, ctx: &mut GpuFrameCtx<'_>) {
        assert_eq!(
            self.device.as_ref(),
            Some(ctx.device),
            "a view paints on the device its init ran on"
        );
        ctx.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("palantir.test.recovery.view"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: ctx.target,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.0,
                        g: 0.5,
                        b: 0.25,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
    }
}

/// One scene that holds every kind of GPU object a host keeps: solid and gradient quads
/// (gradient atlas), four shadows of one key (cutout tables), a rounded clip (stencil), text
/// (glyph atlas), a mask and a color icon (raster atlases), a mesh, a curve, a polyline, a
/// registered image (image store), a `GpuView` (view target), and a backdrop pane (backdrop atlas
/// and blur). It fills its image again on the frame the device is restored.
#[derive(Debug)]
struct Scene {
    tile: RgbaF32,
    image: Image,
    handle: Option<ImageHandle>,
    icons: Rc<IconTable>,
    held: Option<IconSet>,
    view: Rc<RefCell<ClearView>>,
    mesh: Mesh,
    restored_frames: u32,
}

impl Scene {
    fn new() -> Self {
        let mut image = Image::blank(UVec2::new(6, 4));
        for (i, texel) in image.texels_mut().iter_mut().enumerate() {
            *texel = SrgbaU8::rgb((i * 10) as u8, 200, 255 - (i * 10) as u8);
        }
        let mut mesh = Mesh::with_capacity(3, 3);
        let a = mesh.vertex(Vec2::new(4.0, 116.0), RgbaF32::hex(0xff5e44));
        let b = mesh.vertex(Vec2::new(20.0, 90.0), RgbaF32::hex(0xfacc15));
        let c = mesh.vertex(Vec2::new(36.0, 116.0), RgbaF32::hex(0x46c46c));
        mesh.triangle(a, b, c);
        Self {
            tile: RgbaF32::WHITE,
            image,
            handle: None,
            icons: Rc::new(
                IconTable::from_svgs([("mask", ONE_COLOR), ("color", TWO_COLORS)]).unwrap(),
            ),
            held: None,
            view: Rc::default(),
            mesh,
            restored_frames: 0,
        }
    }
}

impl App for Scene {
    fn update(&mut self, _window: WindowToken, ui: &Ui) {
        if ui.device_restored() {
            self.restored_frames += 1;
            if let Some(handle) = &self.handle {
                handle.update(&self.image);
            }
        }
        if self.handle.is_none() {
            self.handle = Some(ui.load_image(&self.image).unwrap());
        }
    }

    fn record(&mut self, _window: WindowToken, ui: &mut Ui) {
        let icons = self.held.insert(ui.load_icons(Rc::clone(&self.icons)));
        let mask = icons.by_name("mask").unwrap();
        let color = icons.by_name("color").unwrap();
        let image = self.handle.clone().unwrap();
        Panel::canvas()
            .id_salt("scene")
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                ui.add_shape(Shape::rect(Rect::new(4.0, 4.0, 12.0, 12.0)).fill(self.tile));
                ui.add_shape(Shape::rect(Rect::new(20.0, 4.0, 40.0, 12.0)).fill(
                    LinearGradient::two_stop(0.0, RgbaF32::hex(0xff0000), RgbaF32::hex(0x0000ff)),
                ));
                for i in 0..4 {
                    Block::new()
                        .id_salt(("shadowed", i))
                        .pos((4.0 + 24.0 * i as f32, 24.0))
                        .size(16.0)
                        .chrome(Chrome {
                            fill: RgbaF32::hex(0x30364a).into(),
                            corners: Corners::all(4.0),
                            shadow: Shadow::drop(
                                RgbaF32::srgba(0.0, 0.0, 0.0, 0.75),
                                Vec2::new(0.0, 2.0),
                                4.0,
                            ),
                            ..Chrome::NONE
                        })
                        .show(ui);
                }
                Panel::canvas()
                    .id_salt("rounded")
                    .pos((100.0, 4.0))
                    .size(28.0)
                    .chrome(Chrome::rounded(RgbaF32::hex(0x4cd3ff), Corners::all(10.0)))
                    .clip(ClipMode::Rounded)
                    .show(ui, |ui| {
                        ui.add_shape(
                            Shape::rect(Rect::new(0.0, 0.0, 28.0, 14.0)).fill(RgbaF32::BLACK),
                        );
                    });
                Text::new("Recover").pos((4.0, 48.0)).show(ui);
                ui.add_shape(Shape::icon(mask).at(Rect::new(60.0, 48.0, 24.0, 12.0)));
                ui.add_shape(Shape::icon(color).at(Rect::new(88.0, 48.0, 16.0, 16.0)));
                ui.add_shape(Shape::mesh(&self.mesh));
                ui.add_shape(Shape::line(
                    Vec2::new(44.0, 90.0),
                    Vec2::new(80.0, 110.0),
                    Stroke::new(RgbaF32::hex(0xffa63d), 3.0),
                ));
                ui.add_shape(Shape::polyline(
                    &[
                        Vec2::new(84.0, 110.0),
                        Vec2::new(96.0, 90.0),
                        Vec2::new(108.0, 110.0),
                    ],
                    Stroke::new(RgbaF32::hex(0xd897ff), 2.0),
                ));
                ui.add_shape(Shape::image(image).at(Rect::new(112.0, 48.0, 12.0, 8.0)));
                GpuView::new(&self.view)
                    .id_salt("view")
                    .pos((112.0, 64.0))
                    .size(20.0)
                    .show(ui);
                ui.add_shape(
                    Shape::rect(Rect::new(96.0, 60.0, 40.0, 30.0))
                        .backdrop_filter(BackdropFilter::blur(2.0)),
                );
            });
    }
}

/// A host on `gpu` as the recovered one is built: real text, GPU timing, manual time.
fn host_on(gpu: &HeadlessTestGpuLease, shaper: &TextShaper) -> OffscreenHost {
    OffscreenHost::builder(gpu.handles())
        .shaper(shaper.clone())
        .collect_gpu_stats(true)
        .time(OffscreenTime::Manual)
        .build()
}

/// Two frames of `scene` on `host`, the first whole and the second partial after the tile turns,
/// each read back.
fn paint_twice(
    host: &mut OffscreenHost,
    scene: &mut Scene,
    target: &OwnedTarget,
) -> [RgbaImage; 2] {
    [RgbaF32::WHITE, RgbaF32::hex(0xd23f7a)].map(|tile| {
        scene.tile = tile;
        let report = host.frame(target.as_target(), 1.0, scene);
        let want = if tile == RgbaF32::WHITE {
            FramePaint::Full
        } else {
            FramePaint::Partial
        };
        assert_eq!(report.paint(), want);
        target.read_back()
    })
}

/// `host`, recovered onto `device`, paints what a fresh host on `device` paints, pixel for pixel,
/// in its first frame and in the partial frame after, which proves its backbuffer new; the
/// recovery showed to the app in the first frame alone, and the view's init ran again on
/// `device`.
fn assert_paints_as_fresh(
    host: &mut OffscreenHost,
    scene: &mut Scene,
    device: &HeadlessTestGpuLease,
    shaper: &TextShaper,
) {
    let restored = scene.restored_frames;
    let inits = scene.view.borrow().inits;
    let target = device.target("palantir.test.recovery", SIZE);
    let got = paint_twice(host, scene, &target);
    let mut fresh = host_on(device, shaper);
    let want = paint_twice(
        &mut fresh,
        &mut Scene::new(),
        &device.target("palantir.test.recovery.fresh", SIZE),
    );
    assert_eq!(got, want);
    assert_eq!(scene.restored_frames, restored + 1);
    let view = scene.view.borrow();
    assert_eq!(view.inits, inits + 1);
    assert_eq!(view.device.as_ref(), Some(&device.device));
}

/// Startup and a recovery are one flow: a host attached again to the device it holds paints what
/// it painted before, whole and then partial, and the attach shows to the app once. No loss is
/// needed for the attach steps to run.
#[test]
fn attaching_the_same_device_again_paints_as_before() {
    let device = headless_test_gpu();
    let mut host = host_on(&device, &TextShaper::with_fonts(FontScope::Bundled));
    let mut scene = Scene::new();
    let target = device.target("palantir.test.recovery.again", SIZE);
    let before = paint_twice(&mut host, &mut scene, &target);
    assert_eq!(scene.restored_frames, 0, "a first attach restores nothing");
    host.recover(device.handles());
    assert_eq!(paint_twice(&mut host, &mut scene, &target), before);
    assert_eq!(scene.restored_frames, 1);
    assert_eq!(scene.view.borrow().inits, 2);
}

/// A loss between two frames, which the next frame finds: that frame runs the app and submits
/// nothing, and the host recovered onto a new device paints as a fresh host on it.
#[test]
fn a_host_recovers_from_a_loss_between_frames() {
    let shaper = TextShaper::with_fonts(FontScope::Bundled);
    let lost = isolated_headless_test_gpu();
    let mut host = host_on(&lost, &shaper);
    let mut scene = Scene::new();
    let target = lost.target("palantir.test.recovery.lost", SIZE);
    host.frame(target.as_target(), 1.0, &mut scene);
    lost.lose();
    assert!(host.is_device_lost());
    host.frame(target.as_target(), 1.0, &mut scene);
    assert!(!host.driver.output_valid(), "the frame submitted nothing");
    assert_eq!(scene.restored_frames, 0);

    let device = headless_test_gpu();
    host.recover(device.handles());
    assert!(!host.is_device_lost());
    assert_paints_as_fresh(&mut host, &mut scene, &device, &shaper);
}

/// A loss inside a frame: the device is lost after the frame's CPU half and its GPU half runs on
/// it, as the rest of a submit runs after the call that finds a real loss, and stages, copies and
/// submits nothing that panics. It is the host's second loss, after one between frames, and the
/// second recovery paints as a fresh host does too.
#[test]
fn a_host_recovers_from_a_second_loss_inside_a_frame() {
    let shaper = TextShaper::with_fonts(FontScope::Bundled);
    let first = isolated_headless_test_gpu();
    let mut host = host_on(&first, &shaper);
    let mut scene = Scene::new();
    host.frame(
        first
            .target("palantir.test.recovery.first", SIZE)
            .as_target(),
        1.0,
        &mut scene,
    );
    first.lose();

    let second = isolated_headless_test_gpu();
    host.recover(second.handles());
    let lost = second.target("palantir.test.recovery.second", SIZE);
    let spec = host.spec(lost.as_target(), 1.0);
    let CpuFrame { mode, .. } = host.core.record(&mut host.driver, spec, &mut scene);
    second.lose();
    host.core.submit(&mut host.driver, lost.as_target(), mode);
    assert!(host.is_device_lost());
    assert_eq!(scene.restored_frames, 1);

    let device = headless_test_gpu();
    host.recover(device.handles());
    assert_paints_as_fresh(&mut host, &mut scene, &device, &shaper);
}

/// A screenshot whose copy is on its way back when the device is lost lands after the recovery,
/// taken again from the first frame on the new device: its image is that frame's pixels.
#[test]
fn a_screenshot_asked_before_the_loss_lands_after_it() {
    let first = isolated_headless_test_gpu();
    let mut host = OffscreenHost::builder(first.handles())
        .shaper(TextShaper::test_mono())
        .build();
    let region = Rect::new(2.0, 2.0, 8.0, 6.0);
    let asked = RefCell::new(None);
    let mut app = RecordApp::new(|ui: &mut Ui| {
        Block::new()
            .id_salt("tile")
            .pos((4.0, 4.0))
            .size(4.0)
            .chrome(Chrome::fill(RgbaF32::hex(0xd23f7a)))
            .show(ui);
        if asked.borrow().is_none() {
            *asked.borrow_mut() = ui.request_screenshot(Some(region));
        }
    });
    let size = UVec2::new(16, 12);
    host.frame(
        first
            .target("palantir.test.recovery.shot", size)
            .as_target(),
        1.0,
        &mut app,
    );
    let id = asked.borrow().expect("the ask holds pixels");
    first.lose();

    let second = headless_test_gpu();
    host.recover(second.handles());
    let target = second.target("palantir.test.recovery.shot", size);
    host.frame(target.as_target(), 1.0, &mut app);
    assert!(
        host.ui().screenshot_taken(id).is_none(),
        "its copy is on its way"
    );
    let pixels = target.read_back();
    host.wait_for_gpu();
    host.frame(target.as_target(), 1.0, &mut app);
    let shot = host
        .ui()
        .screenshot_taken(id)
        .expect("the ask lands from the new device");
    let crop = imageops::crop_imm(&pixels, 2, 2, 8, 6).to_image();
    assert_eq!(shot.image().pixels, crop.into_raw());
}

/// A recovery onto a smaller device takes its limit: `max_image_dimension` reads 2048, the
/// floor Palantir runs on; an image registered at 4096 × 1 on the larger device no longer fits,
/// so its update writes nothing, while a 2048 × 1 one fits and is written; a new 4096 × 1 image
/// is refused; and the gradient atlas grows to 2048 rows at most.
#[test]
fn a_recovery_onto_a_smaller_device_takes_its_limit() {
    let large = headless_test_gpu();
    let mut host = OffscreenHost::builder(large.handles())
        .shaper(TextShaper::test_mono())
        .build();
    let wide = Image::blank(UVec2::new(4096, 1));
    let fits = Image::blank(UVec2::new(2048, 1));
    let wide_handle = host.ui().load_image(&wide).unwrap();
    let fits_handle = host.ui().load_image(&fits).unwrap();
    assert_eq!(host.gpu_image_cache_len(), 2);

    let small = isolated_headless_test_gpu_capped(2048);
    host.recover(small.handles());
    assert_eq!(
        host.gpu_image_cache_len(),
        0,
        "the textures went with the old device"
    );
    assert_eq!(host.ui().max_image_dimension().unwrap().get(), 2048);
    wide_handle.update(&wide);
    fits_handle.update(&fits);
    assert_eq!(host.gpu_image_cache_len(), 1);
    assert!(host.ui().load_image(&wide).is_err());
    assert_eq!(host.core.resources.gradient_atlas().max_rows(), 2048);
}
