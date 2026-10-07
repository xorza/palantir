use crate::internals::harness::UiHarness;

use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::paint::background::Background;
use crate::scene::layer::Layer;
use crate::widget_core::configure::Configure;
use crate::widget_core::widget_look::theme_slot::SlotDefaults;
use crate::widgets::button::Button;
use crate::widgets::theme::button::ButtonTheme;
use glam::UVec2;

#[test]
fn explicit_zero_spacing_overrides_theme_spacing() {
    let mut theme = ButtonTheme {
        defaults: SlotDefaults {
            padding: Spacing::all(8.0),
            margin: Spacing::all(4.0),
            ..ButtonTheme::default().defaults
        },
        ..ButtonTheme::default()
    };
    theme.looks.normal.background = Background::NONE;

    let mut h = UiHarness::new(UVec2::new(200, 120));
    let [explicit, inherited] = h.frame_value(|ui| {
        [
            Button::new()
                .style(&theme)
                .padding(Spacing::ZERO)
                .margin(Spacing::ZERO)
                .show(ui)
                .node(),
            Button::new().style(&theme).show(ui).node(),
        ]
    });

    let layouts = h.ui.tree(Layer::Main).records.layout();
    let explicit = layouts[explicit.idx()];
    let inherited = layouts[inherited.idx()];
    assert_eq!(explicit.padding, Spacing::ZERO);
    assert_eq!(explicit.margin, Spacing::ZERO);
    assert_eq!(inherited.padding, Spacing::all(8.0));
    assert_eq!(inherited.margin, Spacing::all(4.0));
}

/// A Button is a Tab stop a click focuses; a focused one is clicked by Space and Enter once per press via `clicked()`, while an unfocused or disabled one takes neither.
#[test]
fn space_and_enter_click_a_focused_button() {
    use crate::input::keyboard::key::Key;
    use crate::primitives::identity::widget_id::WidgetId;
    use crate::ui::Ui;

    let id = WidgetId::from_hash("button");
    let record = |disabled: bool| {
        move |ui: &mut Ui| {
            Button::new()
                .id(id)
                .size((80.0, 30.0))
                .disabled(disabled)
                .show(ui)
                .clicked()
        }
    };
    let mut h = UiHarness::new(UVec2::new(200, 80));
    assert!(!h.frame_value(record(false)));
    h.key(Key::Char(' '));
    assert!(
        !h.frame_value(record(false)),
        "unfocused: Space is not a click"
    );

    let at = h.center_of(id);
    h.click_at(at);
    assert!(h.frame_value(record(false)), "the click itself");
    assert_eq!(h.focus(), Some(id), "a click focuses a button");
    for key in [Key::Char(' '), Key::Enter] {
        h.key(key);
        assert!(h.frame_value(record(false)), "{key:?} clicks it");
        assert!(
            !h.frame_value(record(false)),
            "{key:?}: one click per press"
        );
    }
    h.key(Key::Enter);
    assert!(
        !h.frame_value(record(true)),
        "a disabled button takes no key"
    );
}
