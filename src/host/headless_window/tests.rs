use crate::display::SCALE_RULE;
use crate::gpu::test_gpu::{headless_test_gpu, isolated_headless_test_gpu};
use crate::host::headless_window::HeadlessWindow;
use crate::internals::panic_probe;
use crate::internals::record_app::RecordApp;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::text::shaper::TextShaper;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widgets::block::Block;
use glam::UVec2;
use image::Rgba;

/// A white 2 × 1 logical block at the origin over a black clear. At scale 1 on 8 × 6 it covers
/// physical x 0..2, y 0..1; resized to 4 × 4 at scale 2 it covers x 0..4, y 0..2. Each read-back
/// is the frame just drawn, at the window's size.
#[test]
fn a_resize_reads_back_the_new_size_and_scale() {
    let gpu = headless_test_gpu();
    let mut window = HeadlessWindow::builder(gpu.handles())
        .shaper(TextShaper::test_mono())
        .open(UVec2::new(8, 6), 1.0);
    window.ui().theme_mut().window_clear = RgbaF32::BLACK;
    let mut app = RecordApp::new(|ui: &mut Ui| {
        Block::new()
            .id_salt("white")
            .size((2.0, 1.0))
            .chrome(Chrome::fill(RgbaF32::WHITE))
            .show(ui);
    });
    let white = |x0: u32, x1: u32, y0: u32, y1: u32| {
        move |x: u32, y: u32| (x0..x1).contains(&x) && (y0..y1).contains(&y)
    };
    for (size, scale, inside) in [
        (UVec2::new(8, 6), 1.0, white(0, 2, 0, 1)),
        (UVec2::new(4, 4), 2.0, white(0, 4, 0, 2)),
    ] {
        window.resize(size, scale);
        window.frame(&mut app);
        assert_eq!((window.size(), window.system_scale()), (size, scale));
        let image = window.read_back();
        assert_eq!(image.dimensions(), (size.x, size.y), "{size}");
        for (x, y, pixel) in image.enumerate_pixels() {
            let want = if inside(x, y) {
                Rgba([255; 4])
            } else {
                Rgba([0, 0, 0, 255])
            };
            assert_eq!(*pixel, want, "{size} at ({x}, {y})");
        }
    }

    panic_probe::assert_panics_with(SCALE_RULE, || {
        HeadlessWindow::builder(gpu.handles()).open(UVec2::splat(4), f32::NAN)
    });
}

/// A window whose device is lost reports it and recovers onto a new one, into a texture of its
/// size there, which the next frame paints whole: the white block at x 0..2, y 0..1 on black.
#[test]
fn a_window_recovers_onto_a_new_device() {
    let lost = isolated_headless_test_gpu();
    let mut window = HeadlessWindow::builder(lost.handles())
        .shaper(TextShaper::test_mono())
        .open(UVec2::new(8, 6), 1.0);
    window.ui().theme_mut().window_clear = RgbaF32::BLACK;
    let mut app = RecordApp::new(|ui: &mut Ui| {
        Block::new()
            .id_salt("white")
            .size((2.0, 1.0))
            .chrome(Chrome::fill(RgbaF32::WHITE))
            .show(ui);
    });
    window.frame(&mut app);
    lost.lose();
    assert!(window.is_device_lost());

    let gpu = headless_test_gpu();
    window.recover(gpu.handles());
    assert!(!window.is_device_lost());
    window.frame(&mut app);
    let image = window.read_back();
    assert_eq!(image.dimensions(), (8, 6));
    for (x, y, pixel) in image.enumerate_pixels() {
        let want = if x < 2 && y < 1 {
            Rgba([255; 4])
        } else {
            Rgba([0, 0, 0, 255])
        };
        assert_eq!(*pixel, want, "({x}, {y})");
    }
}
