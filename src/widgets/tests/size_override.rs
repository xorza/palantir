//! Pins the `Sizes::default()` sentinel contract: widgets that install
//! their own sizing default (`Slider`, `ProgressBar`, `Separator`) do so
//! only when the caller left `Configure::size` untouched — an explicit
//! size wins and fully describes the widget's box.

use crate::Ui;
use crate::forest::Layer;
use crate::forest::element::Configure;
use crate::layout::types::sizing::Sizing;
use crate::widgets::panel::Panel;
use crate::widgets::progress_bar::ProgressBar;
use crate::widgets::separator::Separator;
use crate::widgets::slider::Slider;
use glam::UVec2;

/// Explicit `.size(...)` overrides each widget's built-in default, and
/// an untouched widget still gets that default (Slider: `Fill ×
/// Fixed(knob_size = 18)` → 400×18 in a 400-wide column) — so the
/// sentinel demonstrably changes behavior in both directions.
#[test]
fn explicit_size_overrides_widget_defaults() {
    let mut ui = Ui::for_test();
    let mut v = 0.5_f32;
    let mut sized = [None; 3];
    let mut default_slider = None;
    ui.run_at(UVec2::new(400, 300), |ui| {
        // FILL column: a Hug column would hug to the widest fixed child
        // (120) and the default slider's Fill would resolve to that
        // instead of the surface width.
        let col = Panel::vstack().auto_id().size((Sizing::FILL, Sizing::FILL));
        col.show(ui, |ui| {
            sized[0] = Some(
                Slider::new(&mut v, 0.0..=1.0)
                    .size((Sizing::Fixed(120.0), Sizing::Fixed(30.0)))
                    .show(ui)
                    .node(),
            );
            sized[1] = Some(
                ProgressBar::new(0.3)
                    .size((Sizing::Fixed(80.0), Sizing::Fixed(10.0)))
                    .show(ui)
                    .node(),
            );
            sized[2] = Some(
                Separator::horizontal()
                    .size((Sizing::Fixed(50.0), Sizing::Fixed(3.0)))
                    .show(ui)
                    .node(),
            );
            default_slider = Some(Slider::new(&mut v, 0.0..=1.0).show(ui).node());
        });
    });
    let rects = &ui.layout[Layer::Main].rect;
    let expected = [
        ("slider", 120.0, 30.0),
        ("progress bar", 80.0, 10.0),
        ("separator", 50.0, 3.0),
    ];
    for (node, (name, w, h)) in sized.iter().zip(expected) {
        let r = rects[node.unwrap().idx()];
        assert_eq!((r.size.w, r.size.h), (w, h), "{name} explicit size");
    }
    let r = rects[default_slider.unwrap().idx()];
    assert_eq!(
        (r.size.w, r.size.h),
        (400.0, 18.0),
        "untouched slider keeps its Fill × knob_size default",
    );
}
