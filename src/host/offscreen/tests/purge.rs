//! A purge between frames, as the system warns of low memory, gives the glyphs, the backdrop
//! results and the gradient rows no frame drew back, and the next frame paints as before it.

use glam::UVec2;
use image::RgbaImage;

use crate::gpu::atlas::atlas_format::AtlasFormat;
use crate::gpu::test_gpu::{TestGpu, headless_test_gpu};
use crate::host::offscreen::OffscreenHost;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::paint::brush::gradient::Gradient;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::srgba_u8::SrgbaU8;
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

/// `count` gradients in a row, the first `count` of one sequence.
fn gradients(ui: &mut Ui, count: u32) {
    for i in 0..count {
        let [r, g, _, _] = (40 * i).to_le_bytes();
        ui.add_shape(
            Shape::rect().at(Rect::new(4.0 + 12.0 * i as f32, 4.0, 10.0, 10.0)),
            Gradient::horizontal([SrgbaU8::rgb(r, g, 9).into(), RgbaF32::WHITE]),
        );
    }
}

/// A purge takes the gradient rows the last frame did not draw, and the row texture follows at
/// once: a frame of 20 gradients grows the table from its 16 first rows to 32, a frame of the first
/// alone keeps them, and the purge leaves that one's row and the fallback, 4 KiB live, in a table
/// and a texture of 16 rows, 32 KiB; the next frame of the one paints as before the purge.
#[test]
fn a_purge_takes_the_undrawn_gradient_rows() {
    let gpu = headless_test_gpu();
    let mut host = OffscreenHost::builder(gpu.handles())
        .shaper(TextShaper::test_mono())
        .build();
    let target = gpu.target("palantir.test.purge.rows", UVec2::new(260, 20));
    let rows = |host: &OffscreenHost| {
        let rows = host.atlas_memory().uses[4];
        (rows.live_bytes.unwrap(), rows.bytes)
    };
    host.frame(target.as_target(), &mut |ui: &mut Ui| gradients(ui, 20));
    assert_eq!(rows(&host), (21 * 2048, 32 * 2048));
    host.frame(target.as_target(), &mut |ui: &mut Ui| gradients(ui, 1));
    let before = target.read_back();
    host.device.backend_mut().purge_atlases();
    assert_eq!(rows(&host), (2 * 2048, 16 * 2048));
    host.frame(target.as_target(), &mut |ui: &mut Ui| gradients(ui, 1));
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

/// A purge takes the result of a glass no frame drew since it was blurred, on a page or in a
/// scratch texture of its own, and the window captures it again the next time it paints it, so the
/// glass never shows its clear. The first frames blur it; two frames that change a tile away from
/// it keep its result and draw none of it; the purge takes the result, idle two display frames; and
/// a frame that changes a chip over the glass, with nothing under it changed, blurs it again and
/// paints what a window that never purged paints. A 40 × 30 glass's result lies on a page; a
/// 600 × 32 one's, past half a page of 1024, in a scratch texture.
#[test]
fn a_purge_takes_an_undrawn_backdrop_which_its_next_frame_captures_again() {
    let gpu = headless_test_gpu();
    for (size, glass) in [
        (UVec2::new(160, 120), Rect::new(100.0, 70.0, 40.0, 30.0)),
        (UVec2::new(720, 96), Rect::new(8.0, 48.0, 600.0, 32.0)),
    ] {
        let paint = |purge: bool| purge_and_repaint(&gpu, size, glass, purge);
        let (held, kept) = paint(false);
        assert_eq!(held, 0, "{glass:?}: a chip over the glass keeps its result");
        let (captured, purged) = paint(true);
        assert_eq!(captured, 1, "{glass:?}: captured again");
        assert!(purged == kept, "{glass:?}: paints as with no purge");
    }
}

/// The blurs of the frame that repaints `glass` on a window of `size` after it went undrawn for two
/// frames, and a purge between where `purge` says, and the pixels it paints.
fn purge_and_repaint(gpu: &TestGpu, size: UVec2, glass: Rect, purge: bool) -> (usize, RgbaImage) {
    let (red, blue) = (RgbaF32::srgb(1.0, 0.2, 0.0), RgbaF32::srgb(0.0, 0.3, 1.0));
    let mut host = OffscreenHost::builder(gpu.handles())
        .shaper(TextShaper::test_mono())
        .swapchain_present()
        .build();
    let target = gpu.target("palantir.test.purge.glass", size);
    let frame = |host: &mut OffscreenHost, tile, chip| {
        host.frame(target.as_target(), &mut |ui: &mut Ui| {
            glass_scene(ui, tile, glass, chip);
        });
        host.backdrop_blurs()
    };
    let results = |host: &OffscreenHost| {
        let half_float = host.atlas_memory().uses[2];
        (half_float.live_bytes.unwrap(), half_float.bytes)
    };
    assert_eq!(frame(&mut host, red, red), 1);
    // The second frame paints whole again, as the present strategy settles.
    frame(&mut host, blue, red);
    assert_eq!(frame(&mut host, red, red), 0, "held, undrawn");
    assert_eq!(frame(&mut host, blue, red), 0, "held, undrawn");
    assert!(results(&host).0 > 0);
    if purge {
        host.device.backend_mut().purge_atlases();
        assert_eq!(
            results(&host),
            (0, 0),
            "the purge took the result and its texture"
        );
    }
    let blurs = frame(&mut host, blue, blue);
    (blurs, target.read_back())
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
