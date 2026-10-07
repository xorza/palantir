//! Side-layer widgets: popups (above the main tree, escaping clip), tooltips (delay with warmup) and context menus (secondary-click).

use std::time::Duration;

use crate::support;
use crate::support::{api, note, note_style, raised_bg, readout, row, section, tiles, well_bg};
use palantir::{
    Align, Anchor, AnchorAlign, Block, Button, Checkbox, ClickOutside, Configure, ContextMenu,
    ContextMenuTheme, Justify, Key, MenuItem, MenuSeparator, Panel, Popup, PopupTrigger,
    RadioButton, ResponseSnapshot, Sense, Shortcut, Sizing, Spacing, Text, Tooltip, Ui, WidgetId,
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Side {
    Below,
    Above,
    LeftOf,
    RightOf,
}

#[derive(Debug)]
struct State {
    choice: Option<&'static str>,
    side: Side,
    align: AnchorAlign,
    anchored: bool,
    action: Option<&'static str>,
    /// The roomy menu's theme, built once on the first frame.
    roomy: Option<ContextMenuTheme>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            choice: None,
            side: Side::Below,
            align: AnchorAlign::Start,
            anchored: true,
            action: None,
            roomy: None,
        }
    }
}

pub(crate) fn build(ui: &mut Ui) {
    let state_id = WidgetId::from_hash("showcase::overlays::state");
    ui.with_state::<State, _>(state_id, |ui, s| {
        popup_section(ui, s);
        anchor_section(ui, s);
        tooltip_section(ui);
        context_menu_section(ui, s);
    });
}

fn popup_section(ui: &mut Ui, s: &mut State) {
    section(
        ui,
        "Popup",
        &[api!(PopupTrigger::on), api!(PopupTrigger::is_open)],
        |ui| {
            note(
                ui,
                "A panel dropped below its trigger on the Popup layer, above and outside the \
                 main tree. A second click, an outside press or Escape closes it.",
            );
            row(ui, |ui| {
                let trigger = Button::new().label("Edit ▾").show(ui).snapshot();
                PopupTrigger::on(&trigger)
                    .padding(6.0)
                    .gap(4.0)
                    .min_size((180.0, 0.0))
                    .background(raised_bg())
                    .show(ui, |ui, close| {
                        for label in ["Copy", "Paste", "Delete"] {
                            if Button::new()
                                .id_salt(label)
                                .label(label)
                                .size((Sizing::FILL, Sizing::HUG))
                                .show(ui)
                                .clicked()
                            {
                                s.choice = Some(label);
                                close.close();
                            }
                        }
                    });
                readout(ui, "picked", s.choice.unwrap_or("—"));
            });
        },
    );
}

fn anchor_section(ui: &mut Ui, s: &mut State) {
    section(
        ui,
        "Anchors",
        &[
            api!(Anchor::below),
            api!(Anchor::with_align),
            api!(ClickOutside::PassThrough),
        ],
        |ui| {
            note(
                ui,
                "Where a popup sits against a rect: the side, and how it lines up along that \
                 side. This one lets outside clicks pass through, so it stays open while you \
                 change the controls.",
            );
            row(ui, |ui| {
                for (value, label) in [
                    (Side::Below, "below"),
                    (Side::Above, "above"),
                    (Side::LeftOf, "left_of"),
                    (Side::RightOf, "right_of"),
                ] {
                    RadioButton::new(&mut s.side, value)
                        .id_salt(label)
                        .label(label)
                        .show(ui);
                }
            });
            row(ui, |ui| {
                for (value, label) in [
                    (AnchorAlign::Start, "Start"),
                    (AnchorAlign::Center, "Center"),
                    (AnchorAlign::End, "End"),
                ] {
                    RadioButton::new(&mut s.align, value)
                        .id_salt(label)
                        .label(label)
                        .show(ui);
                }
                Checkbox::new(&mut s.anchored).label("show").show(ui);
            });
            Panel::hstack()
                .size((Sizing::FILL, Sizing::fixed(150.0)))
                .padding(8.0)
                .justify(Justify::Center)
                .child_align(Align::CENTER)
                .background(well_bg())
                .show(ui, |ui| {
                    let target = Block::new()
                        .size((Sizing::fixed(140.0), Sizing::fixed(48.0)))
                        .background(support::swatch_bg(support::A))
                        .show(ui);
                    if s.anchored
                        && let Some(rect) = target.rect
                    {
                        let anchor = match s.side {
                            Side::Below => Anchor::below(rect),
                            Side::Above => Anchor::above(rect),
                            Side::LeftOf => Anchor::left_of(rect),
                            Side::RightOf => Anchor::right_of(rect),
                        };
                        Popup::new(anchor.with_align(s.align).with_gap(6.0))
                            .click_outside(ClickOutside::PassThrough)
                            .padding((10.0, 6.0))
                            .background(raised_bg())
                            .show(ui, |ui, _| {
                                Text::new("popup").style(&note_style()).show(ui);
                            });
                    }
                });
        },
    );
}

fn tooltip_section(ui: &mut Ui) {
    section(
        ui,
        "Tooltips",
        &[
            api!(Tooltip::on as fn(&'static ResponseSnapshot, &'static str) -> Tooltip<'static>),
            api!(Tooltip::delay),
            api!(Tooltip::when_disabled),
        ],
        |ui| {
            note(
                ui,
                "Hover for half a second. The bubble wraps at 280 px unless told otherwise, and \
                 a disabled widget shows none unless it opts in. Once one shows, the next \
                 within a second skips its delay — sweep along the lower row.",
            );
            row(ui, |ui| {
                let r = Button::new().label("default").show(ui).snapshot();
                Tooltip::on(&r, "The default 0.5 s delay before this appears.").show(ui);

                let r = Button::new().label("instant").show(ui).snapshot();
                Tooltip::on(&r, "No delay — shows on the frame the pointer arrives.")
                    .delay(Duration::ZERO)
                    .show(ui);

                let r = Button::new().label("slow").show(ui).snapshot();
                Tooltip::on(&r, "Held for 1.5 s before showing.")
                    .delay(Duration::from_millis(1_500))
                    .show(ui);

                let r = Button::new().label("long text").show(ui).snapshot();
                Tooltip::on(
                    &r,
                    "Tooltips wrap to the configured max width — 280 logical pixels by \
                     default. A long body stacks into several lines, and the bubble's height \
                     hugs the shaped text.",
                )
                .show(ui);

                let r = Button::new().label("narrow").show(ui).snapshot();
                Tooltip::on(&r, "A max width on one tooltip forces a tighter wrap.")
                    .max_size((140.0, f32::INFINITY))
                    .show(ui);

                let r = Button::new()
                    .label("disabled")
                    .disabled(true)
                    .show(ui)
                    .snapshot();
                Tooltip::on(&r, "Never shown: a disabled trigger skips its tooltip.").show(ui);

                let r = Button::new()
                    .label("disabled, explains")
                    .disabled(true)
                    .show(ui)
                    .snapshot();
                Tooltip::on(&r, "when_disabled(true) is for 'why is this off' hints.")
                    .when_disabled(true)
                    .show(ui);
            });
            row(ui, |ui| {
                for (label, tip) in [
                    ("one", "Hover, then move to the next item within a second."),
                    ("two", "This one showed at once — the warmup window."),
                    ("three", "Scanning a row stays quick."),
                    ("four", "Pause for a second and the next one waits again."),
                    ("five", "Last one."),
                ] {
                    let r = Button::new()
                        .id_salt(label)
                        .label(label)
                        .show(ui)
                        .snapshot();
                    Tooltip::on(&r, tip).show(ui);
                }
            });
        },
    );
}

fn context_menu_section(ui: &mut Ui, s: &mut State) {
    section(
        ui,
        "Context menu",
        &[api!(ContextMenu::on), api!(MenuItem::shortcut)],
        |ui| {
            note(
                ui,
                "Right-click the button or either surface. An item click, an outside click or \
                 Escape closes the menu, and ↑ ↓ walk its items. The right surface's menu \
                 has a looser theme.",
            );
            row(ui, |ui| {
                let trigger = Button::new().label("right-click me").show(ui).snapshot();
                attach_menu(ui, &trigger, &mut s.action, None);
                readout(ui, "last action", s.action.unwrap_or("—"));
            });
            let roomy = s.roomy.get_or_insert_with(|| roomy_menu_theme(ui));
            tiles(ui, |ui| {
                for (label, roomy) in [("default theme", None), ("roomy theme", Some(&*roomy))] {
                    let surface = Panel::zstack()
                        .id_salt(label)
                        .size((Sizing::fixed(260.0), Sizing::fixed(90.0)))
                        .child_align(Align::CENTER)
                        .sense(Sense::CLICK)
                        .background(raised_bg())
                        .show(ui, |ui| {
                            Text::new(label).style(&note_style()).show(ui);
                        })
                        .response
                        .snapshot();
                    attach_menu(ui, &surface, &mut s.action, roomy);
                }
            });
        },
    );
}

/// The live context menu theme with looser spacing.
fn roomy_menu_theme(ui: &Ui) -> ContextMenuTheme {
    let mut t = ui.theme().context_menu.clone();
    t.padding = Spacing::all(10.0);
    t.gap = 4.0;
    t.item.defaults.padding = Spacing::xy(12.0, 8.0);
    t.item.gap = 32.0;
    t.separator.thickness = 2.0;
    t.separator.margin = Spacing::xy(0.0, 8.0);
    t
}

/// `style` restyles through the theme bundle every menu widget reads; every `style` setter takes an `Option`, so "styled or default" is a threaded value, not a branch.
fn attach_menu(
    ui: &mut Ui,
    trigger: &ResponseSnapshot,
    action: &mut Option<&'static str>,
    style: Option<&ContextMenuTheme>,
) {
    let mut menu = ContextMenu::on(trigger)
        .size((Sizing::HUG, Sizing::HUG))
        .style(style);
    if style.is_some() {
        menu = menu.min_size((260.0, 0.0)).max_size((320.0, 280.0));
    }
    menu.show(ui, |ui, popup| {
        let item = style.map(|s| &s.item);
        let rule = style.map(|s| &s.separator);
        for (label, shortcut) in [
            ("Copy", Shortcut::ctrl('C')),
            ("Cut", Shortcut::ctrl('X')),
            ("Paste", Shortcut::ctrl('V')),
        ] {
            if MenuItem::new(label)
                .id_salt(label)
                .shortcut(shortcut)
                .style(item)
                .show(ui, popup)
                .clicked()
            {
                *action = Some(label);
            }
        }
        MenuSeparator::new().style(rule).show(ui);
        MenuItem::new("Disabled")
            .disabled(true)
            .style(item)
            .show(ui, popup);
        MenuSeparator::new().style(rule).show(ui);
        if MenuItem::new("Delete")
            .shortcut(Shortcut::key(Key::Backspace))
            .style(item)
            .show(ui, popup)
            .clicked()
        {
            *action = Some("Delete");
        }
    });
}
