//! Which path a frame paints by, from the scene the present path reads: the share a partial frame
//! stays partial under follows the rows and the panes on the surface.

use crate::damage::Damage;
use crate::gpu::test_gpu::headless_test_gpu;
use crate::host::offscreen::OffscreenHost;
use crate::host::window_driver::{CpuFrame, PresentPath};
use crate::primitives::geometry::rect::Rect;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::paint::backdrop_filter::BackdropFilter;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::shape::Shape;
use crate::text::shaper::TextShaper;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widgets::block::Block;
use crate::widgets::panel::Panel;
use glam::{UVec2, Vec2};
use std::cell::Cell;

/// 0.08 Mpx, on a logical window of 80,000 px² at scale 1.
const SIZE: UVec2 = UVec2::new(400, 200);

/// A scene of the screen a frame reads the density of.
#[derive(Clone, Copy, Debug)]
struct Screen {
    /// Small blocks in the corner away from what changes.
    filler_count: u32,
    /// A pane over the window's right half.
    pane: bool,
}

/// A 100 × 80 block that flips, 10 % of the window; a 4 × 4 one that flips; and `screen` around them.
fn record(ui: &mut Ui, screen: Screen, big: RgbaF32, tiny: RgbaF32) {
    let block = |ui: &mut Ui, salt: (&str, u32), at: Vec2, side: Vec2, fill: RgbaF32| {
        Block::new()
            .id_salt(salt)
            .at(at)
            .size((Sizing::fixed(side.x), Sizing::fixed(side.y)))
            .chrome(Chrome::fill(fill))
            .show(ui);
    };
    Panel::canvas()
        .id_salt("promote-scene")
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            block(ui, ("big", 0), Vec2::ZERO, Vec2::new(100.0, 80.0), big);
            block(
                ui,
                ("tiny", 0),
                Vec2::new(110.0, 0.0),
                Vec2::splat(4.0),
                tiny,
            );
            for i in 0..screen.filler_count {
                let at = Vec2::new((i % 50) as f32 * 3.0, 100.0 + (i / 50) as f32 * 3.0);
                block(ui, ("filler", i), at, Vec2::splat(2.0), RgbaF32::WHITE);
            }
            if screen.pane {
                ui.add_shape(
                    Shape::rect()
                        .at(Rect::new(220.0, 0.0, 180.0, 200.0))
                        .fill(RgbaF32::TRANSPARENT)
                        .backdrop_filter(BackdropFilter::blur(2.0)),
                );
            }
        });
}

/// The path of the frame that flips the big block, on a backbuffer the frame before left fresh.
fn path_of_a_tenth(screen: Screen) -> PresentPath {
    let gpu = headless_test_gpu();
    let mut host = OffscreenHost::builder(gpu.handles())
        .shaper(TextShaper::test_mono())
        .swapchain_present()
        .build();
    let target = gpu.target("palantir.test.promote", SIZE);
    let big = Cell::new(RgbaF32::hex(0x204060));
    let tiny = Cell::new(RgbaF32::hex(0x602040));
    let mut app = |ui: &mut Ui| record(ui, screen, big.get(), tiny.get());
    let frame = |host: &mut OffscreenHost, app: &mut dyn FnMut(&mut Ui)| {
        let spec = host.spec(target.as_target());
        let mut app = |ui: &mut Ui| app(ui);
        let CpuFrame { mode, .. } = host.record(spec, &mut app);
        host.submit(target.as_target(), mode);
        mode
    };
    frame(&mut host, &mut app);
    // A small change, which paints through the backbuffer and leaves it fresh.
    tiny.set(RgbaF32::hex(0x20A040));
    frame(&mut host, &mut app);
    big.set(RgbaF32::hex(0xA04020));
    frame(&mut host, &mut app)
}

/// The same 10 % damage, on three screens. Two rows on 0.08 Mpx read a thin scene, so a whole frame
/// is cheaper and it paints direct. Four hundred rows more read a dense one, and it stays partial.
/// A pane over half of the thin scene counts for a whole frame's captures, and it stays partial too.
#[test]
fn the_scene_decides_how_a_partial_frame_paints() {
    let thin = Screen {
        filler_count: 0,
        pane: false,
    };
    assert!(matches!(
        path_of_a_tenth(thin),
        PresentPath::Direct(plan) if plan.damage == Damage::Full
    ));
    for screen in [
        Screen {
            filler_count: 400,
            pane: false,
        },
        Screen {
            filler_count: 0,
            pane: true,
        },
    ] {
        let path = path_of_a_tenth(screen);
        assert!(
            matches!(
                path,
                PresentPath::ViaBackbuffer(plan) if matches!(plan.damage, Damage::Partial(_))
            ),
            "{screen:?}: {path:?}"
        );
    }
}
