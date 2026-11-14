use crate::host::harness::Harness;
use crate::primitives::geometry::size::Size;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::{SizeSpec, Sizing};
use crate::primitives::paint::color::RgbaF32;
use crate::scene::layer::Layer;
use crate::shape::paint::lowered_brush::LoweredBrush;
use crate::theme::Theme;
use crate::theme::color_scheme::ColorScheme;
use crate::theme::combo_box::ComboBoxTheme;
use crate::ui::Ui;
use crate::ui::frame_report::FrameProcessing;
use crate::widget_core::configure::Configure;
use crate::widgets::combo_box::{ComboBox, ComboBoxList, option_id};
use crate::widgets::menu::Menu;
use crate::widgets::panel::Panel;
use glam::{UVec2, Vec2};

const SURFACE: UVec2 = UVec2::new(400, 300);

/// A binding no option equals shows its own label, measured against a combo whose one option is
/// that text, and is not written back; with no options at all it shows the same.
#[test]
fn a_value_no_option_equals_shows_its_own_label_without_writing_back() {
    static OPTIONS: [&str; 2] = ["A", "Longer"];
    let ids = [0, 1, 2].map(|i| WidgetId::from_hash(("combo", i)));
    let (empty, literal) = (
        WidgetId::from_hash("combo-empty"),
        WidgetId::from_hash("combo-literal"),
    );
    let missing = "Missing";
    let mut picks = [OPTIONS[0], OPTIONS[1], missing];
    let (mut none, mut alone) = (missing, missing);
    let mut h = Harness::mono(SURFACE);
    h.frame(|ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("root"))
            .size(SizeSpec::FILL)
            .show(ui, |ui| {
                for (selected, id) in picks.iter_mut().zip(ids) {
                    ComboBox::new(selected, OPTIONS).id(id).show(ui);
                }
                ComboBox::new(&mut none, []).id(empty).show(ui);
                ComboBox::new(&mut alone, ["Missing"]).id(literal).show(ui);
            });
    });
    let width = |id: WidgetId| h.rect(id.with("label")).expect("label arranged").size.w;
    let [first, last, unlisted] = ids.map(width);
    assert_ne!(first, last, "premise: the two options measure apart");
    assert_eq!(
        unlisted,
        width(literal),
        "a value no option equals shows its own label"
    );
    assert_eq!(width(empty), width(literal), "so does one over no options");
    assert_eq!(
        picks,
        ["A", "Longer", "Missing"],
        "nothing was written back"
    );
    assert_eq!(none, "Missing");
    assert_eq!(alone, "Missing");
}

/// `labeled` reads the row's projected field, not the row: measured through the trigger label's width, since the two candidate fields differ in length.
#[test]
fn a_labeled_dropdown_reads_the_projected_field() {
    /// A row that is not text, which no `AsRef<str>` could pick between.
    #[derive(Clone, Copy, Debug, PartialEq)]
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
    let (mut a, mut b, mut c) = (rows[0], rows[0], "Elderberry");
    let mut h = Harness::mono(SURFACE);
    h.frame(|ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("root"))
            .size(SizeSpec::FILL)
            .show(ui, |ui| {
                ComboBox::labeled(&mut a, rows, |r| r.display)
                    .id(projected)
                    .show(ui);
                ComboBox::labeled(&mut b, rows, |r| r.name)
                    .id(other)
                    .show(ui);
                ComboBox::new(&mut c, ["Elderberry"]).id(literal).show(ui);
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

/// A caller holding a reference into a table binds the reference: the options yield `&Row`, a
/// pick moves the picked row's reference in, and the row type needs no `Clone`, so nothing is
/// cloned: the bound value is the table's row itself.
#[test]
fn a_reference_binds_and_a_pick_moves_the_rows_reference_in() {
    #[derive(Debug, PartialEq)]
    struct Row {
        name: String,
        idx: u8,
    }
    fn frame<'r>(h: &mut Harness, rows: &'r [Row], picked: &mut &'r Row) -> bool {
        h.frame_value(|ui| {
            ComboBox::labeled(picked, rows.iter(), |row| row.name.as_str())
                .id(WidgetId::from_hash("combo-ref"))
                .show(ui)
                .changed
        })
    }
    let rows = [0, 1, 2].map(|idx| Row {
        name: String::from("Row"),
        idx,
    });
    let id = WidgetId::from_hash("combo-ref");
    let mut picked = &rows[0];
    let mut h = Harness::mono(SURFACE);
    frame(&mut h, &rows, &mut picked);
    ComboBoxList::open(h.ui_mut(), id);
    frame(&mut h, &rows, &mut picked);
    let third = h
        .rect(option_id(ComboBoxList::menu_id(id), 2))
        .expect("premise: the list is open");
    h.click(third.center());
    assert!(frame(&mut h, &rows, &mut picked), "the click picks");
    assert!(
        std::ptr::eq(picked, &raw const rows[2]),
        "{picked:?} is the third row itself"
    );
}

#[test]
fn dropdown_aligns_to_the_full_trigger_rect_when_flipped_above() {
    static OPTIONS: [&str; 3] = ["One", "Two", "Three"];
    let mut h = Harness::mono(SURFACE);
    let id = WidgetId::from_hash("combo");
    let mut selected = OPTIONS[0];
    let build = |ui: &mut Ui, selected: &mut &'static str| {
        Panel::canvas()
            .id(WidgetId::from_hash("root"))
            .size(SizeSpec::FILL)
            .show(ui, |ui| {
                ComboBox::new(selected, OPTIONS)
                    .id(id)
                    .at(Vec2::new(120.0, 250.0))
                    .size((Sizing::fixed(140.0), Sizing::fixed(30.0)))
                    .show(ui);
            });
    };
    h.frame(|ui| build(ui, &mut selected));
    ComboBoxList::open(h.ui_mut(), id);

    assert_eq!(
        h.frame(|ui| build(ui, &mut selected)).processing,
        FrameProcessing::SingleLayout,
        "dropdown placement must converge in one pass"
    );

    let trigger = h.rect(id).expect("combo trigger arranged");
    let list = h
        .rect(ComboBoxList::menu_id(id))
        .expect("combo list arranged");
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

/// The trigger's shape comes from `Theme::combo_box`: the chevron from `arrow_size`, the gutter from `gap`, which a caller's `gap` replaces. Hug-sized so `Justify::SpaceBetween` leaves no slack hiding `gap`.
#[test]
fn trigger_geometry_follows_the_combo_box_theme() {
    #[derive(Debug, PartialEq)]
    struct Trigger {
        arrow: Size,
        gap: f32,
    }

    let options = ["One"];
    let id = WidgetId::from_hash("geom-combo");

    let measure = |arrow: Size, gap: f32, instance: Option<&ComboBoxTheme>, caller: Option<f32>| {
        let mut h = Harness::mono(SURFACE);
        h.ui_mut().theme_mut().combo_box.arrow_size = arrow;
        h.ui_mut().theme_mut().combo_box.gap = gap;
        let mut selected = options[0];
        h.frame(|ui| {
            Panel::hstack()
                .id(WidgetId::from_hash("root"))
                .show(ui, |ui| {
                    let mut combo = ComboBox::new(&mut selected, options)
                        .id(id)
                        .style(instance)
                        .size(SizeSpec::HUG);
                    if let Some(gap) = caller {
                        combo = combo.gap(gap);
                    }
                    combo.show(ui);
                });
        });
        let label = h.rect(id.with("label")).expect("label arranged");
        let arrow_rect = h.rect(id.with("arrow")).expect("arrow arranged");
        Trigger {
            arrow: arrow_rect.size,
            gap: arrow_rect.min.x - label.max().x,
        }
    };

    let a = measure(Size::new(10.0, 6.0), 12.0, None, None);
    assert_eq!(a.arrow, Size::new(10.0, 6.0), "arrow node takes arrow_size");
    assert_eq!(a.gap, 12.0, "gutter is gap");

    let b = measure(Size::new(20.0, 14.0), 30.0, None, None);
    assert_eq!(b.arrow, Size::new(20.0, 14.0));
    assert_eq!(b.gap, 30.0, "gutter is gap");
    assert_ne!(a.arrow, b.arrow);
    assert_ne!(a.gap, b.gap);

    // The same knobs through `style`: the instance wins.
    let instance = ComboBoxTheme {
        arrow_size: Size::new(20.0, 14.0),
        gap: 30.0,
        ..Theme::stock(ColorScheme::Dark).combo_box
    };
    let c = measure(Size::new(10.0, 6.0), 12.0, Some(&instance), None);
    assert_eq!(c, b, "`style` overrides the slot's arrow_size and gap");
    let d = measure(Size::new(10.0, 6.0), 12.0, None, Some(3.0));
    assert_eq!(d.gap, 3.0, "a caller's gap wins over the theme's");
}

/// The trigger paints in `button_style`'s button theme, else `Theme::button`, read off its chrome on the first-record frame (rest state).
#[test]
fn trigger_chrome_follows_button_style() {
    let options = ["One"];
    let id = WidgetId::from_hash("styled-combo");
    let custom = RgbaF32::srgb(0.9, 0.2, 0.1);
    let mut restyled = Theme::stock(ColorScheme::Dark).button;
    restyled.unselected.normal.chrome.paint.fill = custom.into();
    assert_ne!(
        Theme::stock(ColorScheme::Dark)
            .button
            .unselected
            .normal
            .chrome
            .paint
            .fill,
        custom.into(),
        "premise: the custom fill differs from the stock one",
    );
    for style in [None, Some(&restyled)] {
        let mut h = Harness::mono(SURFACE);
        let themed = h
            .ui()
            .theme()
            .button
            .unselected
            .normal
            .chrome
            .paint
            .fill
            .clone();
        let mut selected = options[0];
        h.frame(|ui| {
            ComboBox::new(&mut selected, options)
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
        let fill = h
            .ui()
            .tree(Layer::Main)
            .chrome(trigger)
            .expect("the trigger paints chrome")
            .fill;
        let want = want.to_f16();
        assert!(
            matches!(fill, LoweredBrush::Solid(got) if got == want),
            "styled {}: {fill:?}, want {want:?}",
            style.is_some(),
        );
    }
}

/// The list takes the menu theme's padding and row gap as well as its color, so a combo and a
/// right-click menu from one theme read as one control; `menu_style` restyles it alike. Its
/// height grows by the padding on two edges plus one row gap.
#[test]
fn the_dropdown_takes_the_menu_theme_it_documents() {
    let list_height = |padding: f32, gap: f32, styled: bool| {
        let options = ["One", "Two"];
        let id = WidgetId::from_hash("combo");
        let mut menu = Theme::stock(ColorScheme::Dark).menu;
        menu.padding = Spacing::all(padding);
        menu.gap = gap;

        let mut h = Harness::mono(SURFACE);
        if !styled {
            let theme = Theme {
                menu: menu.clone(),
                ..Theme::stock(ColorScheme::Dark)
            };
            h.ui_mut()
                .set_scheme_theme(ColorScheme::Dark, theme)
                .unwrap();
        }
        let mut selected = options[0];
        let mut build = |ui: &mut Ui| {
            Panel::canvas()
                .id(WidgetId::from_hash("root"))
                .size(SizeSpec::FILL)
                .show(ui, |ui| {
                    ComboBox::new(&mut selected, options)
                        .id(id)
                        .menu_style(styled.then_some(&menu))
                        .at(Vec2::new(40.0, 40.0))
                        .size((Sizing::fixed(140.0), Sizing::fixed(30.0)))
                        .show(ui);
                });
        };
        h.frame(&mut build);
        ComboBoxList::open(h.ui_mut(), id);
        h.frame(&mut build);
        h.rect(ComboBoxList::menu_id(id))
            .expect("combo list arranged")
            .size
            .h
    };

    for styled in [false, true] {
        assert_eq!(
            list_height(11.0, 7.0, styled) - list_height(0.0, 0.0, styled),
            2.0 * 11.0 + 7.0,
            "styled {styled}",
        );
    }
}

/// Disabling an open ComboBox closes it: no list is recorded and a click where a row was picks nothing.
#[test]
fn disabling_an_open_trigger_closes_its_list() {
    static OPTIONS: [&str; 3] = ["One", "Two", "Three"];
    let combo = WidgetId::from_hash("combo");
    let list = ComboBoxList::menu_id(combo);
    let mut selected = OPTIONS[0];
    let record = |h: &mut Harness, disabled: bool, selected: &mut &'static str| {
        h.frame(|ui| {
            Panel::vstack().auto_id().show(ui, |ui| {
                ComboBox::new(selected, OPTIONS)
                    .id(combo)
                    .disabled(disabled)
                    .show(ui);
            });
        });
    };
    let mut h = Harness::mono(SURFACE);
    record(&mut h, false, &mut selected);
    record(&mut h, false, &mut selected);
    h.click(combo);
    record(&mut h, false, &mut selected);
    let rows = h.rect(list).expect("premise: the click opened the list");
    let last_row = Vec2::new(rows.min.x + rows.size.w * 0.5, rows.max().y - 4.0);

    record(&mut h, true, &mut selected);
    assert!(
        h.rect(list).is_none(),
        "the disabled trigger's list is gone"
    );
    h.click(last_row);
    record(&mut h, true, &mut selected);
    assert_eq!(selected, "One", "a click where a row was picks nothing");
}

/// A focused, closed combo box opens on the keys of the WAI-ARIA APG's select-only combobox and
/// changes no pick: Enter, Space, Alt+Down and Down land on the pick, Up and Home on the first
/// row, End on the last. A value no option equals marks no row, so those keys land on the first,
/// and it stays as it is. `open`, `close` and `is_open`
/// drive it from outside.
#[test]
fn the_keys_open_a_closed_list_and_change_no_pick() {
    use crate::primitives::input::keyboard::key::Key;
    use crate::primitives::input::keyboard::modifiers::Modifiers;

    static OPTIONS: [&str; 3] = ["A", "B", "C"];
    static MISSING: &str = "Missing";
    let id = WidgetId::from_hash("combo-keys");
    let mut h = Harness::mono(SURFACE);
    let frame = |h: &mut Harness, selected: &mut &'static str| {
        h.frame_value(|ui| {
            let r = ComboBox::new(selected, OPTIONS).id(id).show(ui);
            (r.changed, r.committed)
        })
    };
    let row = |h: &Harness, label: &str| {
        h.widgets(Some(label))
            .into_iter()
            .find(|widget| widget.id != id && widget.text == label)
            .map(|widget| widget.id)
    };
    // Tab focuses the combo box from the keyboard, as the ring a keyboard open needs.
    let mut pick = OPTIONS[1];
    frame(&mut h, &mut pick);
    h.key(Key::Tab);
    frame(&mut h, &mut pick);
    for start in [OPTIONS[1], MISSING] {
        let mut selected = start;
        // A binding no option equals marks no row, so the keys that land on the pick land first.
        let shown = if OPTIONS.contains(&start) {
            start
        } else {
            OPTIONS[0]
        };
        for (mods, key, lands) in [
            (Modifiers::NONE, Key::Enter, shown),
            (Modifiers::NONE, Key::Char(' '), shown),
            (Modifiers::ALT, Key::ArrowDown, shown),
            (Modifiers::NONE, Key::ArrowDown, shown),
            (Modifiers::NONE, Key::ArrowUp, "A"),
            (Modifiers::NONE, Key::Home, "A"),
            (Modifiers::NONE, Key::End, "C"),
        ] {
            let what = format!("{start}: {mods:?} {key:?}");
            frame(&mut h, &mut selected);
            assert_eq!(
                h.ui().focus(),
                Some(id),
                "{what}: the combo box holds focus"
            );
            h.hold_modifiers(mods);
            h.key(key);
            assert_eq!(frame(&mut h, &mut selected), (false, false), "{what}");
            h.hold_modifiers(Modifiers::NONE);
            assert!(ComboBoxList::is_open(h.ui(), id), "{what} opens it");
            assert!(
                !Menu::is_open(h.ui(), id),
                "{what}: the combo's row is its own, not a menu's"
            );
            frame(&mut h, &mut selected);
            assert_eq!(h.ui().focus(), row(&h, lands), "{what} lands on {lands}");
            assert!(h.ui().focus().is_some(), "{what}: a row holds focus");
            ComboBoxList::close(h.ui_mut(), id);
            frame(&mut h, &mut selected);
        }
        assert_eq!(selected, start, "no key changed the pick");
    }

    let mut selected = OPTIONS[1];
    ComboBoxList::open(h.ui_mut(), id);
    frame(&mut h, &mut selected);
    assert!(ComboBoxList::is_open(h.ui(), id), "an outside open records");
    assert!(h.rect(ComboBoxList::menu_id(id)).is_some());
}

/// Keyboard path: Enter on the focused combo opens it with focus on the first row; Tab and arrows walk the rows in the list it traps; Enter picks and closes; focus returns to the combo.
#[test]
fn the_keyboard_alone_picks_from_the_dropdown() {
    use crate::primitives::input::keyboard::key::Key;

    static OPTIONS: [&str; 3] = ["A", "B", "C"];
    let id = WidgetId::from_hash("combo-walk");
    let mut selected = OPTIONS[0];
    let mut h = Harness::mono(SURFACE);
    let frame = |h: &mut Harness, selected: &mut &'static str| {
        h.frame_value(|ui| ComboBox::new(selected, OPTIONS).id(id).show(ui).changed)
    };
    frame(&mut h, &mut selected);
    h.key(Key::Tab);
    frame(&mut h, &mut selected);
    assert_eq!(h.ui().focus(), Some(id), "Tab reaches the combo box");

    h.key(Key::Enter);
    frame(&mut h, &mut selected);
    assert!(ComboBoxList::is_open(h.ui(), id));
    let first = h.ui().focus().expect("focus moved into the list");
    assert_ne!(first, id);

    h.key(Key::Tab);
    frame(&mut h, &mut selected);
    let second = h.ui().focus().expect("a row holds focus");
    assert_ne!(second, first, "Tab moves to the next row");
    h.key(Key::ArrowDown);
    frame(&mut h, &mut selected);
    let third = h.ui().focus().expect("a row holds focus");
    assert!(third != first && third != second, "Down moves to the third");
    h.key(Key::ArrowUp);
    frame(&mut h, &mut selected);
    assert_eq!(h.ui().focus(), Some(second), "Up moves back");

    h.key(Key::Enter);
    let changed = frame(&mut h, &mut selected);
    assert!(changed && selected == "B", "Enter picks the second row");
    frame(&mut h, &mut selected);
    assert!(
        !ComboBoxList::is_open(h.ui(), id),
        "the pick closes the list"
    );
    assert_eq!(h.ui().focus(), Some(id), "focus goes back to the combo box");
}

/// The trigger lays its label and arrow apart with `SpaceBetween` under the caller's choice: a
/// caller's justify wins, as with every themed default.
#[test]
fn a_callers_justify_wins_over_the_space_between() {
    use crate::primitives::layout::justify::Justify;
    static OPTIONS: [&str; 2] = ["A", "B"];
    let (plain, centered) = (
        WidgetId::from_hash("plain"),
        WidgetId::from_hash("centered"),
    );
    let (mut a, mut b) = (OPTIONS[0], OPTIONS[0]);
    let mut h = Harness::mono(SURFACE);
    h.frame(|ui| {
        Panel::vstack().auto_id().show(ui, |ui| {
            ComboBox::new(&mut a, OPTIONS).id(plain).show(ui);
            ComboBox::new(&mut b, OPTIONS)
                .id(centered)
                .justify(Justify::Center)
                .show(ui);
        });
    });
    let justify = |id| {
        let node = h.node_for_widget_id(id);
        h.ui().tree(Layer::Main).panel(node).justify
    };
    assert_eq!(justify(plain), Justify::SpaceBetween);
    assert_eq!(justify(centered), Justify::Center);
}

/// A combo's list and a context menu an app puts on the same combo are two overlays: both open
/// at once, each records under an id of its own, and no two nodes collide.
#[test]
fn the_list_and_a_context_menu_on_the_combo_do_not_collide() {
    use crate::widgets::menu::menu_item::MenuItem;

    static OPTIONS: [&str; 2] = ["A", "B"];
    let id = WidgetId::from_hash("combo-with-menu");
    let mut selected = OPTIONS[0];
    let record = |ui: &mut Ui, selected: &mut &'static str| {
        Panel::vstack()
            .id(WidgetId::from_hash("root"))
            .size(SizeSpec::FILL)
            .show(ui, |ui| {
                let combo = ComboBox::new(selected, OPTIONS).id(id).show(ui);
                Menu::context(&combo.response).show(ui, |ui| {
                    MenuItem::new("Reset").show(ui);
                });
            });
    };
    let mut h = Harness::mono(SURFACE);
    h.frame(|ui| record(ui, &mut selected));
    ComboBoxList::open(h.ui_mut(), id);
    Menu::open_at(h.ui_mut(), id, Vec2::new(200.0, 200.0));
    h.frame(|ui| record(ui, &mut selected));
    h.frame(|ui| record(ui, &mut selected));
    assert!(ComboBoxList::is_open(h.ui(), id));
    assert!(Menu::is_open(h.ui(), id));
    assert_eq!(
        h.ui().explicit_id_collision_count(),
        0,
        "two overlays, two ids"
    );
    assert!(
        h.rect(ComboBoxList::menu_id(id)).is_some(),
        "the list records"
    );
    assert!(
        h.rect(Menu::menu_id(id)).is_some(),
        "the context menu records"
    );
    assert_ne!(ComboBoxList::menu_id(id), Menu::menu_id(id));
}
