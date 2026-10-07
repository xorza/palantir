//! Row pitch, the shortcut gutter, separators, radius, and per-instance overrides of each.

use crate::Ui;
use crate::input::shortcut::Shortcut;
use crate::internals::harness::UiHarness;
use crate::primitives::geometry::size::Size;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::paint::background::Background;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::scene::layer::Layer;
use crate::shape::paint::shape_brush::ShapeBrush;
use crate::widget_core::configure::Configure;
use crate::widget_core::widget_look::theme_slot::SlotDefaults;
use crate::widgets::context_menu::ContextMenu;
use crate::widgets::context_menu::menu_item::MenuItem;
use crate::widgets::context_menu::menu_separator::MenuSeparator;
use crate::widgets::context_menu::tests::support::{
    MenuRow, SURFACE, menu_body, menu_rows, trigger_id,
};
use crate::widgets::theme::context_menu::ContextMenuTheme;
use crate::widgets::theme::context_menu::menu_item::MenuItemTheme;
use crate::widgets::theme::separator::SeparatorTheme;
use glam::Vec2;

/// Both menu gutters are theme knobs: `context_menu.gap` is the row pitch, `context_menu.item.gap` the label-to-shortcut floor; each moves only its own axis.
#[test]
fn theme_gaps_drive_row_pitch_and_shortcut_gutter() {
    fn menu(ui: &mut Ui) {
        ContextMenu::for_id(trigger_id()).show(ui, |ui, popup| {
            MenuItem::new("Copy")
                .shortcut(Shortcut::ctrl('C'))
                .show(ui, popup);
            MenuItem::new("Paste").show(ui, popup);
        });
    }

    let mut h = UiHarness::new(SURFACE);
    // Hug the content, or the theme's 160 px floor absorbs the widened gutter.
    h.ui.theme_mut().context_menu.min_width = 0.0;
    ContextMenu::open(&mut h.ui, trigger_id(), Vec2::new(20.0, 20.0));
    h.frame(menu);
    let before = menu_rows(&h, trigger_id());
    assert_eq!(before.len(), 2, "two rows recorded");

    h.ui.theme_mut().context_menu.gap += 6.0;
    h.ui.theme_mut().context_menu.item.gap += 10.0;
    h.frame(menu);
    let after = menu_rows(&h, trigger_id());

    for (i, (a, b)) in after.iter().zip(&before).enumerate() {
        assert_eq!(a.rect.size.w - b.rect.size.w, 10.0, "row {i} width");
        assert_eq!(a.rect.size.h, b.rect.size.h, "row {i} height");
    }
    let pitch_before = before[1].rect.min.y - before[0].rect.min.y;
    let pitch_after = after[1].rect.min.y - after[0].rect.min.y;
    assert_eq!(
        pitch_after - pitch_before,
        6.0,
        "row pitch grows by the menu gap delta",
    );
}

/// An explicit `.gap(0.0)` differs from never setting one: flush rows at `0.0`, the theme gap otherwise.
#[test]
fn an_explicit_zero_gap_beats_the_theme_default() {
    fn rows(h: &mut UiHarness, gap: Option<f32>) -> Vec<MenuRow> {
        h.frame(|ui| {
            let mut menu = ContextMenu::for_id(trigger_id());
            if let Some(g) = gap {
                menu = menu.gap(g);
            }
            menu.show(ui, |ui, popup| {
                MenuItem::new("Copy").show(ui, popup);
                MenuItem::new("Paste").show(ui, popup);
            });
        });
        menu_rows(h, trigger_id())
    }

    let mut h = UiHarness::new(SURFACE);
    h.ui.theme_mut().context_menu.gap = 7.0;
    ContextMenu::open(&mut h.ui, trigger_id(), Vec2::new(20.0, 20.0));

    let unset = rows(&mut h, None);
    assert_eq!(unset.len(), 2);
    let unset_pitch = unset[1].rect.min.y - unset[0].rect.min.y;
    assert_eq!(unset_pitch, unset[0].rect.size.h + 7.0, "themed pitch");

    let zeroed = rows(&mut h, Some(0.0));
    let zero_pitch = zeroed[1].rect.min.y - zeroed[0].rect.min.y;
    assert_eq!(zero_pitch, zeroed[0].rect.size.h, "explicit 0.0 pitch");
    assert_ne!(unset_pitch, zero_pitch);

    let wide = rows(&mut h, Some(20.0));
    let wide_pitch = wide[1].rect.min.y - wide[0].rect.min.y;
    assert_eq!(
        wide_pitch,
        wide[0].rect.size.h + 20.0,
        "explicit 20.0 pitch",
    );
}

/// `MenuSeparator` wears `context_menu.separator`, never `theme.separator`.
#[test]
fn menu_separator_theme_drives_rule_geometry_and_color() {
    fn menu(ui: &mut Ui) {
        ContextMenu::for_id(trigger_id()).show(ui, |ui, popup| {
            MenuItem::new("Copy").show(ui, popup);
            MenuSeparator::new().show(ui);
            MenuItem::new("Paste").show(ui, popup);
        });
    }

    let mut h = UiHarness::new(SURFACE);
    let rule = RgbaF32::hex(0xff00ff);
    h.ui.theme_mut().context_menu.separator = SeparatorTheme {
        color: rule,
        thickness: 3.0,
        margin: Spacing::xy(0.0, 7.0),
    };
    h.ui.theme_mut().separator.thickness = 11.0;
    h.ui.theme_mut().separator.color = RgbaF32::hex(0x00ff00);

    ContextMenu::open(&mut h.ui, trigger_id(), Vec2::new(20.0, 20.0));
    h.frame(menu);
    let rows = menu_rows(&h, trigger_id());
    assert_eq!(rows.len(), 3, "two rows plus the separator between them");
    let [first, sep, second] = [rows[0], rows[1], rows[2]];

    assert_eq!(sep.rect.size.h, 3.0, "thickness is the rule's height");
    assert_eq!(
        sep.rect.min.y - first.rect.max().y,
        7.0,
        "margin.top clears the row above",
    );
    assert_eq!(
        second.rect.min.y - sep.rect.max().y,
        7.0,
        "margin.bottom clears the row below",
    );

    let chrome =
        h.ui.tree(Layer::Menu)
            .chrome(sep.node)
            .expect("separator chrome");
    let ShapeBrush::Solid(fill) = chrome.fill else {
        panic!("the menu rule paints a solid fill");
    };
    assert_eq!(fill, RgbaF16::from(rule), "rule color comes off the menu");
}

/// `.style(...)` beats the global slot on every menu widget and writes nothing back; rows resolve through `WidgetTheme::plan`, so theme `padding`/`margin` fill where the builder was silent and an explicit value wins.
#[test]
fn per_instance_style_overrides_global_menu_theme() {
    let custom = ContextMenuTheme {
        padding: Spacing::all(13.0),
        min_width: 220.0,
        item: MenuItemTheme {
            defaults: SlotDefaults {
                padding: Spacing::all(9.0),
                margin: Spacing::xy(2.0, 6.0),
                ..MenuItemTheme::default().defaults
            },
            ..MenuItemTheme::default()
        },
        separator: SeparatorTheme {
            thickness: 5.0,
            ..SeparatorTheme::default()
        },
        ..ContextMenuTheme::default()
    };

    let mut h = UiHarness::new(SURFACE);
    ContextMenu::open(&mut h.ui, trigger_id(), Vec2::new(20.0, 20.0));
    h.frame(|ui| {
        ContextMenu::for_id(trigger_id())
            .style(&custom)
            .show(ui, |ui, popup| {
                MenuItem::new("Copy").style(&custom.item).show(ui, popup);
                MenuSeparator::new().style(&custom.separator).show(ui);
                MenuItem::new("Bare")
                    .style(&custom.item)
                    .padding(Spacing::ZERO)
                    .margin(Spacing::ZERO)
                    .show(ui, popup);
            });
    });

    let body = menu_body(&h, trigger_id());
    let rows = menu_rows(&h, trigger_id());
    let tree = h.ui.tree(Layer::Menu);
    let layout = tree.records.layout();
    // Recorded padding is the styled 13 plus the panel's 1 px stroke, which `Tree` folds in.
    assert_eq!(
        layout[body.idx()].padding,
        Spacing::all(14.0),
        "panel padding"
    );
    assert_eq!(
        tree.bounds(body).min_size,
        Size::new(220.0, 0.0),
        "width floor"
    );
    assert_eq!(
        layout[rows[0].node.idx()].padding,
        Spacing::all(9.0),
        "row padding"
    );
    assert_eq!(
        layout[rows[0].node.idx()].margin,
        Spacing::xy(2.0, 6.0),
        "row margin"
    );
    assert_eq!(rows[1].rect.size.h, 5.0, "rule thickness");
    assert_eq!(
        layout[rows[2].node.idx()].padding,
        Spacing::ZERO,
        "explicit row padding wins over the theme's 9"
    );
    assert_eq!(
        layout[rows[2].node.idx()].margin,
        Spacing::ZERO,
        "explicit row margin wins over the theme's 2/6"
    );

    let default = ContextMenuTheme::default();
    let global = &h.ui.theme().context_menu;
    assert_eq!(global.padding, default.padding);
    assert_eq!(global.min_width, default.min_width);
    assert_eq!(global.item.defaults.padding, default.item.defaults.padding);
    assert_eq!(global.separator.thickness, default.separator.thickness);
}

/// The menu body takes its box from [`Configure`] (including `margin`); the id derives from the trigger, but an explicit `.id(...)` must win.
#[test]
fn explicit_zero_padding_and_minimum_override_menu_theme() {
    let mut h = UiHarness::new(SURFACE);
    ContextMenu::open(&mut h.ui, trigger_id(), Vec2::new(60.0, 60.0));
    h.frame(|ui| {
        ContextMenu::for_id(trigger_id())
            .background(Background::NONE)
            .padding(Spacing::ZERO)
            .margin(Spacing::all(5.0))
            .min_size(Size::ZERO)
            .show(ui, |_, _| {});
    });

    let derived = trigger_id().with("body");
    let menu = h.node_of(derived).expect("context menu body node");
    assert_eq!(menu.layer, Layer::Menu);
    let tree = h.ui.tree(Layer::Menu);
    let index = menu.node.idx();
    assert_eq!(tree.records.layout()[index].padding, Spacing::ZERO);
    assert_eq!(tree.records.layout()[index].margin, Spacing::all(5.0));
    assert_eq!(tree.bounds(menu.node).min_size, Size::ZERO);

    let explicit = WidgetId::from_hash("my-own-menu-body");
    let mut h = UiHarness::new(SURFACE);
    ContextMenu::open(&mut h.ui, trigger_id(), Vec2::new(60.0, 60.0));
    h.frame(|ui| {
        ContextMenu::for_id(trigger_id())
            .id(explicit)
            .show(ui, |_, _| {});
    });
    assert_eq!(
        h.node_of(explicit).map(|at| at.layer),
        Some(Layer::Menu),
        "an explicit id must reach the recorded menu body",
    );
    assert_eq!(
        h.node_of(derived),
        None,
        "the trigger-derived id must not also be recorded",
    );
}

/// Every widget constructor is `#[track_caller]`, the separator included: two separators on two lines take two call-site ids.
#[test]
fn separators_take_their_call_site_ids() {
    use crate::widgets::panel::Panel;

    let mut h = UiHarness::new(SURFACE);
    let ids = h.frame_value(|ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("seps"))
            .show(ui, |ui| {
                let first = MenuSeparator::new().show(ui).id;
                let second = MenuSeparator::new().show(ui).id;
                (first, second)
            })
            .inner
    });
    assert_ne!(ids.0, ids.1);
    assert_ne!(ids.1, ids.0.with(1), "not an occurrence of the first");
}
