use crate::Ui;
use crate::internals::harness::UiHarness;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::scene::layer::Layer;
use crate::shape::paint::shape_brush::ShapeBrush;
use crate::ui::frame_report::FrameProcessing;
use crate::widget_core::configure::Configure;
use crate::widgets::combo_box::ComboBox;
use crate::widgets::panel::Panel;
use crate::widgets::popup::popup_trigger::PopupTrigger;
use crate::widgets::theme::Theme;
use crate::widgets::theme::button::ButtonTheme;
use crate::widgets::theme::combo_box::ComboBoxTheme;
use glam::{UVec2, Vec2};

const SURFACE: UVec2 = UVec2::new(400, 300);

/// The selection is an index coerced for display: one past the end of the
/// list — a list that shrank under it — shows the last option, measured
/// through the trigger label's width ("Longer" against "A"), and the bound
/// index stays where the caller left it. An empty list shows an empty
/// label rather than panicking.
#[test]
fn a_stale_selection_shows_the_last_option_without_writing_back() {
    const OPTIONS: [&str; 2] = ["A", "Longer"];
    let ids = [0, 1, 9].map(|i| WidgetId::from_hash(("combo", i)));
    let empty = WidgetId::from_hash("combo-empty");
    let mut picks = [0usize, 1, 9];
    let mut none = 4usize;
    let mut h = UiHarness::new(SURFACE);
    h.frame(|ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("root"))
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                for (selected, id) in picks.iter_mut().zip(ids) {
                    ComboBox::new(selected, &OPTIONS).id(id).show(ui);
                }
                ComboBox::new(&mut none, &[] as &[&str]).id(empty).show(ui);
            });
    });
    let width = |id: WidgetId| h.rect(id.with("label")).expect("label arranged").size.w;
    let [first, last, stale] = ids.map(width);
    assert_ne!(first, last, "premise: the two options measure apart");
    assert_eq!(stale, last, "a stale index shows the last option");
    assert_eq!(picks, [0, 1, 9], "nothing was written back");
    assert_eq!(width(empty), 0.0, "an empty list shows an empty label");
    assert_eq!(none, 4);
}

/// `labeled` reads the row's projected field, not the row: a dropdown over
/// records measures exactly as one over the string it projects, and not as
/// one over the row's other string.
///
/// Asserted through the trigger label's width because that is the only
/// thing the option text can reach from outside — and it is enough, since
/// the two candidate fields differ in length.
#[test]
fn a_labeled_dropdown_reads_the_projected_field() {
    /// A row that is not itself text: no `AsRef<str>` impl could pick
    /// between these two, which is the case `labeled` exists for.
    #[derive(Debug)]
    struct Row {
        name: &'static str,
        display: &'static str,
    }
    let rows = [Row {
        name: "a",
        display: "Elderberry",
    }];

    let (projected, other, literal) = (
        WidgetId::from_hash("projected"),
        WidgetId::from_hash("other"),
        WidgetId::from_hash("literal"),
    );
    let (mut a, mut b, mut c) = (0, 0, 0);
    let mut h = UiHarness::new(SURFACE);
    h.frame(|ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("root"))
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                ComboBox::labeled(&mut a, &rows, |r| r.display)
                    .id(projected)
                    .show(ui);
                ComboBox::labeled(&mut b, &rows, |r| r.name)
                    .id(other)
                    .show(ui);
                ComboBox::new(&mut c, &["Elderberry"]).id(literal).show(ui);
            });
    });

    let width = |id: WidgetId| {
        h.rect(id.with("label"))
            .expect("trigger label arranged")
            .size
            .w
    };
    assert_eq!(
        width(projected),
        width(literal),
        "the trigger measured the row's `display`, so it read that field",
    );
    assert_ne!(
        width(projected),
        width(other),
        "and the projection is what chose it — `name` renders differently",
    );
}

#[test]
fn dropdown_aligns_to_the_full_trigger_rect_when_flipped_above() {
    let mut h = UiHarness::new(SURFACE);
    let id = WidgetId::from_hash("combo");
    let options = ["One", "Two", "Three"];
    let mut selected = 0;
    let build = |ui: &mut Ui, selected: &mut usize| {
        Panel::canvas()
            .id(WidgetId::from_hash("root"))
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                ComboBox::new(selected, &options)
                    .id(id)
                    .position(Vec2::new(120.0, 250.0))
                    .size((Sizing::fixed(140.0), Sizing::fixed(30.0)))
                    .show(ui);
            });
    };
    h.frame(|ui| build(ui, &mut selected));
    PopupTrigger::open(&mut h.ui, id);

    assert_eq!(
        h.frame(|ui| build(ui, &mut selected)).processing,
        FrameProcessing::SingleLayout,
        "dropdown placement must converge in one pass"
    );

    let trigger = h.rect(id).expect("combo trigger arranged");
    let list = h.rect(id.with("list")).expect("combo list arranged");
    assert_eq!(list.min.x, trigger.min.x, "list starts at trigger left");
    assert_eq!(
        list.max().y,
        trigger.min.y,
        "above fallback ends at the trigger's top edge",
    );
    assert!(
        list.size.w >= trigger.size.w,
        "list width {} must cover trigger width {}",
        list.size.w,
        trigger.size.w,
    );
}

/// The trigger's shape comes from `Theme::combo_box`, not from
/// constants: the chevron node is sized from `arrow_size`, and the
/// gutter between label and arrow from `gap`.
///
/// Hug-sized so `Justify::SpaceBetween` has no free space to
/// distribute — the rendered gap is then exactly `gap`, which a
/// fixed-width trigger would hide behind the justification slack.
#[test]
fn trigger_geometry_follows_the_combo_box_theme() {
    let options = ["One"];
    let id = WidgetId::from_hash("geom-combo");

    let measure = |arrow: Vec2, gap: f32, instance: Option<&ComboBoxTheme>| -> (Vec2, f32) {
        let mut h = UiHarness::new(SURFACE);
        h.ui.theme_mut().combo_box.arrow_size = arrow;
        h.ui.theme_mut().combo_box.gap = gap;
        let mut selected = 0;
        h.frame(|ui| {
            Panel::hstack()
                .id(WidgetId::from_hash("root"))
                .show(ui, |ui| {
                    ComboBox::new(&mut selected, &options)
                        .id(id)
                        .style(instance)
                        .size((Sizing::HUG, Sizing::HUG))
                        .show(ui);
                });
        });
        let label = h.rect(id.with("label")).expect("label arranged");
        let arrow_rect = h.rect(id.with("arrow")).expect("arrow arranged");
        (
            Vec2::new(arrow_rect.size.w, arrow_rect.size.h),
            arrow_rect.min.x - label.max().x,
        )
    };

    let (size_a, gap_a) = measure(Vec2::new(10.0, 6.0), 12.0, None);
    assert_eq!(size_a, Vec2::new(10.0, 6.0), "arrow node takes arrow_size");
    assert_eq!(gap_a, 12.0, "gutter is gap, got {gap_a}");

    // Both knobs move the layout — neither is baked in.
    let (size_b, gap_b) = measure(Vec2::new(20.0, 14.0), 30.0, None);
    assert_eq!(size_b, Vec2::new(20.0, 14.0));
    assert_eq!(gap_b, 30.0, "gutter is gap, got {gap_b}");
    assert_ne!(size_a, size_b);
    assert_ne!(gap_a, gap_b);

    // The same two knobs through `style`, against a slot set the other
    // way: the instance is what lands.
    let instance = ComboBoxTheme {
        arrow_size: Vec2::new(20.0, 14.0),
        gap: 30.0,
        ..ComboBoxTheme::default()
    };
    let (size_c, gap_c) = measure(Vec2::new(10.0, 6.0), 12.0, Some(&instance));
    assert_eq!(size_c, size_b, "`style` overrides the slot's arrow_size");
    assert_eq!(gap_c, gap_b, "`style` overrides the slot's gap");
}

/// The trigger paints in the button theme `button_style` names, and in
/// `Theme::button` without one — read off the trigger's chrome on the
/// frame it first records, where the look snaps to its rest state.
#[test]
fn trigger_chrome_follows_button_style() {
    let options = ["One"];
    let id = WidgetId::from_hash("styled-combo");
    let custom = RgbaF32::srgb(0.9, 0.2, 0.1);
    let mut restyled = ButtonTheme::default();
    restyled.looks.normal.background.fill = custom.into();
    assert_ne!(
        ButtonTheme::default().looks.normal.background.fill,
        custom.into(),
        "premise: the custom fill differs from the stock one",
    );
    for style in [None, Some(&restyled)] {
        let mut h = UiHarness::new(SURFACE);
        let themed = h.ui.theme().button.looks.normal.background.fill.clone();
        let mut selected = 0;
        h.frame(|ui| {
            ComboBox::new(&mut selected, &options)
                .id(id)
                .button_style(style)
                .show(ui);
        });
        let want = match style {
            Some(_) => custom,
            None => themed
                .as_solid()
                .expect("the stock button rests on a solid fill"),
        };
        let trigger = h.node_of(id).expect("trigger recorded").node;
        let fill =
            h.ui.tree(Layer::Main)
                .chrome(trigger)
                .expect("the trigger paints chrome")
                .fill;
        let want = RgbaF16::from(want);
        assert!(
            matches!(fill, ShapeBrush::Solid(got) if got == want),
            "styled {}: {fill:?}, want {want:?}",
            style.is_some(),
        );
    }
}

/// The list is the context menu's panel, not merely its colour: it takes
/// the menu theme's padding and row gap as well, so a combo and a
/// right-click menu built from one theme read as one control. It applied
/// only the background before, and rendered visibly tighter than the
/// menu it claims to reuse.
///
/// Differential, against the same two options: the list's height grows by
/// exactly the padding it gained on two edges plus the one gap between
/// two rows.
#[test]
fn the_dropdown_takes_the_context_menu_theme_it_documents() {
    let list_height = |padding: f32, gap: f32| {
        let options = ["One", "Two"];
        let id = WidgetId::from_hash("combo");
        let mut theme = Theme::default();
        theme.context_menu.padding = Spacing::all(padding);
        theme.context_menu.gap = gap;

        let mut h = UiHarness::new(SURFACE);
        h.ui.set_theme(theme);
        let mut selected = 0;
        let build = |ui: &mut Ui, selected: &mut usize| {
            Panel::canvas()
                .id(WidgetId::from_hash("root"))
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    ComboBox::new(selected, &options)
                        .id(id)
                        .position(Vec2::new(40.0, 40.0))
                        .size((Sizing::fixed(140.0), Sizing::fixed(30.0)))
                        .show(ui);
                });
        };
        h.frame(|ui| build(ui, &mut selected));
        PopupTrigger::open(&mut h.ui, id);
        h.frame(|ui| build(ui, &mut selected));
        h.rect(id.with("list")).expect("combo list arranged").size.h
    };

    // Two edges of padding, and one gap between the two rows.
    assert_eq!(
        list_height(11.0, 7.0) - list_height(0.0, 0.0),
        2.0 * 11.0 + 7.0,
    );
}

/// Disabling an open ComboBox closes it: the next frame records no list,
/// and a click where a row used to be picks nothing.
#[test]
fn disabling_an_open_trigger_closes_its_list() {
    let combo = WidgetId::from_hash("combo");
    let list = combo.with("list");
    let options = ["One", "Two", "Three"];
    let mut selected = 0;
    let record = |h: &mut UiHarness, disabled: bool, selected: &mut usize| {
        h.frame(|ui| {
            Panel::vstack().auto_id().show(ui, |ui| {
                ComboBox::new(selected, &options)
                    .id(combo)
                    .disabled(disabled)
                    .show(ui);
            });
        });
    };
    let mut h = UiHarness::new(SURFACE);
    record(&mut h, false, &mut selected);
    record(&mut h, false, &mut selected);
    h.click_on(combo);
    record(&mut h, false, &mut selected);
    let rows = h.rect(list).expect("premise: the click opened the list");
    let last_row = Vec2::new(rows.min.x + rows.size.w * 0.5, rows.max().y - 4.0);

    record(&mut h, true, &mut selected);
    assert!(
        h.rect(list).is_none(),
        "the disabled trigger's list is gone"
    );
    h.click_at(last_row);
    record(&mut h, true, &mut selected);
    assert_eq!(selected, 0, "a click where a row was picks nothing");
}

/// A focused, closed combo box steps its pick with the arrows, stopping at
/// the ends, and reports each step as a committed change; Enter, Space and
/// Alt+Down open it, and the arrows step nothing while it is open.
#[test]
fn the_keys_step_a_closed_pick_and_open_the_list() {
    use crate::input::keyboard::key::Key;
    use crate::input::keyboard::modifiers::Modifiers;

    const OPTIONS: [&str; 3] = ["A", "B", "C"];
    let id = WidgetId::from_hash("combo-keys");
    let mut selected = 1usize;
    let mut h = UiHarness::new(SURFACE);
    let frame = |h: &mut UiHarness, selected: &mut usize| {
        h.frame_value(|ui| {
            let r = ComboBox::new(selected, &OPTIONS).id(id).show(ui);
            (r.changed, r.committed)
        })
    };
    frame(&mut h, &mut selected);
    h.set_focus(id);
    frame(&mut h, &mut selected);
    for (key, want, moved) in [
        (Key::ArrowDown, 2, true),
        (Key::ArrowDown, 2, false),
        (Key::ArrowUp, 1, true),
        (Key::ArrowUp, 0, true),
        (Key::ArrowUp, 0, false),
    ] {
        h.key(key);
        assert_eq!(frame(&mut h, &mut selected), (moved, moved), "{key:?}");
        assert_eq!(selected, want, "{key:?}");
    }
    for (mods, key) in [
        (Modifiers::NONE, Key::Enter),
        (Modifiers::NONE, Key::Char(' ')),
        (Modifiers::ALT, Key::ArrowDown),
    ] {
        h.set_modifiers(mods);
        h.key(key);
        frame(&mut h, &mut selected);
        h.set_modifiers(Modifiers::NONE);
        assert!(
            PopupTrigger::is_open(&h.ui, id),
            "{mods:?} {key:?} opens it"
        );
        h.key(Key::ArrowDown);
        frame(&mut h, &mut selected);
        assert_eq!(selected, 0, "an open list steps nothing");
        PopupTrigger::close(&mut h.ui, id);
        frame(&mut h, &mut selected);
        h.set_focus(id);
        frame(&mut h, &mut selected);
    }
}
