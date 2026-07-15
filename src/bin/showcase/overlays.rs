//! Side-layer widgets on one page. `Popup::anchored_to` records a side
//! root in the `Popup` layer that paints above the main tree, escapes
//! ancestor clip, and hit-tests on top. Tooltips live one layer higher
//! still, with a ~0.5 s delay and a warmup window (move between
//! adjacent triggers within ~1 s and the next bubble skips the delay).
//! ContextMenus attach to any sensed widget and auto-open on
//! secondary-click at the pointer position.

use crate::support;
use crate::support::{row, section, surface_bg};
use aperture::{
    Align, Button, Configure, ContextMenu, Frame, Justify, Key, MenuItem, Mods, Panel, Popup, Rect,
    ResponseSnapshot, Sense, Shortcut, Sizing, Spacing, Text, Tooltip, Ui, WidgetId,
};

pub(crate) fn build(ui: &mut Ui) {
    support::page(ui, |ui| {
        support::header(
            ui,
            "Side layers — popup, tooltips, and context menus paint above the \
             main tree and hit-test on top.",
        );
        popup_section(ui);
        tooltip_section(ui);
        context_menu_section(ui);
    });
}

#[derive(Default)]
struct MenuState {
    open: bool,
    last_choice: Option<&'static str>,
}

fn popup_section(ui: &mut Ui) {
    let menu_id = WidgetId::from_hash("showcase::overlays::popup");
    let mut trigger_rect: Option<Rect> = None;
    let mut clicked = false;

    section(
        ui,
        "popup",
        "Popup — click the button; the menu paints on the Popup layer, outside-click dismisses",
        |ui| {
            row(ui, "popup-trigger-row", |ui| {
                let r = Button::new()
                    .id_salt("popup-trigger")
                    .label("menu")
                    .show(ui);
                if r.left.clicked() {
                    clicked = true;
                }
                trigger_rect = r.rect;

                let label = ui
                    .state_mut::<MenuState>(menu_id)
                    .last_choice
                    .unwrap_or("(no selection yet)");
                Text::new(label).id_salt("popup-choice").show(ui);
            });
        },
    );

    if clicked {
        let s = ui.state_mut::<MenuState>(menu_id);
        s.open = !s.open;
    }
    if !ui.state_mut::<MenuState>(menu_id).open {
        return;
    }
    let Some(trigger) = trigger_rect else {
        return;
    };

    let anchor = glam::Vec2::new(trigger.min.x, trigger.min.y + trigger.size.h + 4.0);
    let mut chosen: Option<&'static str> = None;
    let resp = Popup::anchored_to(anchor)
        .id_salt("popup-menu")
        .padding(6.0)
        .size((Sizing::Hug, Sizing::Hug))
        // `min_size` floors the body so the popup doesn't collapse to
        // bare label width — the inner Fill buttons then expand to the
        // floored width.
        .min_size((220.0, 110.0))
        .max_size((280, 200))
        .justify(Justify::Center)
        .child_align(Align::CENTER)
        .gap(10.0)
        .background(surface_bg())
        .show(ui, |ui, _popup| {
            for label in ["copy", "paste", "delete"] {
                if Button::new()
                    .id_salt(("popup-item", label))
                    .label(label)
                    .size((Sizing::FILL, Sizing::Hug))
                    .show(ui)
                    .left
                    .clicked()
                {
                    chosen = Some(label);
                }
            }
        });

    let s = ui.state_mut::<MenuState>(menu_id);
    if let Some(label) = chosen {
        s.last_choice = Some(label);
        s.open = false;
    } else if resp.dismissed {
        s.open = false;
    }
}

fn tooltip_section(ui: &mut Ui) {
    section(
        ui,
        "tooltips",
        "Tooltip — hover ~0.5 s; delays, wrap width, disabled rules, and the warmup window",
        |ui| {
            row(ui, "tt-delays", |ui| {
                let r = Button::new()
                    .id_salt("d-default")
                    .label("default")
                    .show(ui)
                    .snapshot();
                Tooltip::on(&r)
                    .text("Default 0.5 s delay before this appears.")
                    .show(ui);

                let r = Button::new()
                    .id_salt("d-instant")
                    .label("instant")
                    .show(ui)
                    .snapshot();
                Tooltip::on(&r)
                    .text("No delay — fires the frame the pointer arrives.")
                    .delay(0.0)
                    .show(ui);

                let r = Button::new()
                    .id_salt("d-slow")
                    .label("slow (1.5 s)")
                    .show(ui)
                    .snapshot();
                Tooltip::on(&r)
                    .text("Held for 1.5 s before showing.")
                    .delay(1.5)
                    .show(ui);

                let r = Button::new()
                    .id_salt("w-1")
                    .label("long text")
                    .show(ui)
                    .snapshot();
                Tooltip::on(&r)
                    .text(
                        "Tooltips wrap to the configured max width — the default \
                         is 280 logical pixels. Long bodies stack into multiple \
                         lines automatically; the bubble's height hugs the \
                         shaped text.",
                    )
                    .show(ui);

                let r = Button::new()
                    .id_salt("w-2")
                    .label("narrow")
                    .show(ui)
                    .snapshot();
                Tooltip::on(&r)
                    .text("Override max width to force tighter wrap on a single tooltip.")
                    .max_size((140.0, f32::INFINITY))
                    .show(ui);
            });

            row(ui, "tt-disabled", |ui| {
                let r = Button::new()
                    .id_salt("dis-1")
                    .label("disabled (no tooltip)")
                    .disabled(true)
                    .show(ui)
                    .snapshot();
                Tooltip::on(&r)
                    .text("This text is suppressed by the default skip-on-disabled rule.")
                    .show(ui);

                let r = Button::new()
                    .id_salt("dis-2")
                    .label("disabled (with tooltip)")
                    .disabled(true)
                    .show(ui)
                    .snapshot();
                Tooltip::on(&r)
                    .text("Opt-in via .show_when_disabled(true) for 'why is this disabled' hints.")
                    .show_when_disabled(true)
                    .show(ui);

                for i in 0..5 {
                    let r = Button::new()
                        .id_salt(("warm", i))
                        .label(format!("item {}", i + 1))
                        .show(ui)
                        .snapshot();
                    Tooltip::on(&r)
                        .text(match i {
                            0 => "Hover, then move to the next item within ~1 s.",
                            1 => "See how the next bubble appears instantly?",
                            2 => "Warmup window keeps scanning a row snappy.",
                            3 => "Pause for ~1 s and the next one re-delays.",
                            _ => "Last one.",
                        })
                        .show(ui);
                }
            });
        },
    );
}

#[derive(Default)]
struct CtxState {
    last_action: Option<&'static str>,
}

fn context_menu_section(ui: &mut Ui) {
    let state_id = WidgetId::from_hash("showcase::overlays::ctx-menu");

    section(
        ui,
        "ctx",
        "ContextMenu — right-click the button or either surface; item click, outside-click, or Esc dismisses",
        |ui| {
            row(ui, "ctx-trigger-row", |ui| {
                let trigger = Button::new()
                    .id_salt("ctx-button-trigger")
                    .label("right-click me")
                    .show(ui)
                    .snapshot();
                attach_menu(ui, &trigger, state_id, MenuFlavor::Default);

                // Static strings only — no per-frame alloc.
                let label = ui
                    .state_mut::<CtxState>(state_id)
                    .last_action
                    .unwrap_or("last action: (none yet)");
                Text::new(label).id_salt("ctx-status").show(ui);
            });

            Panel::hstack()
                .id_salt("ctx-surfaces")
                .gap(12.0)
                .size((Sizing::FILL, Sizing::Hug))
                .show(ui, |ui| {
                    // A generic Frame surface (Sense::CLICK so it can
                    // receive secondary clicks) with the theme-driven
                    // default menu look.
                    let surface = Frame::new()
                        .id_salt("ctx-surface")
                        .size((Sizing::FILL, Sizing::Fixed(90.0)))
                        .sense(Sense::CLICK)
                        .background(surface_bg())
                        .show(ui)
                        .snapshot();
                    attach_menu(ui, &surface, state_id, MenuFlavor::Default);

                    // Same items, configured wider with bigger padding and
                    // a max width — ContextMenu's Configure surface.
                    let wide = Frame::new()
                        .id_salt("ctx-wide-surface")
                        .size((Sizing::FILL, Sizing::Fixed(90.0)))
                        .sense(Sense::CLICK)
                        .background(support::panel_bg())
                        .show(ui)
                        .snapshot();
                    attach_menu(ui, &wide, state_id, MenuFlavor::Wide);
                });
        },
    );
}

#[derive(Copy, Clone)]
enum MenuFlavor {
    Default,
    Wide,
}

fn attach_menu(ui: &mut Ui, trigger: &ResponseSnapshot, state_id: WidgetId, flavor: MenuFlavor) {
    let mut menu = ContextMenu::attach(ui, trigger).size((Sizing::Hug, Sizing::Hug));
    if let MenuFlavor::Wide = flavor {
        menu = menu
            .min_size((260.0, 0.0))
            .max_size((320.0, 280.0))
            .padding(Spacing::all(10.0));
    }
    menu.show(ui, |ui, popup| {
        if MenuItem::new("Copy")
            .shortcut(Shortcut::ctrl('C'))
            .show(ui, popup)
            .left
            .clicked()
        {
            ui.state_mut::<CtxState>(state_id).last_action = Some("last action: Copy");
        }
        if MenuItem::new("Cut")
            .shortcut(Shortcut::ctrl('X'))
            .show(ui, popup)
            .left
            .clicked()
        {
            ui.state_mut::<CtxState>(state_id).last_action = Some("last action: Cut");
        }
        if MenuItem::new("Paste")
            .shortcut(Shortcut::ctrl('V'))
            .show(ui, popup)
            .left
            .clicked()
        {
            ui.state_mut::<CtxState>(state_id).last_action = Some("last action: Paste");
        }
        MenuItem::separator(ui);
        MenuItem::new("Disabled").enabled(false).show(ui, popup);
        MenuItem::separator(ui);
        if MenuItem::new("Delete")
            .shortcut(Shortcut::new(Mods::NONE, Key::Backspace))
            .show(ui, popup)
            .left
            .clicked()
        {
            ui.state_mut::<CtxState>(state_id).last_action = Some("last action: Delete");
        }
    });
}
