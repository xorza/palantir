use crate::host::harness::Harness;

use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::paint::chrome::Chrome;
use crate::scene::layer::Layer;
use crate::theme::button::ButtonTheme;
use crate::theme::look::slot_defaults::SlotDefaults;
use crate::widget_core::configure::Configure;
use crate::widgets::button::Button;
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
    theme.looks.normal.chrome = Chrome::NONE;

    let mut h = Harness::mono(UVec2::new(200, 120));
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

    let layouts = h.ui().tree(Layer::Main).records.layout();
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
    let mut h = Harness::mono(UVec2::new(200, 80));
    assert!(!h.frame_value(record(false)));
    h.key(Key::Char(' '));
    assert!(
        !h.frame_value(record(false)),
        "unfocused: Space is not a click"
    );

    let at = h.center_of(id);
    h.click(at);
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

/// `show_with`'s body gets the ink of the look the state resolved, in the resting pack or, for a
/// selected button, the selected one, and its value comes back as `inner`. Each look carries a
/// color of its own, so each case names its look; no transition, so a state change shows at once.
#[test]
fn show_with_hands_the_body_its_states_ink() {
    use crate::primitives::identity::widget_id::WidgetId;
    use crate::primitives::paint::color::RgbaF32;
    use crate::theme::look::WidgetLook;
    use crate::theme::look::stateful_look::StatefulLook;
    use crate::theme::text_style_overrides::TextStyleOverrides;
    use crate::ui::Ui;

    let inked = |rgb: u32| WidgetLook {
        chrome: Chrome::NONE,
        text: TextStyleOverrides::NONE.with_color(RgbaF32::hex(rgb)),
    };
    let pack = |base: u32| StatefulLook {
        normal: inked(base),
        hovered: inked(base + 1),
        active: inked(base + 2),
        disabled: inked(base + 3),
    };
    let theme = ButtonTheme {
        looks: pack(0x100),
        selected: pack(0x200),
        defaults: SlotDefaults {
            animation: None,
            ..ButtonTheme::default().defaults
        },
    };
    let id = WidgetId::from_hash("button");
    let ink = |selected: bool, disabled: bool| {
        let theme = &theme;
        move |ui: &mut Ui| {
            Button::new()
                .id(id)
                .size((80.0, 30.0))
                .style(theme)
                .selected(selected)
                .disabled(disabled)
                .show_with(ui, |_, text| text.color)
                .inner
        }
    };
    let mut h = Harness::mono(UVec2::new(200, 80));
    assert_eq!(h.frame_value(ink(false, false)), RgbaF32::hex(0x100));
    assert_eq!(h.frame_value(ink(true, false)), RgbaF32::hex(0x200));
    let at = h.center_of(id);
    h.hover(at);
    assert_eq!(h.frame_value(ink(false, false)), RgbaF32::hex(0x101));
    assert_eq!(h.frame_value(ink(true, false)), RgbaF32::hex(0x201));
    assert_eq!(h.frame_value(ink(true, true)), RgbaF32::hex(0x203));
}
