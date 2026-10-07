use crate::internals::harness::UiHarness;
use crate::internals::harness::size_trio::SizeTrio;
use crate::primitives::geometry::size::Size;

use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::layout::align::{Align, HAlign, VAlign};
use crate::primitives::layout::sizing::Sizing;
use crate::scene::layer::Layer;
use crate::widget_core::configure::Configure;
use crate::widgets::panel::Panel;
use crate::widgets::separator::Separator;
use crate::widgets::theme::separator::SeparatorTheme;
use glam::UVec2;

/// `Separator` takes a per-instance `.style(&SeparatorTheme)` so `MenuSeparator` can hand its slot down whole. `margin` fills in where the builder is silent and loses where it isn't: the menu slot holds the rule off the rows, the in-flow slot is zero, `.margin(...)` beats both.
#[test]
fn instance_style_beats_the_global_slot_and_explicit_margin_beats_both() {
    let styled = SeparatorTheme {
        thickness: 3.0,
        margin: Spacing::xy(0.0, 5.0),
        ..SeparatorTheme::default()
    };
    let mut h = UiHarness::new(UVec2::new(400, 300));
    // Loudly different global slot; a styled rule must not reach it.
    h.ui.theme_mut().separator.thickness = 11.0;
    h.ui.theme_mut().separator.margin = Spacing::all(9.0);

    let [inherited, explicit, global, thick] = h.frame_value(|ui| {
        let col = Panel::vstack().auto_id().size((Sizing::FILL, Sizing::FILL));
        col.show(ui, |ui| {
            [
                Separator::horizontal().style(&styled).show(ui).node(),
                Separator::horizontal()
                    .style(&styled)
                    .margin(Spacing::ZERO)
                    .show(ui)
                    .node(),
                Separator::horizontal().show(ui).node(),
                Separator::horizontal()
                    .style(&styled)
                    .thickness(7.0)
                    .show(ui)
                    .node(),
            ]
        })
        .inner
    });

    let layouts = h.ui.tree(Layer::Main).records.layout();
    let rects = &h.ui.layout(Layer::Main).rect;
    assert_eq!(
        layouts[inherited.idx()].margin,
        Spacing::xy(0.0, 5.0),
        "the styled bundle's margin fills in",
    );
    assert_eq!(
        rects[inherited.idx()].size.h,
        3.0,
        "the styled bundle's thickness wins over the global slot's 11",
    );
    assert_eq!(
        layouts[explicit.idx()].margin,
        Spacing::ZERO,
        "an explicit margin beats the styled bundle",
    );
    assert_eq!(
        layouts[global.idx()].margin,
        Spacing::all(9.0),
        "an unstyled rule still reads the global slot",
    );
    assert_eq!(
        rects[thick.idx()].size.h,
        7.0,
        "an explicit thickness beats the styled bundle's 3",
    );
}

/// Explicit `.size(...)` replaces the Hug+Stretch default; an untouched horizontal rule stretches across the 400-wide FILL column at thickness 1.
#[test]
fn explicit_size_overrides_stretch_default() {
    let trio = SizeTrio::of((Sizing::fixed(50.0), Sizing::fixed(3.0)), |ui, size| {
        let mut rule = Separator::horizontal();
        if let Some(size) = size {
            rule = rule.size(size);
        }
        rule.show(ui).node()
    });
    assert_eq!(
        trio,
        SizeTrio {
            sized: Size::new(50.0, 3.0),
            hug: Size::ZERO,
            default: Size::new(400.0, 1.0),
        }
    );
}

/// The `Hug + Stretch` default is per-axis: in a 400x300 `ZStack` untouched, the rule stretches to 400 at thickness 1. `HAlign::Center` keeps width at `Hug`'s 0, centered at `(400 - 0) / 2`. `VAlign::Bottom` leaves the horizontal axis `Auto`, still stretched, and pins the top at `300 - 1`.
#[test]
fn a_callers_alignment_survives_the_stretch_default_axis_by_axis() {
    let mut h = UiHarness::new(UVec2::new(400, 300));
    let [default, centered, bottom] = h.frame_value(|ui| {
        let layers = Panel::zstack().auto_id().size((Sizing::FILL, Sizing::FILL));
        layers
            .show(ui, |ui| {
                [
                    Separator::horizontal().show(ui).node(),
                    Separator::horizontal()
                        .align(Align::h(HAlign::Center))
                        .show(ui)
                        .node(),
                    Separator::horizontal()
                        .align(Align::v(VAlign::Bottom))
                        .show(ui)
                        .node(),
                ]
            })
            .inner
    });
    let rects = &h.ui.layout(Layer::Main).rect;
    let d = rects[default.idx()];
    assert_eq!(
        (d.min.x, d.size.w),
        (0.0, 400.0),
        "untouched rule stretches"
    );
    let c = rects[centered.idx()];
    assert_eq!(
        (c.min.x, c.size.w),
        (200.0, 0.0),
        "an explicit horizontal alignment beats the stretch default",
    );
    let b = rects[bottom.idx()];
    assert_eq!(
        (b.min.y, b.size.w),
        (299.0, 400.0),
        "a vertical alignment leaves the horizontal stretch in place",
    );
}
