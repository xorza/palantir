//! End-to-end tests for `ContextMenu` + `MenuItem`.

use crate::input::keyboard::{Key, Modifiers};
use crate::input::shortcut::Shortcut;
use crate::layout::types::sizing::Sizing;
use crate::primitives::background::Background;
use crate::primitives::color::{Color, ColorF16};
use crate::primitives::corners::Corners;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::primitives::spacing::Spacing;
use crate::primitives::widget_id::WidgetId;
use crate::scene::layer::Layer;
use crate::scene::node::Configure;
use crate::scene::shapes::paint::ShapeBrush;
use crate::scene::tree::record::NodeId;
use crate::ui::harness::UiHarness;
use crate::widgets::button::Button;
use crate::widgets::context_menu::{ContextMenu, ContextMenuState, MenuItem};
use crate::widgets::panel::Panel;
use crate::widgets::theme::context_menu::ContextMenuTheme;
use crate::widgets::theme::context_menu::menu_item::MenuItemTheme;
use crate::widgets::theme::separator::SeparatorTheme;
use crate::{Sense, Ui};
use glam::{UVec2, Vec2};

const SURFACE: UVec2 = UVec2::new(400, 400);

fn trigger_id() -> WidgetId {
    WidgetId::from_hash("trigger")
}

/// Layout arithmetic lands within f32 slop of the hand-computed value —
/// row extents fold in text measurement, and the gap/margin knobs
/// round-trip through the node columns' f16 lanes.
#[track_caller]
fn assert_close(actual: f32, expected: f32, what: &str) {
    assert!(
        (actual - expected).abs() < 1e-3,
        "{what}: expected {expected}, got {actual}",
    );
}

fn menu_body(h: &UiHarness, for_id: WidgetId) -> NodeId {
    let body_id = for_id.with("body");
    let index = h.ui.forest.trees[Layer::Popup]
        .records
        .widget_id()
        .iter()
        .position(|id| *id == body_id)
        .expect("context menu body recorded");
    NodeId(index as u32)
}

#[derive(Clone, Copy, Debug)]
struct MenuRow {
    node: NodeId,
    rect: Rect,
}

/// The open menu's direct children in record order (separators
/// included), each with the rect arrange gave it. Walks `subtree_end`
/// so a row's own label / shortcut leaves are skipped.
fn menu_rows(h: &UiHarness, for_id: WidgetId) -> Vec<MenuRow> {
    let body = menu_body(h, for_id).idx();
    let tree = &h.ui.forest.trees[Layer::Popup];
    let ends = tree.records.subtree_end();
    let body_end = ends[body].end() as usize;
    let rects = &h.ui.layout[Layer::Popup].rect;
    let mut rows = Vec::new();
    let mut i = body + 1;
    while i < body_end {
        rows.push(MenuRow {
            node: NodeId(i as u32),
            rect: rects[i],
        });
        i = ends[i].end() as usize;
    }
    rows
}

fn build(ui: &mut Ui, clicked_copy: &mut bool, _unused: &mut bool) {
    Panel::vstack()
        .id(WidgetId::from_hash("root"))
        .size((Sizing::FILL, Sizing::FILL))
        .sense(Sense::CLICK)
        .show(ui, |ui| {
            let trigger = Button::new()
                .id(WidgetId::from_hash("trigger"))
                .label("right click me")
                .size((Sizing::fixed(120.0), Sizing::fixed(40.0)))
                .show(ui)
                .snapshot();
            ContextMenu::attach(ui, &trigger).show(ui, |ui, popup| {
                if MenuItem::new("Copy")
                    .shortcut(Shortcut::ctrl('C'))
                    .show(ui, popup)
                    .left
                    .clicked()
                {
                    *clicked_copy = true;
                }
                MenuItem::separator().show(ui);
                MenuItem::new("Paste").show(ui, popup);
            });
        });
}

fn menu_open(ui: &Ui) -> bool {
    ContextMenu::is_open(ui, trigger_id())
}

#[test]
fn close_before_open_does_not_create_state() {
    let mut h = UiHarness::arena();
    ContextMenu::close(h.ui(), trigger_id());
    assert!(h.ui.try_state::<ContextMenuState>(trigger_id()).is_none());
}

#[test]
fn secondary_click_opens_menu_at_pointer() {
    let mut h = UiHarness::new(SURFACE);
    let mut copied = false;
    let mut dismissed = false;
    h.frame(|ui| build(ui, &mut copied, &mut dismissed));
    assert!(!menu_open(&h.ui), "menu starts closed");

    h.right_click_at(Vec2::new(60.0, 20.0));
    let mut copied = false;
    let mut dismissed = false;
    h.frame(|ui| build(ui, &mut copied, &mut dismissed));
    assert!(menu_open(&h.ui), "secondary click on trigger opens menu");
}

#[test]
fn outside_click_dismisses_menu() {
    let mut h = UiHarness::new(SURFACE);
    let mut copied = false;
    let mut dismissed = false;
    h.frame(|ui| build(ui, &mut copied, &mut dismissed));
    h.right_click_at(Vec2::new(60.0, 20.0));
    let mut copied = false;
    let mut dismissed = false;
    h.frame(|ui| build(ui, &mut copied, &mut dismissed));
    assert!(menu_open(&h.ui));

    // Click far from both trigger and any plausible menu body location.
    h.click_at(Vec2::new(380.0, 380.0));
    let mut copied = false;
    let mut dismissed = false;
    h.frame(|ui| build(ui, &mut copied, &mut dismissed));
    assert!(!menu_open(&h.ui), "outside click closes the menu");
}

#[test]
fn item_click_dismisses_and_reports_clicked() {
    let mut h = UiHarness::new(SURFACE);
    let mut copied = false;
    let mut dismissed = false;
    h.frame(|ui| build(ui, &mut copied, &mut dismissed));
    // Open the menu at a known anchor.
    ContextMenu::open(&mut h.ui, trigger_id(), Vec2::new(60.0, 60.0));
    let mut copied = false;
    let mut dismissed = false;
    h.frame(|ui| build(ui, &mut copied, &mut dismissed));
    assert!(menu_open(&h.ui));

    // The menu's container starts at anchor (60, 60). With theme
    // padding (~4) plus row padding, the first item (Copy) sits a
    // few px inside that. Click a couple px past the top-left
    // corner — well inside any plausible row layout.
    h.click_at(Vec2::new(90.0, 80.0));
    let mut copied = false;
    let mut dismissed = false;
    h.frame(|ui| build(ui, &mut copied, &mut dismissed));
    assert!(copied, "clicking the Copy row reports clicked()");
    assert!(!menu_open(&h.ui), "item click auto-closes the menu");
}

/// Pressing a `MenuItem`'s shortcut while the menu is open fires
/// the item (its `Response::clicked` is `true`) AND closes the menu,
/// mirroring native menu behaviour. Disabled items don't intercept.
#[test]
fn shortcut_press_fires_item_and_dismisses() {
    let mut h = UiHarness::new(SURFACE);
    let mut copied = false;
    let mut dismissed = false;
    h.frame(|ui| build(ui, &mut copied, &mut dismissed));
    ContextMenu::open(&mut h.ui, trigger_id(), Vec2::new(60.0, 60.0));
    let mut copied = false;
    let mut dismissed = false;
    h.frame(|ui| build(ui, &mut copied, &mut dismissed));
    assert!(menu_open(&h.ui));

    // Inject the primary command modifier + 'C' — matches
    // `Shortcut::ctrl('C')` on the Copy item. `Modifiers::ctrl` is
    // platform-normalized (Cmd on macOS, Ctrl elsewhere).
    let primary_mods = Modifiers {
        ctrl: true,
        ..Modifiers::NONE
    };
    h.set_modifiers(primary_mods);
    h.key(Key::Char('C'));
    let mut copied = false;
    let mut dismissed = false;
    h.frame(|ui| build(ui, &mut copied, &mut dismissed));
    assert!(copied, "shortcut press synthesizes a click on the Copy row");
    assert!(!menu_open(&h.ui), "shortcut press auto-closes the menu");
}

#[test]
fn escape_dismisses_menu() {
    let mut h = UiHarness::new(SURFACE);
    let mut copied = false;
    let mut dismissed = false;
    h.frame(|ui| build(ui, &mut copied, &mut dismissed));
    ContextMenu::open(&mut h.ui, trigger_id(), Vec2::new(60.0, 60.0));
    let mut copied = false;
    let mut dismissed = false;
    h.frame(|ui| build(ui, &mut copied, &mut dismissed));
    assert!(menu_open(&h.ui));

    // Inject an Escape press.
    h.key(Key::Escape);
    let mut copied = false;
    let mut dismissed = false;
    h.frame(|ui| build(ui, &mut copied, &mut dismissed));
    assert!(!menu_open(&h.ui), "Esc closes the menu");
}

/// Menu body must hug to its content width (theme.min_width floor),
/// not blow up to the surface width. Regresses an issue where `Fill`
/// cross-axis on inner cells leaked `INF` up through the Hug menu
/// container.
#[test]
fn menu_body_width_does_not_span_surface() {
    let mut h = UiHarness::new(SURFACE);
    let mut copied = false;
    let mut dismissed = false;
    h.frame(|ui| build(ui, &mut copied, &mut dismissed));
    ContextMenu::open(&mut h.ui, trigger_id(), Vec2::new(60.0, 60.0));
    let mut copied = false;
    let mut dismissed = false;
    h.frame(|ui| build(ui, &mut copied, &mut dismissed));

    let body_id = trigger_id().with("body");
    let rect =
        h.ui.cascade
            .locate(body_id)
            .map(|l| l.entry_idx)
            .map(|i| h.ui.cascade.entries[i as usize].rect)
            .expect("menu body recorded");
    // Theme min_width is 160; sample labels are short so we expect
    // ≤ 200 px wide. SURFACE.w = 400, so a "spans surface" regression
    // would land ≥ 380.
    assert!(
        rect.size.w < 240.0,
        "menu body w={} — expected hug to content, not surface width ({})",
        rect.size.w,
        SURFACE.x,
    );
}

/// Both menu gutters are theme knobs, not literals baked into the
/// widget: `context_menu.gap` is the row-to-row pitch and
/// `context_menu.item.gap` the floor holding a row's label apart from
/// its shortcut hint. Each moves arranged geometry by exactly its own
/// delta and leaves the other axis alone.
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
    // Hug the content: the theme's 160 px floor would otherwise absorb
    // the widened shortcut gutter instead of letting the row grow.
    h.ui.theme.context_menu.min_width = 0.0;
    ContextMenu::open(&mut h.ui, trigger_id(), Vec2::new(20.0, 20.0));
    h.frame(menu);
    let before = menu_rows(&h, trigger_id());
    assert_eq!(before.len(), 2, "two rows recorded");

    h.ui.theme.context_menu.gap += 6.0;
    h.ui.theme.context_menu.item.gap += 10.0;
    h.frame(menu);
    let after = menu_rows(&h, trigger_id());

    // Only the row carrying a shortcut has a gutter to widen, but rows
    // stretch to the body's content width, so both follow it.
    for (i, (a, b)) in after.iter().zip(&before).enumerate() {
        assert_close(
            a.rect.size.w - b.rect.size.w,
            10.0,
            &format!("row {i} width"),
        );
        assert_close(a.rect.size.h, b.rect.size.h, &format!("row {i} height"));
    }
    let pitch_before = before[1].rect.min.y - before[0].rect.min.y;
    let pitch_after = after[1].rect.min.y - after[0].rect.min.y;
    assert_close(
        pitch_after - pitch_before,
        6.0,
        "row pitch grows by the menu gap delta",
    );
}

/// An explicit `.gap(0.0)` is not the same as never setting one — the
/// theme fills in only the untouched case.
///
/// This is what `Gaps`'s unset state buys. `Configure::gap` writes into
/// a packed f16 pair, and while its zero was indistinguishable from
/// "untouched" the theme fallback had nothing to key on, so `ContextMenu`
/// carried its own `Option<f32>` shadowing the setter. Rows sit flush at
/// `0.0` and a theme gap apart when unset.
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
    h.ui.theme.context_menu.gap = 7.0;
    ContextMenu::open(&mut h.ui, trigger_id(), Vec2::new(20.0, 20.0));

    // Untouched: the theme's 7 px lands between the rows.
    let unset = rows(&mut h, None);
    assert_eq!(unset.len(), 2);
    let unset_pitch = unset[1].rect.min.y - unset[0].rect.min.y;
    assert_close(unset_pitch, unset[0].rect.size.h + 7.0, "themed pitch");

    // Explicit zero: rows sit flush, theme gap ignored.
    let zeroed = rows(&mut h, Some(0.0));
    let zero_pitch = zeroed[1].rect.min.y - zeroed[0].rect.min.y;
    assert_close(zero_pitch, zeroed[0].rect.size.h, "explicit 0.0 pitch");
    assert_ne!(unset_pitch, zero_pitch);

    // And a non-zero explicit value still wins over the theme, so the
    // fallback keys on "set at all", not on "non-zero".
    let wide = rows(&mut h, Some(20.0));
    let wide_pitch = wide[1].rect.min.y - wide[0].rect.min.y;
    assert_close(
        wide_pitch,
        wide[0].rect.size.h + 20.0,
        "explicit 20.0 pitch",
    );
}

/// `MenuSeparator` wears `context_menu.separator`, never the app-wide
/// `theme.separator`: thickness is the rule's arranged height, margin
/// the space it holds off the rows on either side, and color reaches
/// the recorded chrome.
#[test]
fn menu_separator_theme_drives_rule_geometry_and_color() {
    fn menu(ui: &mut Ui) {
        ContextMenu::for_id(trigger_id()).show(ui, |ui, popup| {
            MenuItem::new("Copy").show(ui, popup);
            MenuItem::separator().show(ui);
            MenuItem::new("Paste").show(ui, popup);
        });
    }

    let mut h = UiHarness::new(SURFACE);
    let rule = Color::hex(0xff00ff);
    h.ui.theme.context_menu.separator = SeparatorTheme {
        color: rule,
        thickness: 3.0,
        margin: Spacing::xy(0.0, 7.0),
    };
    // Loudly different app-wide rule — the menu must not reach for it.
    h.ui.theme.separator.thickness = 11.0;
    h.ui.theme.separator.color = Color::hex(0x00ff00);

    ContextMenu::open(&mut h.ui, trigger_id(), Vec2::new(20.0, 20.0));
    h.frame(menu);
    let rows = menu_rows(&h, trigger_id());
    assert_eq!(rows.len(), 3, "two rows plus the separator between them");
    let [first, sep, second] = [rows[0], rows[1], rows[2]];

    assert_close(sep.rect.size.h, 3.0, "thickness is the rule's height");
    assert_close(
        sep.rect.min.y - first.rect.max().y,
        7.0,
        "margin.top clears the row above",
    );
    assert_close(
        second.rect.min.y - sep.rect.max().y,
        7.0,
        "margin.bottom clears the row below",
    );

    let chrome = h.ui.forest.trees[Layer::Popup]
        .chrome(sep.node)
        .expect("separator chrome");
    let ShapeBrush::Solid(fill) = chrome.fill else {
        panic!("the menu rule paints a solid fill");
    };
    assert_eq!(fill, ColorF16::from(rule), "rule color comes off the menu");
}

/// Both menu radii are theme fields, and `with_radius` keeps them in
/// the relationship the default ships: the row chip nests *inside* the
/// panel corner rather than out-rounding it. A `None` chip derives one
/// px under the panel; `Some` overrides it outright. `normal` paints no
/// background at rest and must stay that way.
#[test]
fn with_radius_rerounds_panel_and_nests_the_row_chip() {
    let derived = ContextMenuTheme::default().with_radius(10.0, None);
    assert_eq!(derived.panel.corners, Corners::all(10.0), "panel radius");
    assert_eq!(
        derived
            .item
            .looks
            .hovered
            .background
            .as_ref()
            .unwrap()
            .corners,
        Corners::all(9.0),
        "chip derives one px under the panel",
    );
    assert_eq!(
        derived
            .item
            .looks
            .active
            .background
            .as_ref()
            .unwrap()
            .corners,
        Corners::all(9.0),
        "every state that paints a chip follows",
    );
    assert!(
        derived.item.looks.normal.background.is_none(),
        "rows stay transparent at rest — with_radius must not materialize a chip",
    );

    let explicit = ContextMenuTheme::default().with_radius(10.0, Some(2.0));
    assert_eq!(explicit.panel.corners, Corners::all(10.0));
    assert_eq!(
        explicit
            .item
            .looks
            .hovered
            .background
            .as_ref()
            .unwrap()
            .corners,
        Corners::all(2.0),
        "an explicit chip radius wins over the derived one",
    );

    // A square panel can't take a negative chip.
    let square = ContextMenuTheme::default().with_radius(0.0, None);
    assert_eq!(square.panel.corners, Corners::all(0.0));
    assert_eq!(
        square
            .item
            .looks
            .hovered
            .background
            .as_ref()
            .unwrap()
            .corners,
        Corners::all(0.0),
        "derived chip floors at 0 rather than going negative",
    );
}

/// `.style(...)` beats the global slot on every menu widget and writes
/// nothing back to it. The panel takes the whole bundle; the rows and
/// the rule — recorded by the caller's closure, not by `ContextMenu` —
/// take their own halves.
///
/// Rows resolve their box through the shared `resolve_look`, so both
/// halves of that contract hold: the theme's `padding` / `margin` fill
/// in where the builder was silent, and an explicit value wins. The
/// second was not true while `MenuItem` stamped `node.padding`
/// unconditionally — a caller's `.padding(...)` vanished.
#[test]
fn per_instance_style_overrides_global_menu_theme() {
    let custom = ContextMenuTheme {
        padding: Spacing::all(13.0),
        min_width: 220.0,
        item: MenuItemTheme {
            padding: Spacing::all(9.0),
            // Asymmetric, and distinct from the padding, so a
            // padding/margin mix-up can't read as a pass.
            margin: Spacing::xy(2.0, 6.0),
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
                MenuItem::separator().style(&custom.separator).show(ui);
                MenuItem::new("Bare")
                    .style(&custom.item)
                    .padding(Spacing::ZERO)
                    .margin(Spacing::ZERO)
                    .show(ui, popup);
            });
    });

    let body = menu_body(&h, trigger_id());
    let rows = menu_rows(&h, trigger_id());
    let tree = &h.ui.forest.trees[Layer::Popup];
    let layout = tree.records.layout();
    // The recorded padding is the styled 13 plus the panel's 1 px
    // stroke, which `Tree` folds in so content clears the stroke band.
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
    assert_close(rows[1].rect.size.h, 5.0, "rule thickness");
    // Same styled bundle, but this row set both itself.
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

    // Nothing about `.style` writes back to the global slot.
    let default = ContextMenuTheme::default();
    let global = &h.ui.theme.context_menu;
    assert_eq!(global.padding, default.padding);
    assert_eq!(global.min_width, default.min_width);
    assert_eq!(global.item.padding, default.item.padding);
    assert_eq!(global.separator.thickness, default.separator.thickness);
}

/// Record index of the popup-layer node carrying `id`, if any.
fn popup_node(h: &UiHarness, id: WidgetId) -> Option<usize> {
    h.ui.forest.trees[Layer::Popup]
        .records
        .widget_id()
        .iter()
        .position(|recorded| *recorded == id)
}

/// The menu body takes its box from [`Configure`] — `ContextMenu` used
/// to hand-roll `size` / `min_size` / `max_size` / `padding` and offer
/// nothing else, so `margin` here is a setter it simply did not have.
///
/// Identity is the other half: the body id derives from the trigger,
/// because a menu has no call site of its own worth keying on — but an
/// explicit `.id(...)` has to win, the same way explicit spacing wins
/// over the theme.
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
    let index = popup_node(&h, derived).expect("context menu body node");
    let tree = &h.ui.forest.trees[Layer::Popup];
    assert_eq!(tree.records.layout()[index].padding, Spacing::ZERO);
    assert_eq!(tree.records.layout()[index].margin, Spacing::all(5.0));
    assert_eq!(tree.bounds(NodeId(index as u32)).min_size, Size::ZERO);

    // Same trigger, caller-set id: the derived one must not appear.
    let explicit = WidgetId::from_hash("my-own-menu-body");
    let mut h = UiHarness::new(SURFACE);
    ContextMenu::open(&mut h.ui, trigger_id(), Vec2::new(60.0, 60.0));
    h.frame(|ui| {
        ContextMenu::for_id(trigger_id())
            .id(explicit)
            .show(ui, |_, _| {});
    });
    assert!(
        popup_node(&h, explicit).is_some(),
        "an explicit id must reach the recorded menu body",
    );
    assert!(
        popup_node(&h, derived).is_none(),
        "the trigger-derived id must not also be recorded",
    );
}
