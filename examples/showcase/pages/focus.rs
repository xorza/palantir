//! Keyboard focus end to end; each section ends in a checklist, so the page doubles as the by-hand focus test.

use crate::shell;
use crate::support;
use crate::support::{api, checklist, note, readout, row, section, well};
use palantir::{
    Axis, Button, Checkbox, ColorButton, ColorPicker, ComboBox, Configure, DragValue, FocusPolicy,
    FocusRingTheme, Grid, Modal, Panel, PopupTrigger, RadioButton, RgbaF32, Sizing, Slider, Switch,
    Text, TextEdit, Theme, Track, Ui, WidgetId,
};

const PLANS: [&str; 3] = ["Free", "Pro", "Team"];
const LIST: [&str; 5] = ["Inbox", "Drafts", "Sent", "Archive", "Trash"];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Size {
    Small,
    Medium,
    Large,
}

#[derive(Debug)]
struct State {
    name: String,
    email: String,
    subscribe: bool,
    dark: bool,
    volume: f64,
    size: Size,
    plan: usize,
    seats: i64,
    search: String,
    picked: usize,
    dialog_open: bool,
    dialog_note: String,
    filters: [bool; 3],
    target: String,
    policy: FocusPolicy,
    /// The control that held focus last frame, by name.
    focused: Option<&'static str>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            name: String::new(),
            email: String::new(),
            subscribe: true,
            dark: false,
            volume: 0.4,
            size: Size::Medium,
            plan: 1,
            seats: 3,
            search: String::new(),
            picked: 0,
            dialog_open: false,
            dialog_note: String::new(),
            filters: [true, false, true],
            target: String::from("select me"),
            policy: FocusPolicy::default(),
            focused: None,
        }
    }
}

#[derive(Debug, Default)]
struct Seen(Option<&'static str>);

impl Seen {
    const fn note(&mut self, name: &'static str, focused: bool) {
        if focused {
            self.0 = Some(name);
        }
    }
}

pub(crate) fn build(ui: &mut Ui) {
    let state_id = WidgetId::from_hash("showcase::focus::state");
    ui.with_state::<State, _>(state_id, page);
}

fn page(ui: &mut Ui, s: &mut State) {
    let form_id = WidgetId::from_hash("showcase::focus::form");
    let mut seen = Seen::default();

    tab_order(ui, s, form_id, &mut seen);
    order_and_stops(ui, &mut seen);
    arrow_groups(ui, s, &mut seen);
    containment(ui, s, &mut seen);
    programmatic(ui, s, form_id, &mut seen);
    policy(ui, s);
    ring(ui);

    // Responses read last frame's focus, so a change shows one frame late.
    if seen.0 != s.focused {
        s.focused = seen.0;
        ui.request_repaint();
    }
}

fn tab_order(ui: &mut Ui, s: &mut State, form_id: WidgetId, seen: &mut Seen) {
    section(
        ui,
        "Tab order",
        &[api!(Ui::focus), api!(Ui::is_focus_visible)],
        |ui| {
            note(
                ui,
                "Tab and Shift+Tab visit every enabled control in record order, wrapping \
                 at the ends of the window. The ring shows only when focus came from the \
                 keyboard, as CSS's :focus-visible does.",
            );
            well(ui, |ui| {
                let focus = match (s.focused, ui.focus()) {
                    (Some(name), _) => name,
                    (None, Some(_)) => "a control outside this page's demos",
                    (None, None) => "nothing",
                };
                readout(ui, "focused", focus);
                let via = match (ui.focus(), ui.is_focus_visible()) {
                    (None, _) => "—",
                    (Some(_), true) => "keyboard — the ring shows",
                    (Some(_), false) => "pointer — no ring",
                };
                readout(ui, "came from", via);
            });
            form(ui, s, form_id, seen);
            checklist(
                ui,
                &[
                    "Click the name field: it takes focus with no ring",
                    "Press Tab: the email field takes focus, and the ring shows",
                    "Tab on: every control below gets the ring in turn, top to bottom",
                    "Shift+Tab walks back the same way",
                    "Click anywhere in a control: the ring goes away",
                ],
            );
        },
    );
}

fn form(ui: &mut Ui, s: &mut State, form_id: WidgetId, seen: &mut Seen) {
    const LABELS: [&str; 8] = [
        "Name",
        "Email",
        "Newsletter",
        "Theme",
        "Volume",
        "Size",
        "Plan",
        "Seats",
    ];
    Grid::new()
        .id(form_id)
        .cols([Track::HUG.with_min(90.0), Track::FILL.with_max(320.0)])
        .rows([Track::HUG; LABELS.len() + 1])
        .gap(16.0)
        .line_gap(10.0)
        .size((Sizing::FILL, Sizing::HUG))
        .show(ui, |ui| {
            for (row, label) in LABELS.iter().enumerate() {
                Text::new(*label)
                    .id_salt(row)
                    .style(&support::note_style())
                    .grid_cell((row as u16, 0))
                    .show(ui);
            }
            let r = TextEdit::new(&mut s.name)
                .placeholder("Ada Lovelace")
                .grid_cell((0, 1))
                .size((Sizing::FILL, Sizing::HUG))
                .show(ui);
            seen.note("Name", r.response.focused);
            let r = TextEdit::new(&mut s.email)
                .placeholder("ada@example.com")
                .grid_cell((1, 1))
                .size((Sizing::FILL, Sizing::HUG))
                .show(ui);
            seen.note("Email", r.response.focused);
            let r = Checkbox::new(&mut s.subscribe)
                .label("Send me the newsletter")
                .grid_cell((2, 1))
                .show(ui);
            seen.note("Newsletter", r.response.focused);
            let r = Switch::new(&mut s.dark)
                .label("Dark")
                .grid_cell((3, 1))
                .show(ui);
            seen.note("Theme", r.response.focused);
            let r = Slider::new(&mut s.volume, 0.0..=1.0)
                .grid_cell((4, 1))
                .show(ui);
            seen.note("Volume", r.response.focused);
            Panel::hstack()
                .gap(12.0)
                .arrow_focus(Axis::X)
                .grid_cell((5, 1))
                .show(ui, |ui| {
                    for (value, label) in [
                        (Size::Small, "Small"),
                        (Size::Medium, "Medium"),
                        (Size::Large, "Large"),
                    ] {
                        let r = RadioButton::new(&mut s.size, value)
                            .id_salt(label)
                            .label(label)
                            .show(ui);
                        seen.note(label, r.response.focused);
                    }
                });
            let r = ComboBox::new(&mut s.plan, &PLANS)
                .grid_cell((6, 1))
                .size((Sizing::fixed(160.0), Sizing::HUG))
                .show(ui);
            seen.note("Plan", r.response.focused);
            let r = DragValue::new(&mut s.seats)
                .range(1.0..=50.0)
                .speed(0.1)
                .editable(true)
                .grid_cell((7, 1))
                .size((Sizing::fixed(100.0), Sizing::HUG))
                .show(ui);
            seen.note("Seats", r.response.focused);
            Panel::hstack().gap(8.0).grid_cell((8, 1)).show(ui, |ui| {
                let r = Button::new().label("Submit").show(ui);
                seen.note("Submit", r.focused);
                let r = Button::new().label("Cancel").show(ui);
                seen.note("Cancel", r.focused);
            });
        });
}

fn order_and_stops(ui: &mut Ui, seen: &mut Seen) {
    section(
        ui,
        "Order and stops",
        &[
            api!(Button::tab_index),
            api!(Button::tab_stop),
            api!(Button::disabled),
        ],
        |ui| {
            note(
                ui,
                "Tab visits stops in ascending tab_index, record order breaking ties, over \
                 the whole window: −1 goes ahead of every unindexed control, 1 behind them. \
                 A control out of the Tab order still takes focus from a click.",
            );
            row(ui, |ui| {
                let r = Button::new().label("tab_index −1").tab_index(-1).show(ui);
                seen.note("tab_index −1", r.focused);
                let r = Button::new().label("tab_index 1").tab_index(1).show(ui);
                seen.note("tab_index 1", r.focused);
                let r = Button::new()
                    .label("tab_stop(false)")
                    .tab_stop(false)
                    .show(ui);
                seen.note("tab_stop(false)", r.focused);
                Button::new().label("disabled").disabled(true).show(ui);
            });
            checklist(
                ui,
                &[
                    "With nothing focused, Tab lands on 'tab_index −1' before the nav rail",
                    "Shift+Tab from 'tab_index −1' wraps to 'tab_index 1', the last stop",
                    "Tab never stops on 'tab_stop(false)', but a click focuses it",
                    "Tab never stops on 'disabled'",
                ],
            );
        },
    );
}

fn arrow_groups(ui: &mut Ui, s: &mut State, seen: &mut Seen) {
    section(ui, "Arrow groups", &[api!(Panel::arrow_focus)], |ui| {
        note(
            ui,
            "Inside a group the arrows along its axis move focus to the next or previous \
                 stop, wrapping — WAI-ARIA's toolbars, menus and lists. Tab still stops on \
                 each item. A control inside that claims an arrow keeps it.",
        );
        Panel::hstack()
            .gap(6.0)
            .padding(6.0)
            .background(support::well_bg())
            .arrow_focus(Axis::X)
            .show(ui, |ui| {
                for label in ["Bold", "Italic", "Underline"] {
                    let r = Button::new().id_salt(label).label(label).show(ui);
                    seen.note(label, r.focused);
                }
                let r = TextEdit::new(&mut s.search)
                    .placeholder("search")
                    .size((Sizing::fixed(160.0), Sizing::HUG))
                    .show(ui);
                seen.note("search", r.response.focused);
                let r = Button::new().label("Clear").show(ui);
                if r.clicked() {
                    s.search.clear();
                }
                seen.note("Clear", r.focused);
            });
        Panel::vstack()
            .gap(2.0)
            .padding(6.0)
            .size((Sizing::fixed(200.0), Sizing::HUG))
            .background(support::well_bg())
            .arrow_focus(Axis::Y)
            .show(ui, |ui| {
                for (i, label) in LIST.iter().enumerate() {
                    let r = Button::new()
                        .id_salt(i)
                        .label(*label)
                        .size((Sizing::FILL, Sizing::HUG))
                        .show(ui);
                    if r.clicked() {
                        s.picked = i;
                    }
                    seen.note(label, r.focused);
                }
            });
        readout(ui, "opened", LIST[s.picked]);
        checklist(
            ui,
            &[
                "Tab onto 'Bold': → moves to 'Italic', 'Underline', then into the search field",
                "In the search field ← and → move the caret; Tab moves on to 'Clear'",
                "→ on 'Clear' wraps to 'Bold', and ← on 'Bold' wraps to 'Clear'",
                "↑ and ↓ walk the list and wrap at both ends; Enter opens the item",
            ],
        );
    });
}

fn containment(ui: &mut Ui, s: &mut State, seen: &mut Seen) {
    section(
        ui,
        "Focus inside overlays",
        &[
            api!(Modal::new),
            api!(PopupTrigger::on),
            api!(Ui::focus_first_within),
        ],
        |ui| {
            note(
                ui,
                "An open dialog keeps Tab inside itself, and a popup does while it holds \
                 focus. When either closes, focus goes back to the control that opened it. \
                 A popup opened from the keyboard takes focus; one opened by a click does \
                 not.",
            );
            row(ui, |ui| {
                let r = Button::new().label("Open dialog").show(ui);
                seen.note("Open dialog", r.focused);
                if r.clicked() {
                    s.dialog_open = true;
                }
                let trigger = Button::new().label("Filters ▾").show(ui);
                seen.note("Filters ▾", trigger.focused);
                let trigger = trigger.snapshot();
                PopupTrigger::on(&trigger)
                    .padding(10.0)
                    .gap(6.0)
                    .show(ui, |ui, close| {
                        for (i, label) in ["Unread", "Starred", "Has files"].into_iter().enumerate()
                        {
                            Checkbox::new(&mut s.filters[i])
                                .id_salt(i)
                                .label(label)
                                .show(ui);
                        }
                        if Button::new().label("Done").show(ui).clicked() {
                            close.close();
                        }
                    });
            });
            if s.dialog_open {
                dialog(ui, s);
            }
            checklist(
                ui,
                &[
                    "Open the dialog: focus moves to its first field",
                    "Tab and Shift+Tab cycle inside the dialog only",
                    "Esc or Cancel closes it, and 'Open dialog' has focus again",
                    "Tab to 'Filters ▾' and press Space: the popup opens with focus inside",
                    "Click 'Filters ▾': the popup opens and focus stays on the button",
                    "Esc in the popup closes it and focus returns to 'Filters ▾'",
                ],
            );
        },
    );
}

fn dialog(ui: &mut Ui, s: &mut State) {
    let resp = Modal::new().show(ui, |ui, close| {
        Panel::vstack()
            .gap(12.0)
            .size((Sizing::fixed(300.0), Sizing::HUG))
            .show(ui, |ui| {
                Text::new("Rename layer")
                    .style(&support::body_style())
                    .show(ui);
                TextEdit::new(&mut s.dialog_note)
                    .placeholder("new name")
                    .size((Sizing::FILL, Sizing::HUG))
                    .show(ui);
                row(ui, |ui| {
                    if Button::new().label("Cancel").show(ui).clicked() {
                        close.close();
                    }
                    if Button::new().label("Rename").show(ui).clicked() {
                        close.close();
                    }
                });
            });
    });
    if resp.closed() {
        s.dialog_open = false;
    }
}

fn programmatic(ui: &mut Ui, s: &mut State, form_id: WidgetId, seen: &mut Seen) {
    section(
        ui,
        "Focus from code",
        &[
            api!(Ui::set_focus),
            api!(Ui::clear_focus),
            api!(Ui::focus_first_within),
            api!(TextEdit::select_all_on_focus),
        ],
        |ui| {
            note(
                ui,
                "set_focus keeps whether the ring shows from the last input, so a button \
                 clicked with the pointer moves focus without a ring, and one pressed with \
                 Space moves it with one.",
            );
            row(ui, |ui| {
                let field = TextEdit::new(&mut s.target)
                    .select_all_on_focus(true)
                    .size((Sizing::fixed(180.0), Sizing::HUG))
                    .show(ui);
                seen.note("the target field", field.response.focused);
                let field = field.response.id;
                if Button::new().label("Focus the field").show(ui).clicked() {
                    ui.set_focus(field);
                }
                if Button::new().label("Focus the form").show(ui).clicked() {
                    ui.focus_first_within(form_id);
                }
                if Button::new().label("Clear focus").show(ui).clicked() {
                    ui.clear_focus();
                }
            });
            checklist(
                ui,
                &[
                    "'Focus the field' focuses it with its text selected",
                    "'Focus the form' focuses the name field at the top of the page",
                    "'Clear focus' leaves nothing focused",
                ],
            );
        },
    );
}

fn policy(ui: &mut Ui, s: &mut State) {
    section(
        ui,
        "Focus policy",
        &[
            api!(Ui::set_focus_policy),
            api!(FocusPolicy::PreserveOnMiss),
        ],
        |ui| {
            note(
                ui,
                "What a press on nothing focusable does to focus. The shell puts the default \
                 back on every other page, so the choice here stays on this page.",
            );
            row(ui, |ui| {
                for (value, label) in [
                    (FocusPolicy::ClearOnMiss, "ClearOnMiss — drop focus"),
                    (FocusPolicy::PreserveOnMiss, "PreserveOnMiss — keep it"),
                ] {
                    RadioButton::new(&mut s.policy, value)
                        .id_salt(label)
                        .label(label)
                        .show(ui);
                }
            });
            checklist(
                ui,
                &[
                    "ClearOnMiss: focus a field, click empty card space: nothing is focused",
                    "PreserveOnMiss: the same click leaves the field focused",
                ],
            );
        },
    );
    ui.set_focus_policy(s.policy);
}

/// Edits the live theme's ring, one slot for every widget.
fn ring(ui: &mut Ui) {
    section(
        ui,
        "Ring theme",
        &[api!(Ui::theme), api!(type FocusRingTheme)],
        |ui| {
            note(
                ui,
                "The framework draws the ring, not the widget, from one slot of the theme. \
                 The values below edit the theme in use, so every page shows the change.",
            );
            let ring = &ui.theme().focus_ring;
            let mut width = f64::from(ring.width);
            let mut color = ring.color;
            row(ui, |ui| {
                Text::new("width").style(&support::note_style()).show(ui);
                Slider::new(&mut width, 0.0..=4.0)
                    .step(0.5)
                    .size((Sizing::fixed(160.0), Sizing::HUG))
                    .show(ui);
                Text::new("colour").style(&support::note_style()).show(ui);
                ColorButton::new(ColorPicker::new(&mut color)).show(ui);
                if Button::new().label("Reset").show(ui).clicked() {
                    let default = FocusRingTheme::from_palette(&shell::showcase_palette());
                    width = f64::from(default.width);
                    color = default.color;
                }
            });
            set_ring(ui, width as f32, color);
            checklist(
                ui,
                &[
                    "Tab to the width slider and press → : the ring around it grows",
                    "Width 0 draws no ring at all",
                    "Open another page and press Tab: its ring has the same width and colour",
                    "Reset puts the ring back to 2 px teal",
                ],
            );
        },
    );
}

fn set_ring(ui: &mut Ui, width: f32, color: RgbaF32) {
    let ring = &ui.theme().focus_ring;
    if ring.width == width && ring.color == color {
        return;
    }
    let mut theme = Theme::clone(ui.theme());
    theme.focus_ring = FocusRingTheme { color, width };
    ui.set_theme(theme);
}
