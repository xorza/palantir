use crate::harness::audit_steady_state;
use palantir::{
    Background, Button, Color, Configure, Frame, Grid, Panel, Scroll, Sizing, Text, Track, UiCore,
    WidgetId,
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
        fn rec(ui: &mut UiCore, depth: u32) {
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
                .size((Sizing::Fixed(w), Sizing::Fixed(40.0)))
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
                    .size((Sizing::Fixed(180.0), Sizing::Fixed(800.0)))
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
                    .size((Sizing::Fixed(180.0), Sizing::Fixed(40.0)))
                    .show(ui);
            });
    });
}
