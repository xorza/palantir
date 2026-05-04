use crate::harness::{AllocBudget, audit_until_stable};
use palantir::{Button, Color, Configure, Frame, Grid, Panel, Sizing, Styled, Text, Track, Ui};
use std::rc::Rc;

#[test]
fn empty_frame_alloc_free() {
    audit_until_stable("empty_frame", AllocBudget::ZERO, |_ui| {});
}

#[test]
fn button_only_alloc_free() {
    audit_until_stable("button_only", AllocBudget::ZERO, |ui| {
        Button::new()
            .label("hello")
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui);
    });
}

#[test]
fn nested_vstack_64_alloc_free() {
    audit_until_stable("nested_vstack_64", AllocBudget::ZERO, |ui| {
        fn rec(ui: &mut Ui, depth: u32) {
            if depth == 0 {
                return;
            }
            Panel::vstack_with_id(depth)
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
    audit_until_stable("grid_8x8", AllocBudget::ZERO, move |ui| {
        Grid::new()
            .cols(Rc::clone(&cols))
            .rows(Rc::clone(&rows))
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                for r in 0..8u16 {
                    for c in 0..8u16 {
                        Frame::with_id((r, c))
                            .fill(Color::WHITE)
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
    audit_until_stable("damage_animated_rect", AllocBudget::ZERO, move |ui| {
        tick = tick.wrapping_add(1);
        let w = 100.0 + (tick % 200) as f32;
        Panel::vstack().show(ui, |ui| {
            Frame::new()
                .fill(Color::WHITE)
                .size((Sizing::Fixed(w), Sizing::Fixed(40.0)))
                .show(ui);
        });
    });
}

#[test]
fn static_text_label_alloc_free() {
    audit_until_stable("static_text_label", AllocBudget::ZERO, |ui| {
        Text::new("hello world").show(ui);
    });
}
