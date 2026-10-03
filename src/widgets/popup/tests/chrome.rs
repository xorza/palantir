//! Which background a popup body paints.

use crate::internals::harness::UiHarness;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::anchor::Anchor;
use crate::primitives::paint::background::Background;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::scene::layer::Layer;
use crate::shape::paint::shape_brush::ShapeBrush;
use crate::widget_core::configure::Configure;
use crate::widgets::popup::Popup;
use crate::widgets::popup::tests::support::{ANCHOR, SURFACE};

/// A builder step a case applies: the popup, the default background and
/// the explicit one.
type Step = fn(Popup, &Background, &Background) -> Popup;

/// `default_background` fills in only where the caller set no
/// background, in either order, and the theme's panel background is the
/// last resort.
#[test]
fn default_background_yields_to_an_explicit_one() {
    let theme_fill = RgbaF32::srgb(0.1, 0.2, 0.3);
    let default_fill = RgbaF32::srgb(0.9, 0.1, 0.1);
    let explicit_fill = RgbaF32::srgb(0.1, 0.1, 0.9);
    let default_bg = Background::fill(default_fill);
    let explicit_bg = Background::fill(explicit_fill);
    let id = WidgetId::from_hash("chrome-popup");
    let cases: [(&str, Step, RgbaF32); 4] = [
        ("neither", |p, _, _| p, theme_fill),
        (
            "default only",
            |p, d, _| p.default_background(d),
            default_fill,
        ),
        (
            "explicit then default",
            |p, d, e| p.background(e.clone()).default_background(d),
            explicit_fill,
        ),
        (
            "default then explicit",
            |p, d, e| p.default_background(d).background(e.clone()),
            explicit_fill,
        ),
    ];
    for (label, build, want) in cases {
        let mut h = UiHarness::new(SURFACE);
        h.ui.theme_mut().panel_background = Some(Background::fill(theme_fill));
        h.frame(|ui| {
            build(
                Popup::new(Anchor::at_point(ANCHOR)).id(id),
                &default_bg,
                &explicit_bg,
            )
            .show(ui, |_, _| {});
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
