//! A shadow's corner tables, baked once and held in the atlas across frames, paint what a bake
//! from a clear paints.

use crate::gpu::test_gpu::headless_test_gpu;
use crate::host::offscreen::OffscreenHost;
use crate::internals::record_app::RecordApp;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::shadow::Shadow;
use crate::text::shaper::TextShaper;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widgets::block::Block;
use glam::{UVec2, Vec2};

/// Thirty shadowed blocks of one radius and blur, and three of another, shifted `x` px right:
/// enough of each key's corners that a table pays for its bake (`CutoutPlan`).
fn scene(ui: &mut Ui, x: f32) {
    let blocks = (0..30)
        .map(|i| (i, 6.0, 3.0, 40.0 * (i % 6) as f32, 40.0 * (i / 6) as f32))
        .chain((30..33).map(|i| (i, 10.0, 6.0, 260.0, 20.0 + 70.0 * (i - 30) as f32)));
    for (i, r, sigma, at_x, at_y) in blocks {
        Block::new()
            .id_salt(("shadowed", i))
            .pos((x + 12.0 + at_x, 12.0 + at_y))
            .size(22.0)
            .chrome(Chrome {
                fill: RgbaF32::hex(0x30364a).into(),
                corners: Corners::all(r),
                shadow: Shadow::drop(
                    RgbaF32::srgba(0.0, 0.0, 0.0, 0.7),
                    Vec2::new(1.0, 3.0),
                    sigma,
                ),
                ..Chrome::NONE
            })
            .show(ui);
    }
}

/// A host that painted the blocks at each of five places, its atlas holding their tables from
/// the frames before, paints the last place as a new host paints it, from a clear.
#[test]
fn a_held_table_paints_what_a_fresh_bake_paints() {
    let gpu = headless_test_gpu();
    let host = || {
        let mut host = OffscreenHost::builder(gpu.handles())
            .shaper(TextShaper::test_mono())
            .build();
        host.ui().theme_mut().window_clear = RgbaF32::WHITE;
        host
    };
    let size = UVec2::new(340, 240);
    let held = gpu.target("palantir.test.cutouts.held", size);
    let fresh = gpu.target("palantir.test.cutouts.fresh", size);
    let mut across = host();
    for step in 0..5 {
        let x = 1.25 * step as f32;
        across.frame(
            held.as_target(),
            1.0,
            &mut RecordApp::new(|ui: &mut Ui| scene(ui, x)),
        );
    }
    assert_eq!(
        across.core.backend.held_cutout_tables(),
        2,
        "a table for each key the blocks' corners share"
    );
    host().frame(
        fresh.as_target(),
        1.0,
        &mut RecordApp::new(|ui: &mut Ui| scene(ui, 5.0)),
    );
    assert!(
        held.read_back() == fresh.read_back(),
        "the held tables paint other pixels"
    );
}
