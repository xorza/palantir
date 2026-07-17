use crate::harness::{audit_steady_state, audit_text_steady_state};
use aperture::{
    Background, Button, Color, Configure, ContextMenu, Frame, Grid, MenuItem, Panel, Scroll,
    Shortcut, Sizing, Splitter, Text, TextEdit, Track, Ui, Vec2, WidgetId,
};
use std::rc::Rc;

#[test]
fn empty_frame_alloc_free() {
    audit_steady_state("empty_frame", 0, |_ui| {});
}

#[test]
fn button_only_alloc_free() {
    audit_steady_state("button_only", 0, |ui| {
        Button::new()
            .auto_id()
            .label("hello")
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui);
    });
}

#[test]
fn nested_vstack_64_alloc_free() {
    audit_steady_state("nested_vstack_64", 0, |ui| {
        fn rec(ui: &mut Ui, depth: u32) {
            if depth == 0 {
                return;
            }
            Panel::vstack()
                .id_salt(depth)
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| rec(ui, depth - 1));
        }
        rec(ui, 64);
    });
}

#[test]
fn grid_8x8_alloc_free() {
    let cols: Rc<[Track]> = Rc::from([Track::fill(); 8]);
    let rows: Rc<[Track]> = Rc::from([Track::fill(); 8]);
    audit_steady_state("grid_8x8", 0, move |ui| {
        Grid::new()
            .auto_id()
            .cols(Rc::clone(&cols))
            .rows(Rc::clone(&rows))
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                for r in 0..8u16 {
                    for c in 0..8u16 {
                        Frame::new()
                            .id_salt((r, c))
                            .background(Background {
                                fill: Color::WHITE.into(),
                                ..Default::default()
                            })
                            .grid_cell((r, c))
                            .show(ui);
                    }
                }
            });
    });
}

#[test]
fn splitter_alloc_free() {
    let mut ratio = 0.5;
    audit_steady_state("splitter", 0, move |ui| {
        Splitter::horizontal(&mut ratio)
            .id_salt("splitter")
            .min_pane(80.0)
            .show(ui, |_, _| {});
    });
}

#[test]
fn damage_animated_rect_alloc_free() {
    let mut tick: u32 = 0;
    audit_steady_state("damage_animated_rect", 0, move |ui| {
        tick = tick.wrapping_add(1);
        let w = 100.0 + (tick % 200) as f32;
        Panel::vstack().auto_id().show(ui, |ui| {
            Frame::new()
                .auto_id()
                .background(Background {
                    fill: Color::WHITE.into(),
                    ..Default::default()
                })
                .size((Sizing::fixed(w), Sizing::fixed(40.0)))
                .show(ui);
        });
    });
}

#[test]
fn static_text_label_alloc_free() {
    audit_steady_state("static_text_label", 0, |ui| {
        Text::new("hello world").auto_id().show(ui);
    });
}

/// A `TextEdit` with a stable buffer must record alloc-free in steady
/// state. Pins the fix that routes the display text through the retained
/// record store (`Ui::intern`) instead of cloning the buffer into a fresh
/// `String` every frame — the latter allocated proportional to buffer
/// length on each record pass.
#[test]
fn text_edit_alloc_free() {
    let mut buf = String::from("the quick brown fox jumps over the lazy dog");
    audit_steady_state("text_edit", 0, move |ui| {
        TextEdit::new(&mut buf)
            .id_salt("edit")
            .size((Sizing::FILL, Sizing::fixed(28.0)))
            .show(ui);
    });
}

#[test]
fn open_context_menu_shortcuts_alloc_free() {
    let trigger_id = WidgetId::from_hash("alloc-context-menu-trigger");
    let mut needs_open = true;
    audit_steady_state("open_context_menu_shortcuts", 0, move |ui| {
        let trigger = Button::new()
            .id(trigger_id)
            .label("Actions")
            .show(ui)
            .snapshot();
        if needs_open {
            ContextMenu::open(ui, trigger_id, Vec2::new(40.0, 40.0));
            needs_open = false;
        }
        ContextMenu::attach(ui, &trigger).show(ui, |ui, popup| {
            MenuItem::new("Copy")
                .shortcut(Shortcut::ctrl('C'))
                .show(ui, popup);
            MenuItem::new("Select all")
                .shortcut(Shortcut::ctrl('A'))
                .show(ui, popup);
        });
    });
}

#[test]
fn long_multiline_selection_alloc_free() {
    let editor_id = WidgetId::from_hash("alloc-long-selection");
    let mut document = "selected line\n".repeat(32);
    audit_text_steady_state("long_multiline_selection", 0, move |ui| {
        ui.request_focus(Some(editor_id));
        TextEdit::new(&mut document)
            .id(editor_id)
            .multiline(true)
            .select_all_on_focus()
            .size((Sizing::fixed(360.0), Sizing::fixed(500.0)))
            .show(ui);
    });
}

#[test]
fn state_map_counter_alloc_free() {
    let id = WidgetId::from_hash("counter");
    audit_steady_state("state_map_counter", 0, move |ui| {
        Frame::new().id_salt("counter").show(ui);
        let n = ui.state_mut::<u32>(id);
        *n = n.wrapping_add(1);
    });
}

/// Scroll w/ overflow: pins `PostArrangeRegistry` typed-bucket reuse + `ScrollHook::run` in-place.
#[test]
fn scroll_overflow_alloc_free() {
    audit_steady_state("scroll_overflow", 0, |ui| {
        Scroll::vertical()
            .id_salt("scroll")
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                Frame::new()
                    .id_salt("tall")
                    .size((Sizing::fixed(180.0), Sizing::fixed(800.0)))
                    .show(ui);
            });
    });
}

/// Scroll w/ content fitting viewport: pins the hook's `overflow == new_overflow` early-exit.
#[test]
fn scroll_fits_alloc_free() {
    audit_steady_state("scroll_fits", 0, |ui| {
        Scroll::vertical()
            .id_salt("scroll")
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                Frame::new()
                    .id_salt("short")
                    .size((Sizing::fixed(180.0), Sizing::fixed(40.0)))
                    .show(ui);
            });
    });
}
