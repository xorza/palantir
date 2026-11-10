use crate::animation::animation_spec::AnimationSpec;
use crate::host::harness::Harness;
use crate::theme::Theme;
use crate::theme::color_scheme::ColorScheme;

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
            ..Theme::stock(ColorScheme::Dark).button.defaults
        },
        ..Theme::stock(ColorScheme::Dark).button
    };
    theme.unselected.normal.chrome = Chrome::NONE;

    let mut h = Harness::mono(UVec2::new(200, 120));
    let [explicit, inherited] = h.frame_value(|ui| {
        [
            Button::new()
                .style(&theme)
                .padding(Spacing::ZERO)
                .margin(Spacing::ZERO)
                .show(ui)
                .node(ui),
            Button::new().style(&theme).show(ui).node(ui),
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

/// A Button is a Tab stop a click focuses; a focused one is clicked by Space and Enter once per press via `clicked()`, while an unfocused or disabled one takes neither. It takes no other key from the app's scope around it: a letter reaches the app, and Enter does not.
#[test]
fn space_and_enter_click_a_focused_button() {
    use crate::primitives::identity::widget_id::WidgetId;
    use crate::primitives::input::key_class::KeyFilter;
    use crate::primitives::input::keyboard::key::Key;
    use crate::primitives::input::shortcut::{Shortcut, ShortcutMods};
    use crate::ui::Ui;
    use crate::widgets::panel::Panel;

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
    assert_eq!(h.ui().focus(), Some(id), "a click focuses a button");
    for key in [Key::Char(' '), Key::Enter] {
        h.key(key);
        assert!(h.frame_value(record(false)), "{key:?} clicks it");
        assert!(
            !h.frame_value(record(false)),
            "{key:?}: one click per press"
        );
    }
    // A key acts by the class the scope takes: Shift keeps Space and Enter in their classes, so
    // they click; Ctrl makes a chord, which the button leaves to the app.
    for (shortcut, clicks) in [
        (Shortcut::new(ShortcutMods::SHIFT, Key::Char(' ')), true),
        (Shortcut::new(ShortcutMods::SHIFT, Key::Enter), true),
        (Shortcut::new(ShortcutMods::CTRL, Key::Char(' ')), false),
    ] {
        h.key(shortcut);
        assert_eq!(h.frame_value(record(false)), clicks, "{shortcut:?}");
    }
    // The button takes Space and Enter alone from the app's scope around it: a letter reaches
    // the app, Enter does not.
    let with_app = |ui: &mut Ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("app"))
            .input_scope(KeyFilter::ALL)
            .show(ui, |ui| {
                let clicked = record(false)(ui);
                [
                    clicked,
                    ui.key_pressed(Key::Char('f')),
                    ui.key_pressed(Key::Enter),
                ]
            })
            .inner
    };
    let _ = h.frame_value(with_app);
    h.key(Key::Char('f'));
    assert_eq!(
        h.frame_value(with_app),
        [false, true, false],
        "f reaches the app"
    );
    h.key(Key::Enter);
    assert_eq!(
        h.frame_value(with_app),
        [true, false, false],
        "Enter clicks the button, and the app gets none"
    );
    // With no scope of the app's own, the button's is the only one: a read outside it gets what
    // the button does not take, as the app's scope did.
    let bare = |ui: &mut Ui| {
        let clicked = record(false)(ui);
        [
            clicked,
            ui.key_pressed(Key::Char('f')),
            ui.key_pressed(Key::Enter),
        ]
    };
    let _ = h.frame_value(bare);
    h.key(Key::Char('f'));
    assert_eq!(
        h.frame_value(bare),
        [false, true, false],
        "f reaches the app root"
    );
    h.key(Key::Enter);
    assert_eq!(
        h.frame_value(bare),
        [true, false, false],
        "Enter clicks the button, and the app root gets none"
    );
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
        unselected: pack(0x100),
        selected: pack(0x200),
        defaults: SlotDefaults {
            transition: AnimationSpec::SNAP,
            ..Theme::stock(ColorScheme::Dark).button.defaults
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

/// A label wider than its button's `Fixed` width truncates to one line (`TextWrap::Truncate`)
/// rather than overflow or wrap, cut to the 80 px button less 2 × 12 padding and 2 × 1 border:
/// 54 px of room.
#[test]
fn a_label_truncates_to_one_line_in_a_narrow_button() {
    use crate::primitives::geometry::size::Size;
    use crate::primitives::layout::sizing::Sizing;
    use crate::shape::record::ShapeRecord;
    use crate::text::wrap::TextWrap;
    use crate::widgets::panel::Panel;

    let mut h = Harness::mono(UVec2::new(400, 400));
    let node = h.frame_value(|ui| {
        Panel::vstack()
            .auto_id()
            .size((Sizing::fixed(80.0), Sizing::HUG))
            .show(ui, |ui| {
                Button::new()
                    .auto_id()
                    .label("the quick brown fox")
                    .show(ui)
                    .node(ui)
            })
            .inner
    });
    let wrap = h
        .ui()
        .tree(Layer::Main)
        .shapes_of(node)
        .find_map(|s| match s {
            ShapeRecord::Text { wrap, .. } => Some(*wrap),
            _ => None,
        })
        .expect("a text shape");
    assert_eq!(
        wrap,
        TextWrap::Truncate,
        "a button label truncates by default"
    );

    // One line: the stock 19.203125 px line, its block rounded up to 20 px.
    assert_eq!(h.ui().theme().text.line_height(), 19.203_125);
    let layout = h.ui().plane_layout(Layer::Main);
    let span = layout.text_spans[node.idx()];
    assert_eq!(span.len, 1);
    assert_eq!(
        layout.text_shapes[span.start as usize].extent.size,
        Size::new(54.0, 20.0)
    );
}

/// A Space under a held left press on the focused button adds no click: the press keeps its
/// gesture, its drag included, and makes its own edge on release, so no frame reads a release
/// beside a live drag.
#[test]
fn an_activation_key_under_a_held_press_keeps_the_gesture() {
    use crate::primitives::identity::widget_id::WidgetId;
    use crate::primitives::input::interaction::button_phase::ButtonPhase;
    use crate::primitives::input::interaction::button_state::ButtonState;
    use crate::primitives::input::interaction::drag::Drag;
    use crate::primitives::input::keyboard::key::Key;
    use crate::primitives::input::pointer::PointerButton;
    use crate::ui::Ui;
    use glam::Vec2;

    let id = WidgetId::from_hash("button");
    let record = |ui: &mut Ui| Button::new().id(id).size((80.0, 30.0)).show(ui).left;
    let mut h = Harness::mono(UVec2::new(200, 80));
    let _ = h.frame_value(record);
    let at = h.center_of(id);
    h.click(at);
    let _ = h.frame_value(record);
    assert_eq!(h.ui().focus(), Some(id));

    h.press(at, PointerButton::Left);
    h.drag_to(at + Vec2::new(10.0, 0.0));
    let _ = h.frame_value(record);
    h.key(Key::Char(' '));
    let held = h.frame_value(record);
    assert_eq!(held.phase, ButtonPhase::Held, "the press holds");
    assert_eq!(
        held.drag,
        Drag::Active {
            delta: Vec2::new(10.0, 0.0)
        },
        "its drag runs on"
    );
    assert!(!held.clicked(), "Space adds no click under it");

    h.release(PointerButton::Left);
    assert_eq!(
        h.frame_value(record),
        ButtonState::new(ButtonPhase::Up { click: None }, Drag::Stopped),
        "the release ends the drag with no click"
    );
}

mod press {
    use std::time::Duration;

    use glam::{UVec2, Vec2};

    use crate::animation::animation_spec::AnimationSpec;
    use crate::effect::press_input::PressInput;
    use crate::host::harness::Harness;
    use crate::internals::paint_capture::PaintCall;
    use crate::primitives::effect::effect_name::EffectName;
    use crate::primitives::geometry::corners::Corners;
    use crate::primitives::geometry::coverage::Coverage;
    use crate::primitives::geometry::rect::Rect;
    use crate::primitives::identity::effect_id::EffectId;
    use crate::primitives::identity::widget_id::WidgetId;
    use crate::primitives::input::keyboard::key::Key;
    use crate::primitives::input::pointer::PointerButton;
    use crate::primitives::paint::color::RgbaF32;
    use crate::shape::geometry::box_geometry::PlacedBox;
    use crate::shape::geometry::closed_geometry::PlacedClosed;
    use crate::theme::Theme;
    use crate::theme::look::press_effect::PressEffect;
    use crate::ui::Ui;
    use crate::widget_core::configure::Configure;
    use crate::widget_core::press_list::PressList;
    use crate::widgets::button::Button;

    const fn ms(ms: u64) -> Duration {
        Duration::from_millis(ms)
    }

    /// The stock ripples' ids: loaded first, the flat one before the sparkle that names it.
    const RIPPLE_ID: EffectId = EffectId(0);
    const SPARKLE_ID: EffectId = EffectId(1);
    const COLOR: RgbaF32 = RgbaF32::new(0.25, 0.5, 0.75, 0.125);
    const HIGHLIGHT: RgbaF32 = RgbaF32::new(0.5, 0.25, 0.125, 0.75);

    /// One press effect draw: its effect, the time it reads, its rect and corners, and what it
    /// reads of the press.
    #[derive(Debug, PartialEq)]
    struct PressDraw {
        effect: EffectId,
        time: f32,
        rect: Rect,
        corners: Corners,
        input: PressInput,
    }

    /// The press effect draws of the frame `h` shows.
    fn press_draws(h: &Harness) -> Vec<PressDraw> {
        let scene = h.ui().frame_scene();
        let words = &scene.forest.record_store.effect_params;
        h.encode_paint()
            .calls
            .iter()
            .filter_map(|call| match call {
                PaintCall::Effect(p) => {
                    let PlacedClosed::Box {
                        placed: PlacedBox { rect, corners },
                        coverage: Coverage::Inside,
                    } = p.geometry
                    else {
                        panic!("a press effect paints its button's rect");
                    };
                    Some(PressDraw {
                        effect: p.paint.effect,
                        time: p.paint.time,
                        rect,
                        corners,
                        input: bytemuck::pod_read_unaligned(bytemuck::cast_slice(
                            &words[p.paint.params.range()],
                        )),
                    })
                }
                _ => None,
            })
            .collect()
    }

    /// A window whose buttons press with `effect` in Material's times, 0.225 s in and 0.15 s
    /// out, and the button it records, 80 × 30 at the top left; one frame shown at 0.
    fn pressing(effect: EffectName<PressInput>) -> (Harness, impl FnMut(&mut Ui)) {
        let mut h = Harness::mono(UVec2::new(200, 80));
        let scheme = h.ui().color_scheme();
        let mut theme = Theme::stock(scheme);
        theme.transition = AnimationSpec::SNAP;
        theme.button.defaults.press = Some(PressEffect {
            effect,
            color: COLOR,
            highlight: HIGHLIGHT,
            enter: ms(225),
            exit: ms(150),
        });
        h.ui_mut().set_theme(scheme, theme).unwrap();
        let id = WidgetId::from_hash("button");
        let record = move |ui: &mut Ui| {
            Button::new().id(id).size((80.0, 30.0)).show(ui);
        };
        let mut record = record;
        h.frame(&mut record);
        (h, record)
    }

    /// What the stock theme's button reads of a press at (30, 10), 1 s in, `released` s after
    /// it.
    fn input(released: f32) -> PressInput {
        PressInput {
            color: COLOR,
            highlight: HIGHLIGHT,
            point: Vec2::new(30.0, 10.0),
            released,
            enter: 0.225,
            exit: 0.15,
            seed: 1.0,
            pad0: 0.0,
            pad1: 0.0,
        }
    }

    /// A press draws its ripple over the button's rect and inside its corners, from the point it
    /// landed at, its time running from the press: 0 at the press, 0.1 s later 0.1, held at the
    /// enter's 0.225 while the button stays down. The release at 1.3 s, 0.3 s in, fades over 0.15
    /// s from itself, as the enter ran: 0.4 at 1.4 s. A press 1 s in seeds 1 s mod 64 = 1.
    #[test]
    fn a_press_draws_its_effect_timed_from_the_press() {
        let (mut h, mut record) = pressing(EffectName::RIPPLE);
        assert_eq!(press_draws(&h), [], "no press, no draw");
        let id = WidgetId::from_hash("button");
        let rect = h.ui().response_for(id).rect.unwrap();
        let corners = h.ui().theme().button.unselected.active.chrome.corners;
        h.at(ms(1000));
        h.press(Vec2::new(30.0, 10.0), PointerButton::Left);
        let draw = |time: f32, released: f32| PressDraw {
            effect: RIPPLE_ID,
            time,
            rect,
            corners,
            input: input(released),
        };
        for (at, time) in [(1000, 0.0), (1100, 0.1), (1300, 0.225)] {
            h.at(ms(at)).frame(&mut record);
            assert_eq!(press_draws(&h), [draw(time, -1.0)], "held, at {at} ms");
        }
        h.release(PointerButton::Left);
        h.at(ms(1300)).frame(&mut record);
        assert_eq!(press_draws(&h), [draw(0.3, 0.3)], "released");
        h.at(ms(1400)).frame(&mut record);
        assert_eq!(press_draws(&h), [draw(0.4, 0.3)], "fading");
        h.at(ms(1450)).frame(&mut record);
        assert_eq!(press_draws(&h), [], "a record at its end, 1.45 s, drops it");
        assert!(
            h.ui().state::<PressList>(id).is_none(),
            "no live press, no row"
        );
    }

    /// A button no one pressed holds no press row. A press held when the button turns disabled
    /// ends at that frame, as a disabled state reads no button: disabled at 1.1 s, 0.1 s in, it
    /// fades from the enter's end, so it reads 0.1 with `released` 0.1, not a ripple held on.
    #[test]
    fn a_press_ends_where_its_widget_turns_disabled() {
        let (mut h, mut record) = pressing(EffectName::RIPPLE);
        let id = WidgetId::from_hash("button");
        assert!(h.ui().state::<PressList>(id).is_none(), "never pressed");
        h.at(ms(1000));
        h.press(Vec2::new(30.0, 10.0), PointerButton::Left);
        h.frame(&mut record);
        let mut disabled = |ui: &mut Ui| {
            Button::new()
                .id(id)
                .size((80.0, 30.0))
                .disabled(true)
                .show(ui);
        };
        h.at(ms(1100)).frame(&mut disabled);
        let read: Vec<(f32, f32)> = press_draws(&h)
            .iter()
            .map(|draw| (draw.time, draw.input.released))
            .collect();
        assert_eq!(read, [(0.1, 0.1)]);
    }

    /// The activation key on a focused button taps it: a press at its center, (40, 15), released
    /// in its own frame, so it reads 0 with `released` 0.
    #[test]
    fn an_activation_key_taps_the_center() {
        let (mut h, mut record) = pressing(EffectName::RIPPLE);
        let id = WidgetId::from_hash("button");
        h.ui_mut().set_focus(id);
        h.frame(&mut record);
        h.at(ms(1000));
        h.key(Key::Char(' '));
        h.frame(&mut record);
        let read: Vec<(Vec2, f32, f32)> = press_draws(&h)
            .iter()
            .map(|draw| (draw.input.point, draw.time, draw.input.released))
            .collect();
        assert_eq!(read, [(Vec2::new(40.0, 15.0), 0.0, 0.0)]);
    }

    /// Under reduced motion a press shows its end at once, as any decorative pass does: held, the
    /// grown ripple at the enter's 0.225; released at 1.1 s, 0.1 s in, its end, max(0.1, 0.225)
    /// + 0.15 = 0.375, where the effect paints nothing.
    #[test]
    fn reduced_motion_shows_a_press_at_its_end() {
        let (mut h, mut record) = pressing(EffectName::RIPPLE);
        h.set_system_motion_reduced(true);
        h.at(ms(1000));
        h.press(Vec2::new(30.0, 10.0), PointerButton::Left);
        h.frame(&mut record);
        let times = |h: &Harness| -> Vec<(f32, f32)> {
            press_draws(h)
                .iter()
                .map(|draw| (draw.time, draw.input.released))
                .collect()
        };
        assert_eq!(times(&h), [(0.225, -1.0)], "held");
        h.release(PointerButton::Left);
        h.at(ms(1100)).frame(&mut record);
        assert_eq!(times(&h), [(0.375, 0.1)], "released");
    }

    /// Under the power saver the sparkle ripple paints the flat one its source names, with the
    /// same press: id 1 at full power, id 0 saved.
    #[test]
    fn the_power_saver_swaps_the_sparkle_for_the_flat_ripple() {
        let (mut h, mut record) = pressing(EffectName::SPARKLE);
        h.at(ms(1000));
        h.press(Vec2::new(30.0, 10.0), PointerButton::Left);
        h.frame(&mut record);
        let effects = |h: &Harness| -> Vec<EffectId> {
            press_draws(h).iter().map(|draw| draw.effect).collect()
        };
        assert_eq!(effects(&h), [SPARKLE_ID]);
        h.set_system_power_reduced(true);
        h.at(ms(1050)).frame(&mut record);
        assert_eq!(effects(&h), [RIPPLE_ID]);
    }
}
