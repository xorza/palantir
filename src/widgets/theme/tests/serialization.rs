use crate::primitives::background::Background;
use crate::primitives::color::Color;
use crate::primitives::corners::Corners;
use crate::primitives::shadow::Shadow;
use crate::primitives::stroke::Stroke;
use crate::widgets::theme::Theme;
use crate::widgets::theme::text_style::TextStyle;
use crate::widgets::theme::widget_look::WidgetLook;

use super::pretty;

#[test]
fn default_theme_roundtrips_through_ron() {
    let theme = Theme::default();
    let serialized = pretty(&theme);
    let parsed: Theme = ron::from_str(&serialized).expect("parse");
    let reserialized = pretty(&parsed);
    assert_eq!(serialized, reserialized);
}

#[test]
fn widget_look_serde_roundtrip() {
    let cases = [
        WidgetLook::default(),
        WidgetLook {
            background: Background {
                fill: Color::hex(0x336699).into(),
                stroke: Stroke::solid(Color::hex(0xffffff), 1.5),
                corners: Corners::all(6.0),
                shadow: Shadow::NONE,
            },
            text: Some(TextStyle::default().with_font_size(20.0)),
        },
    ];
    for look in cases {
        let serialized = pretty(&look);
        let parsed: WidgetLook = ron::from_str(&serialized).expect("parse");
        assert_eq!(look, parsed);
    }
}
