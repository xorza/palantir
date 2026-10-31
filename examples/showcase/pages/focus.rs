//! Keyboard focus end to end. Palantir's harness tests hold each behavior a section shows, so the
//! page carries no list to check by hand.

use crate::scaffold::api;
use crate::support;
use crate::support::{Kit, Section};
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

/// The form whose first field "Focus the form" focuses.
fn form_id() -> WidgetId {
    WidgetId::from_hash("showcase::focus::form")
}

pub(crate) fn build(ui: &mut Ui, k: Kit) {
    let state_id = WidgetId::from_hash("showcase::focus::state");
    ui.with_state::<State, _>(state_id, |ui, s| page(ui, k, s));
}

fn page(ui: &mut Ui, k: Kit, s: &mut State) {
    let mut seen = Seen::default();

    tab_order(ui, k, s, &mut seen);
    order_and_stops(ui, k, &mut seen);
    arrow_groups(ui, k, s, &mut seen);
    containment(ui, k, s, &mut seen);
    programmatic(ui, k, s, &mut seen);
    policy(ui, k, s);
    ring(ui, k);

    // Responses read last frame's focus, so a change shows one frame late.
    if seen.0 != s.focused {
        s.focused = seen.0;
        ui.request_repaint();
    }
}

fn tab_order(ui: &mut Ui, k: Kit, s: &mut State, seen: &mut Seen) {
    Section::new("Tab order")
        .apis(&[api!(Ui::focus), api!(Ui::is_focus_visible)])
        .show(ui, k, |ui| {
            support::note(
                ui,
                k,
                "Tab and Shift+Tab visit every enabled control in record order, wrapping \
                 at the ends of the window. The ring shows only when focus came from the \
                 keyboard, as CSS's :focus-visible does.",
            );
            support::well(ui, k, |ui| {
                let focus = match (s.focused, ui.focus()) {
                    (Some(name), _) => name,
                    (None, Some(_)) => "a control outside this page's demos",
                    (None, None) => "nothing",
                };
                support::readout(ui, k, "focused", focus);
                let via = match (ui.focus(), ui.is_focus_visible()) {
                    (None, _) => "—",
                    (Some(_), true) => "keyboard — the ring shows",
                    (Some(_), false) => "pointer — no ring",
                };
                support::readout(ui, k, "came from", via);
            });
            form(ui, k, s, seen);
        });
}

fn form(ui: &mut Ui, k: Kit, s: &mut State, seen: &mut Seen) {
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
        .id(form_id())
        .cols([Track::HUG.with_min(90.0), Track::FILL.with_max(320.0)])
        .rows([Track::HUG; LABELS.len() + 1])
        .gap(16.0)
        .line_gap(10.0)
        .size((Sizing::FILL, Sizing::HUG))
        .show(ui, |ui| {
            for (row, label) in LABELS.iter().enumerate() {
                Text::new(*label)
                    .id_salt(row)
                    .style(&k.t.caption())
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

fn order_and_stops(ui: &mut Ui, k: Kit, seen: &mut Seen) {
    Section::new("Order and stops")
        .apis(&[
            api!(Button::tab_idx),
            api!(Button::tab_stop),
            api!(Button::disabled),
        ])
        .show(ui, k, |ui| {
            support::note(
                ui,
                k,
                "Tab visits stops in ascending tab_idx, record order breaking ties, over \
                 the whole window: −1 goes ahead of every unindexed control, 1 behind them. \
                 A control out of the Tab order still takes focus from a click.",
            );
            support::row(ui, |ui| {
                let r = Button::new().label("tab_idx −1").tab_idx(-1).show(ui);
                seen.note("tab_idx −1", r.focused);
                let r = Button::new().label("tab_idx 1").tab_idx(1).show(ui);
                seen.note("tab_idx 1", r.focused);
                let r = Button::new()
                    .label("tab_stop(false)")
                    .tab_stop(false)
                    .show(ui);
                seen.note("tab_stop(false)", r.focused);
                Button::new().label("disabled").disabled(true).show(ui);
            });
        });
}

fn arrow_groups(ui: &mut Ui, k: Kit, s: &mut State, seen: &mut Seen) {
    Section::new("Arrow groups")
        .apis(&[api!(Panel::arrow_focus)])
        .show(ui, k, |ui| {
            support::note(
                ui,
                k,
                "Inside a group the arrows along its axis move focus to the next or previous \
                 stop, wrapping — WAI-ARIA's toolbars, menus and lists. Tab still stops on \
                 each item. A control inside that claims an arrow keeps it.",
            );
            Panel::hstack()
                .gap(6.0)
                .padding(6.0)
                .chrome(k.t.card())
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
                .chrome(k.t.card())
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
            support::readout(ui, k, "opened", LIST[s.picked]);
        });
}

fn containment(ui: &mut Ui, k: Kit, s: &mut State, seen: &mut Seen) {
    Section::new("Focus inside overlays")
        .apis(&[
            api!(Modal::new),
            api!(PopupTrigger::on),
            api!(Ui::focus_first_within),
        ])
        .show(ui, k, |ui| {
            support::note(
                ui,
                k,
                "An open dialog keeps Tab inside itself, and a popup does while it holds \
                 focus. When either closes, focus goes back to the control that opened it. \
                 A popup opened from the keyboard takes focus; one opened by a click does \
                 not.",
            );
            support::row(ui, |ui| {
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
                dialog(ui, k, s);
            }
        });
}

fn dialog(ui: &mut Ui, k: Kit, s: &mut State) {
    let resp = Modal::new().show(ui, |ui, close| {
        Panel::vstack()
            .gap(12.0)
            .size((Sizing::fixed(300.0), Sizing::HUG))
            .show(ui, |ui| {
                Text::new("Rename layer").style(&k.t.body()).show(ui);
                TextEdit::new(&mut s.dialog_note)
                    .placeholder("new name")
                    .size((Sizing::FILL, Sizing::HUG))
                    .show(ui);
                support::row(ui, |ui| {
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

fn programmatic(ui: &mut Ui, k: Kit, s: &mut State, seen: &mut Seen) {
    Section::new("Focus from code")
        .apis(&[
            api!(Ui::set_focus),
            api!(Ui::clear_focus),
            api!(Ui::focus_first_within),
            api!(TextEdit::select_all_on_focus),
        ])
        .show(ui, k, |ui| {
            support::note(
                ui,
                k,
                "set_focus keeps whether the ring shows from the last input, so a button \
                 clicked with the pointer moves focus without a ring, and one pressed with \
                 Space moves it with one.",
            );
            support::row(ui, |ui| {
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
                    ui.focus_first_within(form_id());
                }
                if Button::new().label("Clear focus").show(ui).clicked() {
                    ui.clear_focus();
                }
            });
        });
}

fn policy(ui: &mut Ui, k: Kit, s: &mut State) {
    Section::new("Focus policy")
        .apis(&[
            api!(Ui::set_focus_policy),
            api!(FocusPolicy::PreserveOnMiss),
        ])
        .show(ui, k, |ui| {
            support::note(
                ui,
                k,
                "What a press on nothing focusable does to focus. The shell puts the default \
                 back on every other page, so the choice here stays on this page.",
            );
            support::row(ui, |ui| {
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
        });
    ui.set_focus_policy(s.policy);
}

/// Edits the live theme's ring, one slot for every widget.
fn ring(ui: &mut Ui, k: Kit) {
    Section::new("Ring theme")
        .apis(&[api!(Ui::theme), api!(type FocusRingTheme)])
        .show(ui, k, |ui| {
            support::note(
                ui,
                k,
                "The framework draws the ring, not the widget, from one slot of the theme. \
                 The values below edit the theme in use, so every page shows the change.",
            );
            let ring = &ui.theme().focus_ring;
            let mut width = f64::from(ring.width);
            let mut color = ring.color;
            support::row(ui, |ui| {
                Text::new("width").style(&k.t.caption()).show(ui);
                Slider::new(&mut width, 0.0..=4.0)
                    .step(0.5)
                    .size((Sizing::fixed(160.0), Sizing::HUG))
                    .show(ui);
                Text::new("color").style(&k.t.caption()).show(ui);
                ColorButton::new(ColorPicker::new(&mut color)).show(ui);
                if Button::new().label("Reset").show(ui).clicked() {
                    let default = FocusRingTheme::default();
                    width = f64::from(default.width);
                    color = default.color;
                }
            });
            set_ring(ui, width as f32, color);
        });
}

fn set_ring(ui: &mut Ui, width: f32, color: RgbaF32) {
    let ring = &ui.theme().focus_ring;
    if ring.width == width && ring.color == color {
        return;
    }
    let mut theme = Theme::clone(ui.theme());
    theme.focus_ring = FocusRingTheme { color, width };
    let scheme = ui.color_scheme();
    ui.set_theme_for(scheme, theme);
}
