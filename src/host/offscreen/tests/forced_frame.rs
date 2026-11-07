//! A forced frame paints a decision on a still screen, as a damage sweep times it.

use crate::damage::Damage;
use crate::damage::dirty_panes::DirtyPanes;
use crate::damage::region::DamageRegion;
use crate::gpu::test_gpu::headless_test_gpu;
use crate::host::offscreen::OffscreenHost;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::paint::backdrop_filter::BackdropFilter;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::shape::Shape;
use crate::text::shaper::TextShaper;
use crate::ui::Ui;
use crate::ui::frame_report::FramePaint;
use crate::widget_core::configure::Configure;
use crate::widgets::block::Block;
use crate::widgets::panel::Panel;
use glam::UVec2;

const SIZE: UVec2 = UVec2::new(160, 120);
/// A pane in the window's middle, past the reach of a change at its top-left corner.
const PANE: Rect = Rect::new(100.0, 70.0, 40.0, 30.0);

/// Two tiles over a backdrop, with an optional pane.
fn scene(ui: &mut Ui, pane: bool) {
    Panel::zstack()
        .id_salt("forced-scene")
        .size((Sizing::FILL, Sizing::FILL))
        .chrome(Chrome::fill(RgbaF32::hex(0x202830)))
        .show(ui, |ui| {
            Block::new()
                .id_salt("forced-tile")
                .size((Sizing::fixed(60.0), Sizing::fixed(40.0)))
                .chrome(Chrome::fill(RgbaF32::srgba(1.0, 0.2, 0.0, 0.5)))
                .show(ui);
            if pane {
                Panel::canvas()
                    .id_salt("forced-panes")
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui, |ui| {
                        ui.add_shape(
                            Shape::rect()
                                .at(PANE)
                                .fill(RgbaF32::TRANSPARENT)
                                .backdrop_filter(BackdropFilter::blur(2.0)),
                        );
                    });
            }
        });
}

fn partial(rects: &[Rect]) -> Damage {
    Damage::Partial(DamageRegion::from_unmerged(rects).unmeasured())
}

/// A still screen plans no paint, and each forced frame paints as forced and no longer: partial
/// past any coverage a promote would read, as it holds none, and whole. Its pixels stay the
/// screen's: over rects that overlap too, which the scissors keep disjoint, so nothing composites
/// twice.
#[test]
fn a_forced_frame_paints_its_decision_and_keeps_the_screen() {
    let gpu = headless_test_gpu();
    let mut host = OffscreenHost::builder(gpu.handles())
        .shaper(TextShaper::test_mono())
        .swapchain_present()
        .build();
    let target = gpu.target("palantir.test.forced", SIZE);
    let mut app = |ui: &mut Ui| scene(ui, false);
    assert_eq!(
        host.frame(target.as_target(), &mut app).paint(),
        FramePaint::Full
    );
    assert_eq!(
        host.frame(target.as_target(), &mut app).paint(),
        FramePaint::Skip
    );
    let still = target.read_back();

    let most = [Rect::new(0.0, 0.0, 160.0, 110.0)];
    let overlapping = [
        Rect::new(10.0, 10.0, 60.0, 60.0),
        Rect::new(40.0, 20.0, 60.0, 60.0),
    ];
    for rects in [&most[..], &overlapping[..]] {
        host.force_next_frame(partial(rects), rects);
        let forced = host.frame(target.as_target(), &mut app).paint();
        assert_eq!(forced, FramePaint::Partial, "{rects:?}");
        assert!(target.read_back() == still, "{rects:?}: other pixels");
        assert_eq!(
            host.frame(target.as_target(), &mut app).paint(),
            FramePaint::Skip,
            "the force lasts one frame"
        );
    }
    host.force_next_frame(Damage::Full, &[]);
    assert_eq!(
        host.frame(target.as_target(), &mut app).paint(),
        FramePaint::Full
    );
    assert!(target.read_back() == still, "a whole repaint: other pixels");
}

/// A change under a pane's footprint makes it capture again, as damage under it does; a change far
/// from it leaves its capture held, the merged rect reaching over it or not.
#[test]
fn a_forced_frame_stales_the_panes_its_change_touches() {
    let gpu = headless_test_gpu();
    let mut host = OffscreenHost::builder(gpu.handles())
        .shaper(TextShaper::test_mono())
        .swapchain_present()
        .build();
    let target = gpu.target("palantir.test.forced_pane", SIZE);
    let mut app = |ui: &mut Ui| scene(ui, true);
    host.frame(target.as_target(), &mut app);
    host.frame(target.as_target(), &mut app);
    let far = Rect::new(0.0, 0.0, 4.0, 4.0);
    let mut captures = |damage: Damage, changed: &[Rect]| {
        host.force_next_frame(damage, changed);
        host.frame(target.as_target(), &mut app);
        host.last_render_buffer()
            .backdrops
            .iter()
            .filter(|row| row.source.is_new())
            .count()
    };
    // The first partial frame after a direct one repaints whole, as the backbuffer is stale, and
    // captures the pane; from the next the capture holds.
    assert_eq!(
        captures(partial(&[far]), &[far]),
        1,
        "through a stale backbuffer"
    );
    assert_eq!(captures(partial(&[far]), &[far]), 0, "a far change");
    assert_eq!(
        captures(partial(&[Rect::new(0.0, 0.0, 160.0, 120.0)]), &[far]),
        0,
        "a far change under a merged rect that reaches the pane"
    );
    assert_eq!(
        captures(partial(&[PANE]), &[PANE]),
        1,
        "a change under the pane"
    );
    assert_eq!(
        captures(partial(&[far]), &[far]),
        0,
        "the mark lasts one frame"
    );
    captures(Damage::Full, &[PANE]);
    assert_eq!(
        *host.window.frames.damage().dirty_panes(),
        DirtyPanes::default(),
        "a whole frame reports no stale pane"
    );
}
