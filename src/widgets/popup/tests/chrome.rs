//! Which chrome a popup body paints.

use crate::host::harness::Harness;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::anchor::Anchor;
use crate::primitives::layout::clip_mode::ClipMode;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::paint::border::Border;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::material::Material;
use crate::scene::layer::Layer;
use crate::shape::paint::closed_paint::ClosedPaint;
use crate::shape::paint::lowered_brush::LoweredBrush;
use crate::shape::record::ShapeRecord;
use crate::theme::popup::PopupTheme;
use crate::widget_core::configure::Configure;
use crate::widgets::block::Block;
use crate::widgets::popup::Popup;
use crate::widgets::popup::tests::support::{ANCHOR, SURFACE};
use glam::Vec2;

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
        panel: Chrome::new(style_fill),
        material: None,
        padding: Spacing::all(5.0),
    };
    let cases: [(&str, Step, Option<RgbaF32>, Spacing); 6] = [
        ("neither", |p, _| p, Some(theme_fill), theme_padding),
        (
            "default only",
            |p, _| p.default_chrome(Chrome::new(RgbaF32::srgb(0.9, 0.1, 0.1))),
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
                    .default_chrome(Chrome::new(RgbaF32::srgb(0.9, 0.1, 0.1)))
            },
            Some(default_fill),
            Spacing::all(5.0),
        ),
    ];
    for (label, build, want, padding) in cases {
        let mut h = Harness::mono(SURFACE);
        let theme = h.ui_mut().theme_mut();
        theme.popup.panel = Chrome::new(theme_fill);
        theme.popup.material = None;
        theme.popup.padding = theme_padding;
        theme.panel_chrome = Some(Chrome::new(RgbaF32::srgb(0.0, 1.0, 0.0)));
        h.frame(|ui| {
            build(Popup::new().anchor(Anchor::at_point(ANCHOR)).id(id), &style).show(ui, |_| {});
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
                    matches!(fill, Some(LoweredBrush::Solid(got)) if got == want),
                    "{label}: {fill:?}, want {want:?}",
                );
            }
        }
    }
}

/// A panel over its slot's material draws as a shape of the panel's paint over the material in a
/// frame around the popup's node, which fills it, keeps its id, and holds the body, padded by the
/// slot's padding and the border, 5 + 2 = 7 on each side, clipped to the panel's corners, and
/// stacked at the popup's gap of 3: the popup lays out as the panel without a material does, its
/// rect and its body's the same, while that panel paints as the node's chrome, padded alike, in no
/// frame; and so does a chrome the caller sets over a slot with a material, which wears none.
#[test]
fn a_material_draws_under_the_panel_in_a_frame_around_the_popup() {
    let id = WidgetId::from_hash("material-popup");
    let body_id = WidgetId::from_hash("material-popup-body");
    let next_id = WidgetId::from_hash("material-popup-next");
    let fill = RgbaF32::srgb(0.1, 0.2, 0.3).with_alpha(0.5);
    let panel =
        Chrome::rounded(fill, Corners::all(6.0)).with_border(Border::new(RgbaF32::WHITE, 2.0));
    let layout = |material: Option<Material>, own: Option<&Chrome>| {
        let mut h = Harness::mono(SURFACE);
        let style = PopupTheme {
            panel: panel.clone(),
            material,
            padding: Spacing::all(5.0),
        };
        h.ui_mut().theme_mut().panel_clip = ClipMode::Rounded;
        h.frame(|ui| {
            let popup = Popup::new()
                .anchor(Anchor::at_point(ANCHOR))
                .id(id)
                .style(&style)
                .gap(3.0);
            match own {
                Some(chrome) => popup.chrome(chrome.clone()),
                None => popup,
            }
            .show(ui, |ui| {
                for id in [body_id, next_id] {
                    Block::new()
                        .id(id)
                        .size((Sizing::fixed(20.0), Sizing::fixed(10.0)))
                        .show(ui);
                }
            });
        });
        let rect = |id| {
            let node = h.node_of(id).expect("recorded").node;
            h.ui().plane_layout(Layer::Popup).rect[node.idx()]
        };
        let popup = h.node_of(id).expect("the popup recorded").node;
        let tree = h.ui().tree(Layer::Popup);
        assert_eq!(
            tree.records.layout()[popup.idx()].padding,
            Spacing::all(7.0),
            "the padding and the border"
        );
        assert_eq!(
            tree.records.attrs()[popup.idx()].clip_mode(),
            ClipMode::Rounded
        );
        if let Some(frame) = h.node_of(id.with("frame")) {
            assert!(own.is_none(), "a chrome the caller sets wears no material");
            assert_eq!(
                rect(id.with("frame")),
                rect(id),
                "the popup fills its frame"
            );
            assert_eq!(
                tree.chrome(popup).map(|row| row.corners),
                Some(Corners::all(6.0)),
                "the corners the popup clips to"
            );
            assert!(
                tree.chrome(frame.node).is_none(),
                "the panel paints as a shape"
            );
            let first = tree.records.shape_span()[frame.node.idx()].start;
            let ShapeRecord::Closed {
                paint: ClosedPaint::Fill(paint),
                ..
            } = &tree.shapes.records[first as usize]
            else {
                panic!("the frame's first shape is the panel");
            };
            assert!(
                paint.material.is_some(),
                "the panel's shape wears the material"
            );
            assert!(
                matches!(paint.fill, LoweredBrush::Solid(got) if got == fill.to_f16()),
                "the panel's own fill: {:?}",
                paint.fill
            );
        } else {
            let row = tree
                .chrome(popup)
                .expect("the panel paints as the node's chrome");
            assert!(matches!(row.fill, LoweredBrush::Solid(got) if got == fill.to_f16()));
        }
        [rect(id), rect(body_id), rect(next_id)]
    };
    let frost = || Some(Material::frosted(4.0));
    let worn = layout(frost(), None);
    assert_eq!(worn, layout(None, None));
    assert_eq!(worn, layout(frost(), Some(&panel)));
    assert_eq!(worn[1].min, worn[0].min + Vec2::splat(7.0));
    assert_eq!(worn[2].min, worn[1].min + Vec2::new(0.0, 10.0 + 3.0));
}
