use crate::internals::harness::UiHarness;
use crate::primitives::identity::widget_id::WidgetId;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widget_core::overlay_response::OverlayResponse;
use crate::widgets::button::Button;
use crate::widgets::popup::popup_trigger::PopupTrigger;
use glam::{UVec2, Vec2};

const SURFACE: UVec2 = UVec2::new(400, 300);

fn trigger_id() -> WidgetId {
    WidgetId::from_hash("popup-trigger")
}

/// One frame: the trigger (disabled or not) and its popup, whose body returns 7.
fn scene(ui: &mut Ui, disabled: bool) -> OverlayResponse<Option<i32>> {
    let trigger = Button::new()
        .id(trigger_id())
        .label("open")
        .disabled(disabled)
        .show(ui)
        .snapshot();
    PopupTrigger::on(&trigger).show(ui, |_, _| 7)
}

fn trigger_centre(h: &UiHarness) -> Vec2 {
    h.rect(trigger_id()).expect("trigger arranged").center()
}

/// A click opens the popup; a second lands outside and is a dismissal: the body runs once more, the response reports `closed()`, the next frame records nothing.
#[test]
fn a_click_toggles_and_the_body_runs_only_while_open() {
    let mut h = UiHarness::new(SURFACE);
    assert_eq!(h.frame_value(|ui| scene(ui, false)).inner, None);
    assert!(!PopupTrigger::is_open(&h.ui, trigger_id()));

    let at = trigger_centre(&h);
    h.click_at(at);
    let opened = h.frame_value(|ui| scene(ui, false));
    assert_eq!(opened.inner, Some(7), "the click opens it");
    assert!(PopupTrigger::is_open(&h.ui, trigger_id()));

    assert_eq!(
        h.frame_value(|ui| scene(ui, false)).inner,
        Some(7),
        "it stays open"
    );

    h.click_at(at);
    assert!(
        h.frame_value(|ui| scene(ui, false)).closed(),
        "a second click closes it"
    );
    assert!(!PopupTrigger::is_open(&h.ui, trigger_id()));
    assert_eq!(h.frame_value(|ui| scene(ui, false)).inner, None);
}

/// A programmatic open records next frame; a disabled trigger closes an open popup without running its body.
#[test]
fn open_close_and_a_disabled_trigger() {
    let mut h = UiHarness::new(SURFACE);
    h.frame(|ui| {
        scene(ui, false);
    });

    PopupTrigger::open(&mut h.ui, trigger_id());
    assert_eq!(h.frame_value(|ui| scene(ui, false)).inner, Some(7));

    assert_eq!(
        h.frame_value(|ui| scene(ui, true)).inner,
        None,
        "disabled closes it"
    );
    assert!(!PopupTrigger::is_open(&h.ui, trigger_id()));
    assert_eq!(
        h.frame_value(|ui| scene(ui, false)).inner,
        None,
        "and it stays closed once enabled again",
    );

    PopupTrigger::open(&mut h.ui, trigger_id());
    PopupTrigger::close(&mut h.ui, trigger_id());
    assert_eq!(h.frame_value(|ui| scene(ui, false)).inner, None);
}

/// A keyboard-opened popup takes focus and returns it to the trigger on Escape; a click-opened one leaves focus on the trigger.
#[test]
fn a_keyboard_open_moves_focus_in_and_a_click_open_does_not() {
    use crate::input::keyboard::key::Key;
    use crate::widgets::block::Block;

    let item = WidgetId::from_hash("popup-item");
    let record = |ui: &mut Ui| {
        let trigger = Button::new()
            .id(trigger_id())
            .label("open")
            .show(ui)
            .snapshot();
        PopupTrigger::on(&trigger).show(ui, |ui, _| {
            Block::new().id(item).size(20.0).focusable(true).show(ui);
        });
    };

    let mut h = UiHarness::new(SURFACE);
    h.frame(record);
    h.key(Key::Tab);
    h.frame(record);
    assert_eq!(h.focus(), Some(trigger_id()));
    h.key(Key::Enter);
    h.frame(record);
    assert!(PopupTrigger::is_open(&h.ui, trigger_id()));
    assert_eq!(h.focus(), Some(item), "the keyboard's open moves focus in");
    h.key(Key::Escape);
    h.frame(record);
    h.frame(record);
    assert!(!PopupTrigger::is_open(&h.ui, trigger_id()));
    assert_eq!(h.focus(), Some(trigger_id()), "closing gives it back");

    let mut h = UiHarness::new(SURFACE);
    h.frame(record);
    let at = trigger_centre(&h);
    h.click_at(at);
    h.frame(record);
    assert!(PopupTrigger::is_open(&h.ui, trigger_id()));
    h.frame(record);
    assert_eq!(h.focus(), Some(trigger_id()), "a click's open leaves focus");
}
