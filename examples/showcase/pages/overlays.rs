//! Side-layer widgets: popups (above the main tree, escaping clip), a modal dialog, tooltips (delay with warmup), context menus (secondary-click) and a menu bar.

use std::time::Duration;

use crate::appearance::Appearance;
use crate::scaffold::api;
use crate::support;
use crate::support::{Dialog, Kit, Section};
use palantir::{
    Align, Anchor, AnchorAlign, Axis, Block, Button, Checkbox, ClickOutside, CloseHandle,
    Configure, Justify, Key, Menu, MenuBar, MenuItem, MenuSeparator, MenuTheme, Metrics, Modal,
    Panel, Popup, PopupTrigger, RadioButton, ResponseSnapshot, Sense, Shortcut, Sizing, Slider,
    Spacing, Text, Theme, Tooltip, Ui, WidgetId, fmt,
};

const M: Metrics = Metrics::DEFAULT;

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
    asking: bool,
    answer: Option<&'static str>,
    /// The tooltip knobs: its delay in milliseconds and the width it wraps at.
    tip_delay_ms: f64,
    tip_width: f32,
    action: Option<&'static str>,
    /// The roomy menu's theme, built once on the first frame.
    roomy: Option<MenuTheme>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            choice: None,
            side: Side::Below,
            align: AnchorAlign::Start,
            anchored: true,
            asking: false,
            answer: None,
            tip_delay_ms: 500.0,
            tip_width: 280.0,
            action: None,
            roomy: None,
        }
    }
}

pub(crate) fn build(ui: &mut Ui, k: Kit, look: &mut Appearance) {
    let state_id = WidgetId::from_hash("showcase::overlays::state");
    ui.with_state::<State, _>(state_id, |ui, s| {
        popup_section(ui, k, s);
        material_section(ui, k);
        anchor_section(ui, k, s);
        modal_section(ui, k, s, look);
        tooltip_section(ui, k, s);
        context_menu_section(ui, k, s);
        menu_bar_section(ui, k, s);
    });
}

fn material_section(ui: &mut Ui, k: Kit) {
    Section::new("Frosted glass")
        .apis(&[
            api!(Ui::set_transparency_preference),
            api!(Theme::frost_floats),
        ])
        .show(ui, k, |ui| {
            support::note(
                ui,
                k,
                "Menus, combo lists, popups, the picker, tooltips and the modal float on frosted \
                 glass, its blur held so their text keeps 4.5:1 over any page. Reduce \
                 transparency, as the system's setting or a pick here asks, and every pane paints \
                 its solid fill with no blur; every page follows.",
            );
            support::well(ui, k, |ui| {
                let mut transparency = ui.transparency_preference();
                support::reduce_picker(ui, "transparency", &mut transparency);
                if transparency != ui.transparency_preference() {
                    ui.set_transparency_preference(transparency);
                }
            });
        });
}

fn popup_section(ui: &mut Ui, k: Kit, s: &mut State) {
    Section::new("Popup")
        .apis(&[api!(PopupTrigger::on), api!(PopupTrigger::is_open)])
        .show(ui, k, |ui| {
            support::note(
                ui,
                k,
                "A panel dropped below its trigger on the Popup layer, above and outside the \
                 main tree. A second click, an outside press or Escape closes it.",
            );
            support::well(ui, k, |ui| {
                support::row(ui, |ui| {
                    let trigger = Button::new().label("Edit ▾").show(ui).snapshot();
                    let menu = &ui.theme().menu;
                    let (padding, gap, width) = (menu.padding, menu.gap, menu.min_width);
                    PopupTrigger::on(&trigger)
                        .padding(padding)
                        .gap(gap)
                        .min_size((width, 0.0))
                        .arrow_focus(Axis::Y)
                        .show(ui, |ui, _| {
                            for label in ["Copy", "Paste", "Delete"] {
                                if MenuItem::new(label).id_salt(label).show(ui).clicked() {
                                    s.choice = Some(label);
                                }
                            }
                        });
                    support::readout(ui, k, "picked", s.choice.unwrap_or("—"));
                });
            });
        });
}

fn anchor_section(ui: &mut Ui, k: Kit, s: &mut State) {
    Section::new("Anchors")
        .apis(&[
            api!(Anchor::below),
            api!(Anchor::with_align),
            api!(ClickOutside::PassThrough),
        ])
        .show(ui, k, |ui| {
            support::note(
                ui,
                k,
                "Where a popup sits against a rect: the side, and how it lines up along that \
                 side. This one lets outside clicks pass through, so it stays open while you \
                 change the controls.",
            );
            support::row(ui, |ui| {
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
            support::row(ui, |ui| {
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
                .padding(M.space.sm)
                .justify(Justify::Center)
                .child_align(Align::CENTER)
                .chrome(k.t.card())
                .show(ui, |ui| {
                    let target = Block::new()
                        .size((Sizing::fixed(140.0), Sizing::fixed(48.0)))
                        .chrome(support::swatch_chrome(k.swatch.teal))
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
                        Popup::new()
                            .anchor(anchor.with_align(s.align).with_gap(6.0))
                            .click_outside(ClickOutside::PassThrough)
                            .padding((10.0, 6.0))
                            .show(ui, |ui, _| {
                                Text::new("popup").style(&k.t.caption()).show(ui);
                            });
                    }
                });
        });
}

fn modal_section(ui: &mut Ui, k: Kit, s: &mut State, look: &Appearance) {
    Section::new("Modal")
        .apis(&[api!(Modal::new), api!(CloseHandle::close)])
        .show(ui, k, |ui| {
            support::note(
                ui,
                k,
                "A modal dims the window and takes every pointer. Escape or a click on the \
                 backdrop closes it, and Tab stays inside it — see the focus & keyboard page \
                 for the focus side.",
            );
            support::well(ui, k, |ui| {
                support::row(ui, |ui| {
                    if Button::new().label("Delete all…").show(ui).clicked() {
                        s.asking = true;
                    }
                    support::readout(ui, k, "last answer", s.answer.unwrap_or("—"));
                });
            });
        });
    if !s.asking {
        return;
    }
    let accent = look.accent_button(ui.color_scheme());
    let response = Dialog {
        title: "Delete all the things?",
        body: "Every thing goes, and nothing brings it back.",
    }
    .show(ui, k, |ui, close| {
        for (label, style) in [("Cancel", None), ("Delete", Some(&accent))] {
            if Button::new()
                .id_salt(label)
                .label(label)
                .style(style)
                .show(ui)
                .clicked()
            {
                s.answer = Some(label);
                close.close();
            }
        }
    });
    if response.closed() {
        s.asking = false;
    }
}

fn tooltip_section(ui: &mut Ui, k: Kit, s: &mut State) {
    Section::new("Tooltips")
        .apis(&[
            api!(Tooltip::on as fn(&'static ResponseSnapshot, &'static str) -> Tooltip<'static>),
            api!(Tooltip::delay),
            api!(Tooltip::when_disabled),
        ])
        .show(ui, k, |ui| {
            support::note(
                ui,
                k,
                "Set the delay and the wrap width, then hover. A disabled widget shows no \
                 tooltip unless it opts in. Once one shows, the next within a second skips its \
                 delay — sweep along the lower row.",
            );
            support::well(ui, k, |ui| {
                support::row(ui, |ui| {
                    let r = Button::new().label("hover me").show(ui).snapshot();
                    Tooltip::on(
                        &r,
                        "The delay and the width to wrap at come from the sliders beside this \
                         button; a long body stacks into lines, and the bubble hugs the text.",
                    )
                    .delay(Duration::from_secs_f64(s.tip_delay_ms / 1000.0))
                    .max_size((s.tip_width, f32::INFINITY))
                    .show(ui);
                    Text::new("delay").style(&k.t.caption()).show(ui);
                    Slider::new(&mut s.tip_delay_ms, 0.0..=1500.0)
                        .step(100.0)
                        .size((Sizing::fixed(120.0), Sizing::HUG))
                        .show(ui);
                    let delay = fmt!(ui, "{:.0} ms", s.tip_delay_ms);
                    Text::new(delay).style(&k.t.mono(k.t.ink)).show(ui);
                    Text::new("width").style(&k.t.caption()).show(ui);
                    Slider::new(&mut s.tip_width, 120.0..=400.0)
                        .step(20.0)
                        .size((Sizing::fixed(120.0), Sizing::HUG))
                        .show(ui);
                    let width = fmt!(ui, "{:.0} px", s.tip_width);
                    Text::new(width).style(&k.t.mono(k.t.ink)).show(ui);
                });
                support::row(ui, |ui| {
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
                support::row(ui, |ui| {
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
            });
        });
}

fn context_menu_section(ui: &mut Ui, k: Kit, s: &mut State) {
    Section::new("Context menu")
        .apis(&[api!(Menu::context), api!(MenuItem::shortcut)])
        .show(ui, k, |ui| {
            support::note(
                ui,
                k,
                "Right-click the button or either surface. An item click, an outside click or \
                 Escape closes the menu, and ↑ ↓ walk its items. The right surface's menu \
                 has a looser theme.",
            );
            support::row(ui, |ui| {
                let trigger = Button::new().label("right-click me").show(ui).snapshot();
                DemoMenu {
                    trigger: &trigger,
                    style: None,
                }
                .show(ui, &mut s.action);
                support::readout(ui, k, "last action", s.action.unwrap_or("—"));
            });
            let roomy = s.roomy.get_or_insert_with(|| roomy_menu_theme(ui));
            support::tiles(ui, |ui| {
                for (label, roomy) in [("default theme", None), ("roomy theme", Some(&*roomy))] {
                    let surface = Panel::zstack()
                        .id_salt(label)
                        .size((Sizing::fixed(260.0), Sizing::fixed(90.0)))
                        .child_align(Align::CENTER)
                        .sense(Sense::CLICK)
                        .chrome(support::raised_chrome(k))
                        .show(ui, |ui| {
                            Text::new(label).style(&k.t.caption()).show(ui);
                        })
                        .response
                        .snapshot();
                    DemoMenu {
                        trigger: &surface,
                        style: roomy,
                    }
                    .show(ui, &mut s.action);
                }
            });
        });
}

fn menu_bar_section(ui: &mut Ui, k: Kit, s: &mut State) {
    Section::new("Menu bar")
        .apis(&[api!(MenuBar::new)])
        .show(ui, k, |ui| {
            support::note(
                ui,
                k,
                "Click a title to open its menu. While one is open, moving onto another title \
                 opens that one, and ← → move between the menus.",
            );
            support::well(ui, k, |ui| {
                support::row(ui, |ui| {
                    MenuBar::new().show(ui, |bar| {
                        for (title, rows) in [
                            ("File", ["New", "Open…", "Save"]),
                            ("Edit", ["Undo", "Redo", "Copy"]),
                            ("View", ["Zoom In", "Zoom Out", "Fit"]),
                        ] {
                            bar.menu(title, |ui, _| {
                                for row in rows {
                                    if MenuItem::new(row).id_salt(row).show(ui).clicked() {
                                        s.action = Some(row);
                                    }
                                }
                            });
                        }
                    });
                    support::readout(ui, k, "last action", s.action.unwrap_or("—"));
                });
            });
        });
}

/// The live context menu theme with looser spacing.
fn roomy_menu_theme(ui: &Ui) -> MenuTheme {
    let mut t = ui.theme().menu.clone();
    t.padding = Spacing::all(10.0);
    t.gap = 4.0;
    t.item.defaults.padding = Spacing::xy(12.0, 8.0);
    t.item.gap = 32.0;
    t.separator.thickness = 2.0;
    t.separator.margin = Spacing::xy(0.0, 8.0);
    t
}

/// The demo's context menu on `trigger`.
#[derive(Debug)]
struct DemoMenu<'a> {
    trigger: &'a ResponseSnapshot,
    /// Restyles through the theme bundle every menu widget reads; every `style` setter takes an `Option`, so "styled or default" is a threaded value, not a branch.
    style: Option<&'a MenuTheme>,
}

impl DemoMenu<'_> {
    /// Records the menu, writing the clicked item's label to `action`.
    fn show(self, ui: &mut Ui, action: &mut Option<&'static str>) {
        let Self { trigger, style } = self;
        let mut menu = Menu::context(trigger)
            .size((Sizing::HUG, Sizing::HUG))
            .style(style);
        if style.is_some() {
            menu = menu.min_size((260.0, 0.0)).max_size((320.0, 280.0));
        }
        menu.show(ui, |ui, _| {
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
                    .show(ui)
                    .clicked()
                {
                    *action = Some(label);
                }
            }
            MenuSeparator::new().style(rule).show(ui);
            MenuItem::new("Disabled")
                .disabled(true)
                .style(item)
                .show(ui);
            MenuSeparator::new().style(rule).show(ui);
            if MenuItem::new("Delete")
                .shortcut(Shortcut::key(Key::Backspace))
                .style(item)
                .show(ui)
                .clicked()
            {
                *action = Some("Delete");
            }
        });
    }
}
