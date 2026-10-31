//! The offscreen host's frames, targets and screenshots.

use std::cell::Cell;

use super::*;
use crate::damage::Damage;
use crate::diagnostics::DebugOverlayConfig;
use crate::gpu::test_gpu::headless_test_gpu;
use crate::host::window_driver::DriverConfig;
use crate::host::window_driver::PresentPath;
use crate::internals::panic_probe;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::paint::backdrop_filter::BackdropFilter;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::renderer::render_plan::RenderPlan;
use crate::shape::Shape;
use crate::ui::resources::UiResources;
use crate::widget_core::configure::Configure;
use crate::widgets::block::Block;
use crate::widgets::panel::Panel;
use crate::window::cursor_icon::CursorIcon;
use crate::window::vsync::Vsync;
use crate::window::window_config::WindowConfig;
use glam::UVec2;
use image::{Rgba, imageops};

/// A driver as this host opens its one window, over `shared`, on a stream with no device.
fn driver(token: WindowToken, shared: &UiResources) -> WindowDriver {
    WindowDriver::new(
        token,
        shared,
        DriverConfig {
            strategy: PresentStrategy::BackbufferCopy,
            pixel_snap: true,
        },
    )
}

/// A host with no window lifecycle drains a quiet frame as a windowed one does
/// (the veto lives a frame either way) and keeps the *levels* the recorder
/// reads back.
#[test]
fn deny_window_commands_accepts_a_quiet_frame_and_clears_the_veto() {
    let shared = UiResources::isolated_mono();
    let mut quiet = driver(WindowToken(1), &shared);
    quiet.ui.keep_open();
    quiet.ui.set_vsync(Vsync::Off);
    quiet.ui.set_cursor(CursorIcon::Text);

    deny_window_commands(&mut quiet);

    assert!(
        !quiet.ui.window_requests().close_vetoed,
        "a veto against a close that was never requested must not persist"
    );
    assert_eq!(
        quiet.ui.vsync(),
        Vsync::Off,
        "a level the host cannot apply is still the one the app set",
    );
    assert_eq!(quiet.ui.window_requests().levels.cursor, CursorIcon::Text);
}

#[test]
#[should_panic(expected = "Ui::open_window(WindowToken(9))")]
fn deny_window_commands_rejects_an_open() {
    let shared = UiResources::isolated_mono();
    let mut opener = driver(WindowToken(1), &shared);
    opener
        .ui
        .open_window(WindowToken(9), WindowConfig::new("unservable"));

    deny_window_commands(&mut opener);
}

#[test]
#[should_panic(expected = "Ui::close_window(WindowToken(4))")]
fn deny_window_commands_rejects_a_close() {
    let shared = UiResources::isolated_mono();
    let mut closer = driver(WindowToken(1), &shared);
    closer.ui.close_window(WindowToken(4));

    deny_window_commands(&mut closer);
}

/// A manual host stamps each frame with the time it was moved to: a wake asked for 1 s on
/// is due at 2 s + 1 s = 3 s after `at(2 s)`, and at 3.5 s + 1 s = 4.5 s after a further
/// `advance(1.5 s)`, which passes the 3 s one.
#[test]
fn a_manual_host_stamps_frames_with_the_time_it_was_moved_to() {
    let gpu = headless_test_gpu();
    let mut host = OffscreenHost::builder(gpu.handles())
        .shaper(TextShaper::test_mono())
        .time(OffscreenTime::Manual)
        .build();
    let texture = gpu.target("palantir.offscreen.time", UVec2::new(8, 8));
    let mut app = |ui: &mut Ui| ui.request_repaint_after(Duration::from_secs(1));
    let mut wake =
        |host: &mut OffscreenHost| host.frame(texture.as_target(), 1.0, &mut app).repaint_after;
    assert_eq!(
        wake(&mut host),
        Some(Duration::from_secs(1)),
        "it starts at zero"
    );
    host.at(Duration::from_secs(2));
    assert_eq!(wake(&mut host), Some(Duration::from_secs(3)));
    host.advance(Duration::from_millis(1500));
    assert_eq!(wake(&mut host), Some(Duration::from_millis(4500)));

    let mut realtime = OffscreenHost::builder(gpu.handles())
        .shaper(TextShaper::test_mono())
        .build();
    panic_probe::assert_panics_with("OffscreenTime::Manual", || {
        realtime.advance(Duration::from_secs(1));
    });
}

/// A retained target's partial frame paints into the target, loading what it holds: a pixel
/// the frame does not damage keeps what was written into the texture after the last frame,
/// so nothing cleared or copied over it, and the damaged 4 × 4 block takes its new color. A
/// still frame then presents nothing.
#[test]
fn a_retained_partial_frame_repaints_the_target_in_place() {
    let gpu = headless_test_gpu();
    let mut host = OffscreenHost::builder(gpu.handles())
        .shaper(TextShaper::test_mono())
        .retained_target(true)
        .build();
    host.ui().theme_mut().window_clear = RgbaF32::BLACK;
    let texture = gpu.target("palantir.offscreen.retained", UVec2::new(32, 8));
    let color = Cell::new(RgbaF32::WHITE);
    let mut app = |ui: &mut Ui| {
        Block::new()
            .id_salt("tile")
            .size(4.0)
            .chrome(Chrome::fill(color.get()))
            .show(ui);
    };
    let mut frame = |host: &mut OffscreenHost| {
        let target = texture.as_target();
        let spec = host.spec(target, 1.0);
        let CpuFrame { mode, .. } =
            host.core
                .record(&mut host.driver, spec, &mut app, FrameDepth::Paint);
        host.core.submit(&mut host.driver, target, mode);
        mode
    };
    assert!(matches!(
        frame(&mut host),
        PresentPath::Direct(RenderPlan {
            damage: Damage::Full,
            ..
        })
    ));
    gpu.queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: texture.as_target().texture(),
            mip_level: 0,
            origin: wgpu::Origin3d { x: 30, y: 6, z: 0 },
            aspect: wgpu::TextureAspect::All,
        },
        &[0, 255, 0, 255],
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4),
            rows_per_image: Some(1),
        },
        wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
    );
    color.set(RgbaF32::new(1.0, 0.0, 0.0, 1.0));
    assert!(matches!(
        frame(&mut host),
        PresentPath::Direct(RenderPlan {
            damage: Damage::Partial(_),
            ..
        })
    ));
    let image = texture.read_back();
    assert_eq!(
        *image.get_pixel(1, 1),
        Rgba([255, 0, 0, 255]),
        "the damaged block"
    );
    assert_eq!(
        *image.get_pixel(30, 6),
        Rgba([0, 255, 0, 255]),
        "the kept marker"
    );
    assert_eq!(
        *image.get_pixel(20, 2),
        Rgba([0, 0, 0, 255]),
        "the kept clear"
    );
    assert_eq!(frame(&mut host), PresentPath::SkipNoop, "a still frame");
}

/// A retained target never keeps a damage outline: with the outline on, a partial frame
/// paints through the backbuffer, whole after a direct frame and partial after one through
/// it, and the first partial frame after the outline goes off still does, as the target
/// holds the last outline. The frame after that paints in place, and the target then matches
/// a host that never outlined.
#[test]
fn a_retained_target_keeps_no_damage_outline() {
    let gpu = headless_test_gpu();
    let host = |retained: bool| {
        let mut host = OffscreenHost::builder(gpu.handles())
            .shaper(TextShaper::test_mono())
            .retained_target(retained)
            .build();
        host.ui().theme_mut().window_clear = RgbaF32::BLACK;
        host
    };
    let mut retained = host(true);
    let texture = gpu.target("palantir.offscreen.outlined", UVec2::new(32, 16));
    let scene = |red: bool| {
        move |ui: &mut Ui| {
            let color = if red {
                RgbaF32::new(1.0, 0.0, 0.0, 1.0)
            } else {
                RgbaF32::WHITE
            };
            Block::new()
                .id_salt("tile")
                .size(4.0)
                .chrome(Chrome::fill(color))
                .show(ui);
        }
    };
    for (frame, outline, via_backbuffer, partial) in [
        (0, true, false, false),
        (1, true, true, false),
        (2, true, true, true),
        (3, false, true, true),
        (4, false, false, true),
    ] {
        retained.ui().set_debug_overlay(DebugOverlayConfig {
            damage_rect: outline,
            ..DebugOverlayConfig::default()
        });
        let target = texture.as_target();
        let spec = retained.spec(target, 1.0);
        let mut app = scene(frame % 2 == 1);
        let CpuFrame { mode, .. } =
            retained
                .core
                .record(&mut retained.driver, spec, &mut app, FrameDepth::Paint);
        let what = format!("frame {frame}: {mode:?}");
        let (route, plan) = match mode {
            PresentPath::ViaBackbuffer(plan) => (true, plan),
            PresentPath::Direct(plan) => (false, plan),
            PresentPath::SkipCopy | PresentPath::SkipNoop => panic!("{what}"),
        };
        assert_eq!(route, via_backbuffer, "{what}");
        assert_eq!(matches!(plan.damage, Damage::Partial(_)), partial, "{what}");
        retained.core.submit(&mut retained.driver, target, mode);
    }
    let reference = gpu.target("palantir.offscreen.unoutlined", UVec2::new(32, 16));
    host(false).frame(reference.as_target(), 1.0, &mut scene(false));
    assert_eq!(texture.read_back(), reference.read_back());
}

/// On a retained target that gives no copy, a partial frame whose pane captures paints
/// through the backbuffer, decided before it builds: as a partial frame over the backbuffer
/// the first frame left, then whole once a direct frame left the backbuffer behind. A partial
/// frame whose pane holds its result paints in place.
#[test]
fn a_retained_target_without_copies_captures_through_the_backbuffer() {
    let gpu = headless_test_gpu();
    let mut host = OffscreenHost::builder(gpu.handles())
        .shaper(TextShaper::test_mono())
        .retained_target(true)
        .build();
    let texture = gpu.uncopied_target("palantir.offscreen.uncopied", UVec2::new(64, 32));
    let scene = |under: bool, away: bool| {
        let color = |on: bool| if on { RgbaF32::WHITE } else { RgbaF32::BLACK };
        move |ui: &mut Ui| {
            Panel::canvas()
                .id_salt("uncopied")
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    ui.add_shape(Shape::rect(Rect::new(8.0, 8.0, 4.0, 4.0)).fill(color(under)));
                    ui.add_shape(Shape::rect(Rect::new(50.0, 20.0, 4.0, 4.0)).fill(color(away)));
                    ui.add_shape(
                        Shape::rect(Rect::new(4.0, 4.0, 16.0, 12.0))
                            .backdrop_filter(BackdropFilter::blur(1.0)),
                    );
                });
        }
    };
    for (under, away, via_backbuffer, partial) in [
        (false, false, true, false),
        (true, false, true, true),
        (true, true, false, true),
        (false, true, true, false),
    ] {
        let target = texture.as_target();
        let spec = host.spec(target, 1.0);
        let mut app = scene(under, away);
        let CpuFrame { mode, .. } =
            host.core
                .record(&mut host.driver, spec, &mut app, FrameDepth::Paint);
        let what = format!("under {under}, away {away}: {mode:?}");
        let (route, plan) = match mode {
            PresentPath::ViaBackbuffer(plan) => (true, plan),
            PresentPath::Direct(plan) => (false, plan),
            PresentPath::SkipCopy | PresentPath::SkipNoop => panic!("{what}"),
        };
        assert_eq!(route, via_backbuffer, "{what}");
        assert_eq!(matches!(plan.damage, Damage::Partial(_)), partial, "{what}");
        host.core.submit(&mut host.driver, target, mode);
    }
}

/// The CPU half leaves a frame's output pending and the GPU half completes it.
#[test]
fn the_gpu_half_completes_what_the_cpu_half_leaves_pending() {
    let gpu = headless_test_gpu();
    let mut host = OffscreenHost::builder(gpu.handles())
        .shaper(TextShaper::test_mono())
        .build();
    let texture = gpu.target("palantir.offscreen.validity", UVec2::new(64, 48));
    let mut app = |ui: &mut Ui| {
        Block::new()
            .id_salt("tile")
            .size(10.0)
            .chrome(Chrome::fill(RgbaF32::WHITE))
            .show(ui);
    };

    for (frame, paints) in [(0, true), (1, false)] {
        let target = texture.as_target();
        let spec = host.spec(target, 1.0);
        let CpuFrame { mode, .. } =
            host.core
                .record(&mut host.driver, spec, &mut app, FrameDepth::Paint);
        assert_eq!(
            matches!(mode, PresentPath::Direct(_) | PresentPath::ViaBackbuffer(_)),
            paints,
            "frame {frame}: {mode:?}",
        );
        if !paints {
            assert_eq!(mode, PresentPath::SkipCopy, "a still frame re-presents");
        }
        assert!(
            !host.driver.output_valid(),
            "frame {frame}: {mode:?} is pending until the GPU half runs",
        );
        host.core.submit(&mut host.driver, target, mode);
        assert!(
            host.driver.output_valid(),
            "frame {frame}: {mode:?} is complete once submitted",
        );
    }
}
/// A frame asks for a region but is never presented, as an outdated swapchain leaves it, and
/// the window shrinks from 64 × 48 to 32 × 24 before a frame paints. The request carries to
/// that frame and is resolved against its window: (20, 10) + 30 × 20 clamps to
/// (20, 10) + 12 × 14, and the image is that crop of the small target.
#[test]
fn a_screenshot_request_carries_to_the_frame_that_paints_it() {
    let gpu = headless_test_gpu();
    let mut host = OffscreenHost::builder(gpu.handles())
        .shaper(TextShaper::test_mono())
        .build();
    let asked = Cell::new(None);
    let scene = |ui: &mut Ui, ask: bool| {
        Block::new()
            .id_salt("tile")
            .pos((18.0, 8.0))
            .size(10.0)
            .chrome(Chrome::fill(RgbaF32::hex(0xd23f7a)))
            .show(ui);
        if ask {
            asked.set(ui.request_screenshot(Some(Rect::new(20.0, 10.0, 30.0, 20.0))));
        }
    };

    let large = gpu.target("palantir.offscreen.capture.large", UVec2::new(64, 48));
    let spec = host.spec(large.as_target(), 1.0);
    let mut asking = |ui: &mut Ui| scene(ui, true);
    let CpuFrame { report, .. } =
        host.core
            .record(&mut host.driver, spec, &mut asking, FrameDepth::Paint);
    assert!(
        report.repaint_requested,
        "the unpainted request asks for a frame"
    );

    let small = gpu.target("palantir.offscreen.capture.small", UVec2::new(32, 24));
    let mut still = |ui: &mut Ui| scene(ui, false);
    host.frame(small.as_target(), 1.0, &mut still);
    let target = small.read_back();
    host.wait_for_gpu();
    host.frame(small.as_target(), 1.0, &mut still);
    let shot = host
        .ui()
        .screenshot_taken(asked.get().unwrap())
        .expect("the carried request lands once the device finishes its copy");
    assert_eq!(shot.region(), Rect::new(20.0, 10.0, 12.0, 14.0));
    let image = shot.image();
    assert_eq!(image.size(), UVec2::new(12, 14));
    let crop = imageops::crop_imm(&target, 20, 10, 12, 14).to_image();
    assert_eq!(image.pixels, crop.into_raw());
}

/// A carried request whose region the window shrank wholly past has no pixel to copy: the
/// frame that resolves it copies nothing, and no image lands. (40, 30) + 10 × 10 is inside
/// 64 × 48 and outside 32 × 24.
#[test]
fn a_carried_request_the_window_shrank_past_lands_nothing() {
    let gpu = headless_test_gpu();
    let mut host = OffscreenHost::builder(gpu.handles())
        .shaper(TextShaper::test_mono())
        .build();
    let asked = Cell::new(None);
    let large = gpu.target("palantir.offscreen.capture.large", UVec2::new(64, 48));
    let spec = host.spec(large.as_target(), 1.0);
    let mut asking = |ui: &mut Ui| {
        asked.set(ui.request_screenshot(Some(Rect::new(40.0, 30.0, 10.0, 10.0))));
    };
    let _ = host
        .core
        .record(&mut host.driver, spec, &mut asking, FrameDepth::Paint);
    let id = asked
        .get()
        .expect("the region has pixels in the large window");

    let small = gpu.target("palantir.offscreen.capture.small", UVec2::new(32, 24));
    let mut still = |_: &mut Ui| {};
    host.frame(small.as_target(), 1.0, &mut still);
    host.wait_for_gpu();
    host.frame(small.as_target(), 1.0, &mut still);
    assert!(host.ui().screenshot_taken(id).is_none());
}

mod canvas;
mod held_cutouts;
mod recovery;
