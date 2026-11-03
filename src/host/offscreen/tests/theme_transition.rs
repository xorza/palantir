//! What a theme transition whose font sizes move costs the glyph atlas.

use std::time::Duration;

use glam::UVec2;

use crate::display::system_scale::SystemScale;
use crate::gpu::test_gpu::headless_test_gpu;
use crate::host::offscreen::{OffscreenHost, OffscreenTime};
use crate::text::shaper::TextShaper;
use crate::theme::Theme;
use crate::theme::color_scheme::ColorScheme;
use crate::theme::metrics::{Metrics, TextSteps};
use crate::theme::palette::Palette;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widgets::text::Text;

/// Forty lines of a pangram, each glyph of Latin's alphabet on screen.
fn scene(ui: &mut Ui) {
    for row in 0..40 {
        Text::new("The quick brown fox jumps over the lazy dog")
            .id_salt(row)
            .pos((8.0, 8.0 + 18.0 * row as f32))
            .show(ui);
    }
}

/// A 300 ms transition from 14 px text to 18 px at scale 2 shows a larger size on each of its
/// first 18 frames of 16 ms, yet the glyph atlas neither grows nor evicts: a transition's sizes
/// fit beside the rest, so its font sizes need no quantizing.
#[test]
fn a_transition_of_font_sizes_neither_grows_nor_evicts_the_glyph_atlas() {
    let gpu = headless_test_gpu();
    let mut host = OffscreenHost::builder(gpu.handles())
        .shaper(TextShaper::new())
        .time(OffscreenTime::Manual)
        .build();
    let target = gpu.target("palantir.test.theme_atlas", UVec2::new(1280, 1600));
    let frame = |host: &mut OffscreenHost| {
        host.advance(Duration::from_millis(16));
        host.set_system_scale(SystemScale::new(2.0).unwrap())
            .frame(target.as_target(), &mut |ui: &mut Ui| scene(ui));
        host.ui().theme().text.font_size
    };
    for _ in 0..3 {
        frame(&mut host);
    }
    let d = Metrics::DEFAULT;
    let roomy = Theme::from_parts(
        &Palette::DARK,
        &Metrics {
            text: TextSteps {
                caption: d.text.caption + 4.0,
                body: d.text.body + 4.0,
                title: d.text.title + 4.0,
            },
            ..d
        },
    );
    host.ui_mut().set_theme_for(ColorScheme::Dark, roomy);
    let sizes: Vec<f32> = (0..24).map(|_| frame(&mut host)).collect();
    let moving = &sizes[..18];
    assert!(
        moving.windows(2).all(|pair| pair[0] < pair[1]) && moving[0] > 14.0,
        "{sizes:?}"
    );
    assert_eq!(sizes[23], 18.0);
    let counts = host.core.backend().text_atlas_counts();
    assert_eq!(
        (counts.grows, counts.evictions, counts.oversized),
        (0, 0, 0),
        "{counts:?}"
    );
}
