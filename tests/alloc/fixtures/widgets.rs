use crate::harness::{AllocBudget, audit_steady_state};
use palantir::{
    Background, Button, Color, Configure, Frame, Grid, Panel, Scroll, Sizing, Text, Track, Ui,
    WidgetId,
};
use std::rc::Rc;

#[test]
fn empty_frame_alloc_free() {
    audit_steady_state("empty_frame", AllocBudget::ZERO, |_ui| {});
}

#[test]
fn button_only_alloc_free() {
    audit_steady_state("button_only", AllocBudget::ZERO, |ui| {
        Button::new()
            .auto_id()
            .label("hello")
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui);
    });
}

#[test]
fn nested_vstack_64_alloc_free() {
    audit_steady_state("nested_vstack_64", AllocBudget::ZERO, |ui| {
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
    audit_steady_state("grid_8x8", AllocBudget::ZERO, move |ui| {
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
                                fill: Color::WHITE,
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
    audit_steady_state("damage_animated_rect", AllocBudget::ZERO, move |ui| {
        tick = tick.wrapping_add(1);
        let w = 100.0 + (tick % 200) as f32;
        Panel::vstack().auto_id().show(ui, |ui| {
            Frame::new()
                .auto_id()
                .background(Background {
                    fill: Color::WHITE,
                    ..Default::default()
                })
                .size((Sizing::Fixed(w), Sizing::Fixed(40.0)))
                .show(ui);
        });
    });
}

#[test]
fn static_text_label_alloc_free() {
    audit_steady_state("static_text_label", AllocBudget::ZERO, |ui| {
        Text::new("hello world").auto_id().show(ui);
    });
}

#[test]
fn state_map_counter_alloc_free() {
    let id = WidgetId::from_hash("counter");
    audit_steady_state("state_map_counter", AllocBudget::ZERO, move |ui| {
        Frame::new().id_salt("counter").show(ui);
        let n = ui.state_mut::<u32>(id);
        *n = n.wrapping_add(1);
    });
}

/// Scrollbar with overflowing content. Pins both halves of the
/// scroll-shaped post-arrange path:
///
/// - `PostArrangeRegistry`'s typed-bucket reuse (one `Box::new` for
///   `ScrollHook` ever, none after warmup).
/// - `ScrollHook::run` reading `LayoutResult` + mutating `ScrollState`
///   in place, no per-frame heap touches.
///
/// Cold-mount triggers a relayout (pass A + pass B both run record
/// phase). Two warmup frames absorb that; steady state is back to one
/// pass per frame and zero allocations.
#[test]
fn scroll_overflow_alloc_free() {
    audit_steady_state("scroll_overflow", AllocBudget::ZERO, |ui| {
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

/// Scrollbar with content that fits inside the viewport: no relayout
/// flip after the first measure, no gutter reservation. Pairs with
/// `scroll_overflow_alloc_free` as the negative case — exercises the
/// hook's `overflow == new_overflow` early-exit path.
#[test]
fn scroll_fits_alloc_free() {
    audit_steady_state("scroll_fits", AllocBudget::ZERO, |ui| {
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
