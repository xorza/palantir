//! A purge between frames, as the system warns of low memory, gives the glyphs no frame drew
//! back, and the next frame paints as before it.

use glam::UVec2;

use crate::gpu::test_gpu::headless_test_gpu;
use crate::host::offscreen::OffscreenHost;
use crate::primitives::paint::color::RgbaF32;
use crate::text::shaper::TextShaper;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
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
