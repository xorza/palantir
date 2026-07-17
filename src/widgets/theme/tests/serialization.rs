use crate::primitives::background::Background;
use crate::primitives::color::Color;
use crate::primitives::corners::Corners;
use crate::primitives::shadow::Shadow;
use crate::primitives::stroke::Stroke;
use crate::widgets::theme::Theme;
use crate::widgets::theme::text_style::TextStyle;
use crate::widgets::theme::widget_look::WidgetLook;

#[test]
fn default_theme_roundtrips_through_toml() {
    let theme = Theme::default();
    let serialized = toml::to_string_pretty(&theme).expect("serialize");
    let parsed: Theme = toml::from_str(&serialized).expect("parse");
    let reserialized = toml::to_string_pretty(&parsed).expect("re-serialize");
    assert_eq!(serialized, reserialized);
}

#[test]
fn widget_look_serde_roundtrip() {
    let cases = [
        WidgetLook::default(),
        WidgetLook {
            background: Some(Background {
                fill: Color::hex(0x336699).into(),
                stroke: Stroke::solid(Color::hex(0xffffff), 1.5),
                corners: Corners::all(6.0),
                shadow: Shadow::NONE,
            }),
            text: Some(TextStyle::default().with_font_size(20.0)),
        },
    ];
    for look in cases {
        let serialized = toml::to_string_pretty(&look).expect("serialize");
        let parsed: WidgetLook = toml::from_str(&serialized).expect("parse");
        assert_eq!(look, parsed);
    }
}
