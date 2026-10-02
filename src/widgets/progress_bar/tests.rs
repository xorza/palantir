use crate::primitives::size::Size;
use crate::ui::harness::UiHarness;
use crate::widgets::size_trio::SizeTrio;

use crate::layout::types::sizing::Sizing;
use crate::widgets::configure::Configure;
use crate::widgets::progress_bar::ProgressBar;
use glam::UVec2;

/// Explicit `.size(...)` wins over the widget's `Fill × theme.thickness`
/// default, and an untouched bar still gets that default (400-wide FILL
/// column → 400 × theme thickness 6).
#[test]
fn explicit_size_overrides_fill_default() {
    let trio = SizeTrio::of((Sizing::fixed(80.0), Sizing::fixed(10.0)), |ui, size| {
        let mut bar = ProgressBar::new(0.3);
        if let Some(size) = size {
            bar = bar.size(size);
        }
        bar.show(ui).node()
    });
    assert_eq!(
        trio,
        SizeTrio {
            sized: Size::new(80.0, 10.0),
            hug: Size::ZERO,
            default: Size::new(400.0, 6.0),
        }
    );
}

/// Both endpoints collapse one segment to a zero-extent `Fixed` rather
/// than a zero-weight `Fill`, and a fraction that names no share reads as
/// empty instead of reaching `Sizing::share`'s finite assert —
/// `ProgressBar::new(done / total)` with `total == 0` is the case app
/// code writes without thinking.
#[test]
fn endpoint_segments_collapse_without_invalid_fill_weights() {
    for (fraction, expected) in [
        (0.0, [0.0, 100.0]),
        (1.0, [100.0, 0.0]),
        (f32::NAN, [0.0, 100.0]), // what `done / total` yields at total 0
        (f32::INFINITY, [0.0, 100.0]),
        (-1.0, [0.0, 100.0]),
        (2.0, [100.0, 0.0]),
    ] {
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
