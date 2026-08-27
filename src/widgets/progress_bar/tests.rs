use crate::ui::harness::UiHarness;

use crate::layout::types::sizing::Sizing;
use crate::scene::layer::Layer;
use crate::scene::node::configure::Configure;
use crate::widgets::panel::Panel;
use crate::widgets::progress_bar::ProgressBar;
use glam::UVec2;

/// Explicit `.size(...)` wins over the widget's `Fill × theme.height`
/// default, and an untouched bar still gets that default (400-wide FILL
/// column → 400 × theme height 6).
#[test]
fn explicit_size_overrides_fill_default() {
    let mut h = UiHarness::new(UVec2::new(400, 300));
    let (mut sized, mut hug, mut default) = (None, None, None);
    h.frame(|ui| {
        let col = Panel::vstack().auto_id().size((Sizing::FILL, Sizing::FILL));
        col.show(ui, |ui| {
            sized = Some(
                ProgressBar::new(0.3)
                    .size((Sizing::fixed(80.0), Sizing::fixed(10.0)))
                    .show(ui)
                    .node(),
            );
            hug = Some(
                ProgressBar::new(0.3)
                    .size((Sizing::HUG, Sizing::HUG))
                    .show(ui)
                    .node(),
            );
            default = Some(ProgressBar::new(0.3).show(ui).node());
        });
    });
    let rects = &h.ui.layout(Layer::Main).rect;
    let s = rects[sized.unwrap().idx()];
    assert_eq!((s.size.w, s.size.h), (80.0, 10.0), "explicit size");
    let h = rects[hug.unwrap().idx()];
    assert_eq!((h.size.w, h.size.h), (0.0, 0.0), "explicit hug");
    let d = rects[default.unwrap().idx()];
    assert_eq!((d.size.w, d.size.h), (400.0, 6.0), "untouched default");
}

#[test]
fn endpoint_segments_collapse_without_invalid_fill_weights() {
    for (fraction, expected) in [(0.0, [0.0, 100.0]), (1.0, [100.0, 0.0])] {
        let mut h = UiHarness::new(UVec2::new(100, 20));
        let root = h.frame_value(|ui| {
            ProgressBar::new(fraction)
                .size((Sizing::fixed(100.0), Sizing::fixed(10.0)))
                .show(ui)
                .node()
        });
        let widths: Vec<_> = h
            .main_child_rects(root)
            .into_iter()
            .map(|rect| rect.size.w)
            .collect();
        assert_eq!(widths, expected, "fraction {fraction}");
    }
}
