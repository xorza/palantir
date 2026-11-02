//! Which chrome a popup body paints.

use crate::host::harness::Harness;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::anchor::Anchor;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::scene::layer::Layer;
use crate::shape::paint::shape_brush::ShapeBrush;
use crate::theme::popup::PopupTheme;
use crate::widget_core::configure::Configure;
use crate::widgets::popup::Popup;
use crate::widgets::popup::tests::support::{ANCHOR, SURFACE};

/// A builder step a case applies, handed the style a case may set.
type Step = for<'a> fn(Popup<'a>, &'a PopupTheme) -> Popup<'a>;

/// The theme's popup slot is a popup's last resort: its panel paints and its padding insets the
/// body when the caller set neither, a `style` beats it, a default beats both, `Chrome::NONE`
/// paints nothing, and the in-tree containers' `panel_chrome` never reaches a popup. Default-versus-explicit resolution
/// is checked for every chrome widget in `widgets::tests`.
#[test]
fn the_theme_popup_slot_is_the_last_resort() {
    let theme_fill = RgbaF32::srgb(0.1, 0.2, 0.3);
    let default_fill = RgbaF32::srgb(0.9, 0.1, 0.1);
    let theme_padding = Spacing::all(7.0);
    let id = WidgetId::from_hash("chrome-popup");
    let style_fill = RgbaF32::srgb(0.2, 0.7, 0.4);
    let style = PopupTheme {
        panel: Chrome::fill(style_fill),
        padding: Spacing::all(5.0),
    };
    let cases: [(&str, Step, Option<RgbaF32>, Spacing); 6] = [
        ("neither", |p, _| p, Some(theme_fill), theme_padding),
        (
            "default only",
            |p, _| p.default_chrome(Chrome::fill(RgbaF32::srgb(0.9, 0.1, 0.1))),
            Some(default_fill),
            theme_padding,
        ),
        ("none", |p, _| p.chrome(Chrome::NONE), None, theme_padding),
        (
            "own padding",
            |p, _| p.padding(Spacing::all(2.0)),
            Some(theme_fill),
            Spacing::all(2.0),
        ),
        (
            "style",
            |p, s| p.style(s),
            Some(style_fill),
            Spacing::all(5.0),
        ),
        (
            "style and default",
            |p, s| {
                p.style(s)
                    .default_chrome(Chrome::fill(RgbaF32::srgb(0.9, 0.1, 0.1)))
            },
            Some(default_fill),
            Spacing::all(5.0),
        ),
    ];
    for (label, build, want, padding) in cases {
        let mut h = Harness::mono(SURFACE);
        let theme = h.ui().theme_mut();
        theme.popup.panel = Chrome::fill(theme_fill);
        theme.popup.padding = theme_padding;
        theme.panel_chrome = Some(Chrome::fill(RgbaF32::srgb(0.0, 1.0, 0.0)));
        h.frame(|ui| {
            build(Popup::new().anchor(Anchor::at_point(ANCHOR)).id(id), &style).show(ui, |_, _| {});
        });
        let body = h.node_of(id).expect("popup body recorded");
        assert_eq!(body.plane.layer, Layer::Popup);
        let tree = h.ui().tree(Layer::Popup);
        assert_eq!(
            tree.records.layout()[body.node.idx()].padding,
            padding,
            "{label}"
        );
        let fill = tree.chrome(body.node).map(|chrome| chrome.fill);
        match want {
            None => assert!(fill.is_none(), "{label}: {fill:?}, want no chrome"),
            Some(want) => {
                let want = want.to_f16();
                assert!(
                    matches!(fill, Some(ShapeBrush::Solid(got)) if got == want),
                    "{label}: {fill:?}, want {want:?}",
                );
            }
        }
    }
}
