//! A forced frame paints a decision on a still screen, as a damage sweep times it.

use crate::damage::Damage;
use crate::damage::dirty_backdrops::DirtyBackdrops;
use crate::damage::region::DamageRegion;
use crate::gpu::test_gpu::headless_test_gpu;
use crate::host::offscreen::OffscreenHost;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::material::Material;
use crate::primitives::paint::paint::Paint;
use crate::renderer::render_buffer::backdrop_row::BackdropSource;
use crate::shape::Shape;
use crate::text::shaper::TextShaper;
use crate::ui::Ui;
use crate::ui::frame_report::FramePaint;
use crate::widget_core::configure::Configure;
use crate::widgets::block::Block;
use crate::widgets::panel::Panel;
use glam::UVec2;

const SIZE: UVec2 = UVec2::new(160, 120);
/// A backdrop in the window's middle, past the reach of a change at its top-left corner.
const BACKDROP: Rect = Rect::new(100.0, 70.0, 40.0, 30.0);

/// Two tiles over a backdrop, with an optional material.
fn scene(ui: &mut Ui, backdrop: bool) {
    Panel::zstack()
        .id_salt("forced-scene")
        .size((Sizing::FILL, Sizing::FILL))
        .chrome(Chrome::new(RgbaF32::hex(0x202830)))
        .show(ui, |ui| {
            Block::new()
                .id_salt("forced-tile")
                .size((Sizing::fixed(60.0), Sizing::fixed(40.0)))
                .chrome(Chrome::new(RgbaF32::srgba(1.0, 0.2, 0.0, 0.5)))
                .show(ui);
            if backdrop {
                Panel::canvas()
                    .id_salt("forced-backdrops")
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui, |ui| {
                        ui.add_shape(
                            Shape::rect().at(BACKDROP),
                            Paint::from(RgbaF32::TRANSPARENT).with_material(Material::blur(2.0)),
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

/// A change under a backdrop's footprint makes it capture again, as damage under it does; a change
/// far from it leaves its capture held, the merged rect reaching over it or not.
#[test]
fn a_forced_frame_stales_the_backdrops_its_change_touches() {
    let gpu = headless_test_gpu();
    let mut host = OffscreenHost::builder(gpu.handles())
        .shaper(TextShaper::test_mono())
        .swapchain_present()
        .build();
    let target = gpu.target("palantir.test.forced_backdrop", SIZE);
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
            .filter(|row| row.source().is_some_and(BackdropSource::is_new))
            .count()
    };
    // The first partial frame after a direct one repaints whole, as the backbuffer is stale, and
    // captures the backdrop; from the next the capture holds.
    assert_eq!(
        captures(partial(&[far]), &[far]),
        1,
        "through a stale backbuffer"
    );
    assert_eq!(captures(partial(&[far]), &[far]), 0, "a far change");
    assert_eq!(
        captures(partial(&[Rect::new(0.0, 0.0, 160.0, 120.0)]), &[far]),
        0,
        "a far change under a merged rect that reaches the backdrop"
    );
    assert_eq!(
        captures(partial(&[BACKDROP]), &[BACKDROP]),
        1,
        "a change under the backdrop"
    );
    assert_eq!(
        captures(partial(&[far]), &[far]),
        0,
        "the mark lasts one frame"
    );
    captures(Damage::Full, &[BACKDROP]);
    assert_eq!(
        *host.window.frames.damage().dirty_backdrops(),
        DirtyBackdrops::default(),
        "a whole frame reports no stale backdrop"
    );
}
