//! Side-layer widgets. `Popup::anchored_to` records a side root in the
//! `Popup` layer that paints above the main tree, escapes ancestor clip,
//! and hit-tests on top. Tooltips live one layer higher still, with a
//! ~0.5 s delay and a warmup window — move between adjacent triggers
//! within ~1 s and the next bubble skips the delay. ContextMenus attach
//! to any sensed widget and auto-open on secondary-click at the pointer.

use std::time::Duration;

use crate::support;
use crate::support::{note_style, raised_bg, row, section};
use palantir::{
    Align, Button, Configure, ContextMenu, ContextMenuTheme, Frame, Justify, Key, MenuItem, Mods,
    Panel, Popup, Rect, ResponseSnapshot, Sense, Shortcut, Sizing, Spacing, Text, Tooltip, Ui,
    Vec2, WidgetId, fmt,
};

pub(crate) fn build(ui: &mut Ui) {
    popup_section(ui);
    tooltip_section(ui);
    context_menu_section(ui);
}

#[derive(Default, Debug)]
struct MenuState {
    open: bool,
    last_choice: Option<&'static str>,
}

fn popup_section(ui: &mut Ui) {
    let menu_id = WidgetId::from_hash("showcase::overlays::popup");
    ui.with_state::<MenuState, _>(menu_id, popup_menu);
}

fn popup_menu(ui: &mut Ui, menu: &mut MenuState) {
    let mut trigger_rect: Option<Rect> = None;
    let mut clicked = false;

    section(
        ui,
        "popup — paints on the Popup layer, above and outside the main tree; an \
         outside click dismisses it",
        |ui| {
            row(ui, |ui| {
                let r = Button::new()
                    .id_salt("popup-trigger")
                    .label("menu")
                    .show(ui);
                if r.left.clicked() {
                    clicked = true;
                }
                trigger_rect = r.rect;

                let label = menu.last_choice.unwrap_or("(no selection yet)");
                Text::new(label)
                    .id_salt("popup-choice")
                    .style(&note_style())
                    .show(ui);
            });
        },
    );

    menu.open ^= clicked;
    if !menu.open {
        return;
    }
    let Some(trigger) = trigger_rect else {
        return;
    };

    let anchor = Vec2::new(trigger.min.x, trigger.min.y + trigger.size.h + 4.0);
    let mut chosen: Option<&'static str> = None;
    let resp = Popup::anchored_to(anchor)
        .id_salt("popup-menu")
        .padding(6.0)
        .size((Sizing::HUG, Sizing::HUG))
        // `min_size` floors the body so the popup doesn't collapse to
        // bare label width — the inner Fill buttons then expand to the
        // floored width.
        .min_size((220.0, 110.0))
        .max_size((280, 200))
        .justify(Justify::Center)
        .child_align(Align::CENTER)
        .gap(10.0)
        .background(raised_bg())
        .show(ui, |ui, _popup| {
            for label in ["copy", "paste", "delete"] {
                if Button::new()
                    .id_salt(("popup-item", label))
                    .label(label)
                    .size((Sizing::FILL, Sizing::HUG))
                    .show(ui)
                    .left
                    .clicked()
                {
                    chosen = Some(label);
                }
            }
        });

    if let Some(label) = chosen {
        menu.last_choice = Some(label);
        menu.open = false;
    } else if resp.dismissed {
        menu.open = false;
    }
}

fn tooltip_section(ui: &mut Ui) {
    section(
        ui,
        "tooltips — hover ~0.5 s; delays, wrap width, and the disabled rules",
        |ui| {
            row(ui, |ui| {
                let r = Button::new()
                    .id_salt("d-default")
                    .label("default")
                    .show(ui)
                    .snapshot();
                Tooltip::on(&r)
                    .label("Default 0.5 s delay before this appears.")
                    .show(ui);

                let r = Button::new()
                    .id_salt("d-instant")
                    .label("instant")
                    .show(ui)
                    .snapshot();
                Tooltip::on(&r)
                    .label("No delay — fires the frame the pointer arrives.")
                    .delay(Duration::ZERO)
                    .show(ui);

                let r = Button::new()
                    .id_salt("d-slow")
                    .label("slow (1.5 s)")
                    .show(ui)
                    .snapshot();
                Tooltip::on(&r)
                    .label("Held for 1.5 s before showing.")
                    .delay(Duration::from_millis(1_500))
                    .show(ui);

                let r = Button::new()
                    .id_salt("w-1")
                    .label("long text")
                    .show(ui)
                    .snapshot();
                Tooltip::on(&r)
                    .label(
                        "Tooltips wrap to the configured max width — the default is \
                         280 logical pixels. Long bodies stack into multiple lines \
                         automatically; the bubble's height hugs the shaped text.",
                    )
                    .show(ui);

                let r = Button::new()
                    .id_salt("w-2")
                    .label("narrow")
                    .show(ui)
                    .snapshot();
                Tooltip::on(&r)
                    .label("Override max width to force tighter wrap on a single tooltip.")
                    .max_size((140.0, f32::INFINITY))
                    .show(ui);

                let r = Button::new()
                    .id_salt("dis-1")
                    .label("disabled (no tooltip)")
                    .disabled(true)
                    .show(ui)
                    .snapshot();
                Tooltip::on(&r)
                    .label("This text is suppressed by the default skip-on-disabled rule.")
                    .show(ui);

                let r = Button::new()
                    .id_salt("dis-2")
                    .label("disabled (with tooltip)")
                    .disabled(true)
                    .show(ui)
                    .snapshot();
                Tooltip::on(&r)
                    .label("Opt in via .show_when_disabled(true) for 'why is this disabled' hints.")
                    .show_when_disabled(true)
                    .show(ui);
            });
        },
    );

    section(
        ui,
        "tooltip warmup — hover one, then move along the row within ~1 s and the \
         next bubble skips its delay; pause and it re-delays",
        |ui| {
            row(ui, |ui| {
                for i in 0..5 {
                    let r = Button::new()
                        .id_salt(("warm", i))
                        .label(fmt!(ui, "item {}", i + 1))
                        .show(ui)
                        .snapshot();
                    Tooltip::on(&r)
                        .label(match i {
                            0 => "Hover, then move to the next item within ~1 s.",
                            1 => "See how the next bubble appears instantly?",
                            2 => "The warmup window keeps scanning a row snappy.",
                            3 => "Pause for ~1 s and the next one re-delays.",
                            _ => "Last one.",
                        })
                        .show(ui);
                }
            });
        },
    );
}

#[derive(Default, Debug)]
struct CtxState {
    last_action: Option<&'static str>,
}

fn context_menu_section(ui: &mut Ui) {
    let state_id = WidgetId::from_hash("showcase::overlays::ctx-menu");

    section(
        ui,
        "context menu — right-click the button or either surface; an item click, an \
         outside click, or Esc dismisses",
        |ui| {
            row(ui, |ui| {
                let trigger = Button::new()
                    .id_salt("ctx-button-trigger")
                    .label("right-click me")
                    .show(ui)
                    .snapshot();
                attach_menu(ui, &trigger, state_id, Flavor::Default);

                // Static strings only — no per-frame alloc.
                let label = ui
                    .state_mut::<CtxState>(state_id)
                    .last_action
                    .unwrap_or("last action: (none yet)");
                Text::new(label)
                    .id_salt("ctx-status")
                    .style(&note_style())
                    .show(ui);
            });

            Panel::hstack()
                .id_salt("ctx-surfaces")
                .gap(12.0)
                .size((Sizing::FILL, Sizing::HUG))
                .show(ui, |ui| {
                    // A generic Frame surface (Sense::CLICK so it can
                    // receive secondary clicks) with the theme-driven
                    // default menu look.
                    let surface = Frame::new()
                        .id_salt("ctx-surface")
                        .size((Sizing::FILL, Sizing::fixed(90.0)))
                        .sense(Sense::CLICK)
                        .background(raised_bg())
                        .show(ui)
                        .snapshot();
                    attach_menu(ui, &surface, state_id, Flavor::Default);

                    // Same items, configured wider with bigger padding and
                    // a maximum width.
                    let wide = Frame::new()
                        .id_salt("ctx-wide-surface")
                        .size((Sizing::FILL, Sizing::fixed(90.0)))
                        .sense(Sense::CLICK)
                        .background(support::well_bg())
                        .show(ui)
                        .snapshot();
                    attach_menu(ui, &wide, state_id, Flavor::Wide);
                });
        },
    );
}

#[derive(Copy, Clone, Debug)]
enum Flavor {
    Default,
    Wide,
}

/// The `Wide` flavor's per-instance theme: same palette, looser
/// everything. Built per call rather than kept in a static because a
/// `ContextMenuTheme` isn't `const`-constructible; it's one small
/// struct on an already-open menu's frame.
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

fn attach_menu(ui: &mut Ui, trigger: &ResponseSnapshot, state_id: WidgetId, flavor: Flavor) {
    // `Wide` restyles through the theme bundle every menu widget reads;
    // the panel takes it via `.style`, and the rows — recorded by this
    // closure, not by `ContextMenu` — take their own halves of it. Every
    // `style` setter takes an `Option`, so "styled or default" stays a value
    // threaded through the tree rather than a branch around each widget.
    let style = matches!(flavor, Flavor::Wide).then(|| roomy_menu_theme(ui));
    let mut menu = ContextMenu::attach(ui, trigger)
        .size((Sizing::HUG, Sizing::HUG))
        .style(style.as_ref());
    if style.is_some() {
        menu = menu.min_size((260.0, 0.0)).max_size((320.0, 280.0));
    }
    menu.show(ui, |ui, popup| {
        let item = style.as_ref().map(|s| &s.item);
        let rule = style.as_ref().map(|s| &s.separator);
        for (label, shortcut, action) in [
            ("Copy", Shortcut::ctrl('C'), "last action: Copy"),
            ("Cut", Shortcut::ctrl('X'), "last action: Cut"),
            ("Paste", Shortcut::ctrl('V'), "last action: Paste"),
        ] {
            if MenuItem::new(label)
                .shortcut(shortcut)
                .style(item)
                .show(ui, popup)
                .left
                .clicked()
            {
                ui.state_mut::<CtxState>(state_id).last_action = Some(action);
            }
        }
        MenuItem::separator().style(rule).show(ui);
        MenuItem::new("Disabled")
            .disabled(true)
            .style(item)
            .show(ui, popup);
        MenuItem::separator().style(rule).show(ui);
        if MenuItem::new("Delete")
            .shortcut(Shortcut::new(Mods::NONE, Key::Backspace))
            .style(item)
            .show(ui, popup)
            .left
            .clicked()
        {
            ui.state_mut::<CtxState>(state_id).last_action = Some("last action: Delete");
        }
    });
}
