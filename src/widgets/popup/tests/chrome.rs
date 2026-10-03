//! Which background a popup body paints.

use crate::internals::harness::UiHarness;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::paint::background::Background;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::scene::layer::Layer;
use crate::shape::paint::shape_brush::ShapeBrush;
use crate::widget_core::configure::Configure;
use crate::widgets::popup::Popup;
use crate::widgets::popup::tests::support::{ANCHOR, SURFACE};

/// A builder step a case applies.
type Step = fn(Popup) -> Popup;

/// The theme's panel background is the popup's last resort: it paints when
/// the caller set no background, and a default beats it. How the default
/// and an explicit background resolve is checked for every chrome widget
/// at once, in `widgets::tests`.
#[test]
fn the_theme_panel_is_the_last_resort() {
    let theme_fill = RgbaF32::srgb(0.1, 0.2, 0.3);
    let default_fill = RgbaF32::srgb(0.9, 0.1, 0.1);
    let id = WidgetId::from_hash("chrome-popup");
    let cases: [(&str, Step, RgbaF32); 2] = [
        ("neither", |p| p, theme_fill),
        (
            "default only",
            |p| p.default_background(Background::fill(RgbaF32::srgb(0.9, 0.1, 0.1))),
            default_fill,
        ),
    ];
    for (label, build, want) in cases {
        let mut h = UiHarness::new(SURFACE);
        h.ui.theme_mut().panel_background = Some(Background::fill(theme_fill));
        h.frame(|ui| {
            build(Popup::at_point(ANCHOR).id(id)).show(ui, |_, _| {});
        });
        let body = h.node_of(id).expect("popup body recorded");
        assert_eq!(body.layer, Layer::Popup);
        let fill =
            h.ui.tree(Layer::Popup)
                .chrome(body.node)
                .expect("the body paints chrome")
                .fill;
        let want = RgbaF16::from(want);
        assert!(
            matches!(fill, ShapeBrush::Solid(got) if got == want),
            "{label}: {fill:?}, want {want:?}",
        );
    }
}
