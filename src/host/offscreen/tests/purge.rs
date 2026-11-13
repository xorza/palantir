//! A purge between frames, as the system warns of low memory, gives the glyphs and the backdrop
//! results no frame drew back, and the next frame paints as before it.

use glam::UVec2;

use crate::gpu::atlas::atlas_format::AtlasFormat;
use crate::gpu::surface::owned_target::OwnedTarget;
use crate::gpu::test_gpu::headless_test_gpu;
use crate::host::offscreen::OffscreenHost;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::material::Material;
use crate::primitives::paint::paint::Paint;
use crate::shape::Shape;
use crate::text::shaper::TextShaper;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widgets::block::Block;
use crate::widgets::panel::Panel;
use crate::widgets::text::Text;

/// `text` at the top left of the window.
fn scene(ui: &mut Ui, text: &str) {
    Text::new(text).id_salt("purged").at((4.0, 4.0)).show(ui);
}

/// A frame of a pangram, then a frame of "Hi": the mask pages hold both frames' glyphs. A purge
/// keeps the glyphs the last frame drew alone, so the pages' live bytes fall, and the next frame
/// of "Hi" paints the pixels it painted before, with no glyph rasterized again.
#[test]
fn a_purge_keeps_the_last_frames_glyphs_and_paints_as_before() {
    let gpu = headless_test_gpu();
    let mut host = OffscreenHost::builder(gpu.handles())
        .shaper(TextShaper::new())
        .build();
    host.ui_mut().theme_mut().window_bg = RgbaF32::WHITE;
    let target = gpu.target("palantir.test.purge", UVec2::new(400, 40));
    let mask_live = |host: &OffscreenHost| host.atlas_memory().uses[0].live_bytes.unwrap();
    host.frame(target.as_target(), &mut |ui: &mut Ui| {
        scene(ui, "The quick brown fox jumps over the lazy dog");
    });
    host.frame(target.as_target(), &mut |ui: &mut Ui| scene(ui, "Hi"));
    let before = target.read_back();
    let both = mask_live(&host);
    host.device.backend_mut().purge_atlases();
    let kept = mask_live(&host);
    assert!(0 < kept && kept < both, "kept {kept} of {both} bytes");
    host.frame(target.as_target(), &mut |ui: &mut Ui| scene(ui, "Hi"));
    assert_eq!(mask_live(&host), kept, "nothing rasterized again");
    assert!(
        target.read_back() == before,
        "the purge changed what paints"
    );
}

/// A tile of `tile` at the top left over a striped ground, a glass of `glass` past the reach of a
/// change of the tile, and a `chip` painted over the glass's corner, which changes nothing under
/// it.
fn glass_scene(ui: &mut Ui, tile: RgbaF32, glass: Rect, chip: RgbaF32) {
    Panel::zstack()
        .id_salt("purged-glass-scene")
        .size((Sizing::FILL, Sizing::FILL))
        .chrome(Chrome::new(RgbaF32::hex(0x202830)))
        .show(ui, |ui| {
            Block::new()
                .id_salt("purged-tile")
                .size((Sizing::fixed(24.0), Sizing::fixed(16.0)))
                .chrome(Chrome::new(tile))
                .show(ui);
            Panel::canvas()
                .id_salt("purged-glass")
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    for i in 0..8 {
                        let stripe =
                            Rect::new(glass.min.x + 6.0 * i as f32, glass.min.y, 2.0, 30.0);
                        ui.add_shape(Shape::rect().at(stripe), RgbaF32::WHITE);
                    }
                    ui.add_shape(
                        Shape::rect().at(glass),
                        Paint::from(RgbaF32::TRANSPARENT).with_material(Material::blur(2.0)),
                    );
                    let corner = Rect::new(glass.min.x + 4.0, glass.min.y + 4.0, 6.0, 6.0);
                    ui.add_shape(Shape::rect().at(corner), chip);
                });
        });
}

/// A purge takes the result of a glass no frame drew since it was blurred, and the window
/// captures it again the next time it paints it, so the glass never shows its clear. The first
/// frames blur it; two frames that change a tile away from it keep its result and draw none of
/// it; the purge takes the result, idle two display frames; and a frame that changes a chip over
/// the glass, with nothing under it changed, blurs it again and paints what a window that never
/// purged paints.
#[test]
fn a_purge_takes_an_undrawn_backdrop_which_its_next_frame_captures_again() {
    const GLASS: Rect = Rect::new(100.0, 70.0, 40.0, 30.0);
    const SIZE: UVec2 = UVec2::new(160, 120);
    let (red, blue) = (RgbaF32::srgb(1.0, 0.2, 0.0), RgbaF32::srgb(0.0, 0.3, 1.0));
    let gpu = headless_test_gpu();
    let host = || {
        OffscreenHost::builder(gpu.handles())
            .shaper(TextShaper::test_mono())
            .swapchain_present()
            .build()
    };
    let frame = |host: &mut OffscreenHost, target: &OwnedTarget, tile, chip| {
        host.frame(target.as_target(), &mut |ui: &mut Ui| {
            glass_scene(ui, tile, GLASS, chip);
        });
        host.backdrop_blurs()
    };
    let results_live = |host: &OffscreenHost| host.atlas_memory().uses[2].live_bytes.unwrap();
    let paint = |purge: bool| {
        let mut host = host();
        let target = gpu.target("palantir.test.purge.glass", SIZE);
        assert_eq!(frame(&mut host, &target, red, red), 1);
        // The second frame paints whole again, as the present strategy settles.
        frame(&mut host, &target, blue, red);
        assert_eq!(frame(&mut host, &target, red, red), 0, "held, undrawn");
        assert_eq!(frame(&mut host, &target, blue, red), 0, "held, undrawn");
        assert!(results_live(&host) > 0);
        if purge {
            host.device.backend_mut().purge_atlases();
            assert_eq!(results_live(&host), 0, "the purge took the result");
        }
        let blurs = frame(&mut host, &target, blue, blue);
        (blurs, target.read_back())
    };
    let (held, kept) = paint(false);
    assert_eq!(held, 0, "a chip over the glass keeps its result");
    let (captured, purged) = paint(true);
    assert_eq!(captured, 1, "captured again");
    assert!(purged == kept, "the glass paints as with no purge");
}

/// A purge after a window closed frees the scratch texture its last frame's glass read, though
/// no frame came between: the backend's last frame holds a glass past half a page, its result in
/// a scratch texture; the window retires, the texture idles, and the purge drops it.
#[test]
fn a_purge_after_a_window_closed_drops_the_scratch_its_glass_read() {
    const WIDE: Rect = Rect::new(8.0, 16.0, 600.0, 32.0);
    let gpu = headless_test_gpu();
    let mut host = OffscreenHost::builder(gpu.handles())
        .shaper(TextShaper::test_mono())
        .build();
    let target = gpu.target("palantir.test.purge.scratch", UVec2::new(720, 64));
    host.frame(target.as_target(), &mut |ui: &mut Ui| {
        glass_scene(ui, RgbaF32::WHITE, WIDE, RgbaF32::WHITE);
    });
    let scratch = |host: &OffscreenHost| {
        host.device
            .backend()
            .atlas_scratch_texels(AtlasFormat::Rgba16F)
    };
    assert!(
        scratch(&host) > 0,
        "the wide result holds a scratch texture"
    );
    host.window.frames.retire(host.device.backend_mut());
    host.device.backend_mut().purge_atlases();
    assert_eq!(scratch(&host), 0);
}
