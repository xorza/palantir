use crate::primitives::size::Size;
use crate::ui::harness::UiHarness;
use crate::ui::harness::size_trio::SizeTrio;

use crate::layout::types::align::{Align, HAlign, VAlign};
use crate::layout::types::sizing::Sizing;
use crate::primitives::spacing::Spacing;
use crate::scene::layer::Layer;
use crate::widgets::configure::Configure;
use crate::widgets::panel::Panel;
use crate::widgets::separator::Separator;
use crate::widgets::theme::separator::SeparatorTheme;
use glam::UVec2;

/// `Separator` gained the per-instance `.style(&SeparatorTheme)`
/// every other themed widget already had — which is what lets
/// `MenuSeparator` hand its slot down whole instead of unpacking it
/// field by field.
///
/// `margin` came with it, so the bundle also has to fill in where
/// the builder stayed silent and lose where it didn't: the menu slot
/// holds its rule off the rows around it, the in-flow slot leaves it
/// at zero, and a caller who says `.margin(...)` beats both.
#[test]
fn instance_style_beats_the_global_slot_and_explicit_margin_beats_both() {
    let styled = SeparatorTheme {
        thickness: 3.0,
        margin: Spacing::xy(0.0, 5.0),
        ..SeparatorTheme::default()
    };
    let mut h = UiHarness::new(UVec2::new(400, 300));
    // Loudly different global slot — a styled rule must not reach it.
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

/// Explicit `.size(...)` replaces the Hug+Stretch/thickness default
/// entirely, and an untouched horizontal rule still stretches across
/// the 400-wide FILL column at the theme thickness of 1.
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

/// The `Hug + Stretch` default is per-axis, so it fills in only the axis
/// the caller left `Auto`.
///
/// In a 400x300 `ZStack` both axes are cross axes. Untouched, the rule
/// stretches to the full 400 at the theme thickness of 1. A caller's
/// `HAlign::Center` keeps the width at `Hug`'s 0, centered at
/// `(400 - 0) / 2`. A caller's `VAlign::Bottom` leaves the horizontal
/// axis `Auto`, so the stretch still fills it in, and pins the rule's
/// top at `300 - 1`.
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
