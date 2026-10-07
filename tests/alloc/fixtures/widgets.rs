//! One widget or small composition per fixture, each painting the same tree every frame on a strict-zero budget.

use crate::harness::{Audit, new_ui};
use std::time::Duration;

use palantir::{
    Anchor, AnimationSpec, Background, Block, Button, Checkbox, ColorCoords, ColorField,
    ColorPicker, ColorStrip, Configure, ContextMenu, Easing, Expander, ExpanderTheme, Grid,
    MenuItem, Modal, Panel, Popup, ProgressBar, RadioButton, RgbaF32, Scroll, Separator, Shortcut,
    Sizing, Slider, SlotDefaults, Spinner, Splitter, Switch, Text, TextEdit, Tooltip, Track, Ui,
    Vec2, WidgetId,
};

#[test]
fn empty_frame_alloc_free() {
    Audit::new().run(|_ui| {});
}

#[test]
fn button_only_alloc_free() {
    Audit::new().run(|ui| {
        Button::new()
            .auto_id()
            .label("hello")
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui);
    });
}

/// A settled colour field re-paints from its texture without allocating.
#[test]
fn color_field_alloc_free() {
    let mut coords = ColorCoords::default();
    Audit::new().run(|ui| {
        ColorField::new(&mut coords).auto_id().show(ui);
    });
}

/// A hue drag rewrites the field's texture every frame into the handle's buffer; the scene never settles, so the warmup is fixed.
#[test]
fn color_field_hue_drag_alloc_free() {
    let mut coords = ColorCoords::default();
    Audit::new().warmup(4).run(|ui| {
        coords.set_hue(coords.hue() + 0.01);
        ColorField::new(&mut coords).auto_id().show(ui);
    });
}

#[test]
fn color_strip_alloc_free() {
    let mut coords = ColorCoords::default();
    Audit::new().run(|ui| {
        ColorStrip::for_hue(&mut coords).auto_id().show(ui);
    });
}

/// The whole panel, swatch row and hex field included.
#[test]
fn color_picker_alloc_free() {
    let mut color = RgbaF32::hex(0x4cd3ff);
    Audit::new().text().run(|ui| {
        ColorPicker::new(&mut color)
            .alpha(true)
            .history(true)
            .auto_id()
            .show(ui);
    });
}

#[test]
fn nested_vstack_64_alloc_free() {
    Audit::new().run(|ui| {
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
    Audit::new().run(|ui| {
        Grid::new()
            .auto_id()
            .cols([Track::FILL; 8])
            .rows([Track::FILL; 8])
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                for r in 0..8u16 {
                    for c in 0..8u16 {
                        Block::new()
                            .id_salt((r, c))
                            .background(Background {
                                fill: RgbaF32::WHITE.into(),
                                ..Default::default()
                            })
                            .grid_cell((r, c))
                            .show(ui);
                    }
                }
            });
    });
}

/// A settled section, open and closed; a shut section must not keep asking for frames.
#[test]
fn expander_alloc_free() {
    for open in [false, true] {
        Audit::new().run(move |ui| {
            Expander::new("section")
                .auto_id()
                .start_open(open)
                .show(ui, |ui| {
                    Text::new("body").auto_id().show(ui);
                });
        });
    }
}

/// A body kept across a collapse records every frame, which would show a per-frame `Vec`.
#[test]
fn expander_keep_body_alloc_free() {
    Audit::new().run(|ui| {
        Expander::new("section")
            .auto_id()
            .keep_body(true)
            .show(ui, |ui| {
                Text::new("body").auto_id().show(ui);
            });
    });
}

/// Mid-tween, the one path that reads a remembered height and clips the body.
/// Driven frame by frame, not through [`Audit::run`], whose loop holds the clock
/// still; one 60 Hz step per frame (a smaller step carries below the animation
/// substep). The long warmup is the reveal settling: a moving `max_size`
/// invalidates the measure cache each frame until its arenas have grown once.
#[test]
fn expander_mid_reveal_alloc_free() {
    let base = ExpanderTheme::default();
    let theme = ExpanderTheme {
        defaults: SlotDefaults {
            animation: Some(AnimationSpec::duration(
                Duration::from_secs(60),
                Easing::Linear,
            )),
            ..base.defaults
        },
        ..base
    };
    let mut h = new_ui();
    let mut open = true;
    let section = |ui: &mut Ui, open: &mut bool| {
        Expander::new("section")
            .auto_id()
            .style(&theme)
            .open(open)
            .show(ui, |ui| {
                Text::new("body").auto_id().show(ui);
            })
            .openness
    };
    for _ in 0..4 {
        h.frame(|ui| {
            section(ui, &mut open);
        });
    }
    open = false;
    h.frame(|ui| {
        section(ui, &mut open);
    });
    Audit::new().warmup(32).run_frames(|| {
        h.advance(Duration::from_millis(16));
        let mut openness = 0.0;
        h.frame(|ui| openness = section(ui, &mut open));
        assert!(
            openness > 0.0 && openness < 1.0,
            "the frame lands mid-reveal, got openness {openness}",
        );
    });
}

#[test]
fn splitter_alloc_free() {
    let mut ratio = 0.5;
    Audit::new().run(move |ui| {
        Splitter::row(&mut ratio)
            .id_salt("splitter")
            .min_pane(80.0)
            .show(ui, |_, _| {});
    });
}

#[test]
fn damage_animated_rect_alloc_free() {
    let mut tick: u32 = 0;
    Audit::new().run(move |ui| {
        tick = tick.wrapping_add(1);
        let w = 100.0 + (tick % 200) as f32;
        Panel::vstack().auto_id().show(ui, |ui| {
            Block::new()
                .auto_id()
                .background(Background {
                    fill: RgbaF32::WHITE.into(),
                    ..Default::default()
                })
                .size((Sizing::fixed(w), Sizing::fixed(40.0)))
                .show(ui);
        });
    });
}

#[test]
fn static_text_label_alloc_free() {
    Audit::new().run(|ui| {
        Text::new("hello world").auto_id().show(ui);
    });
}

/// A `TextEdit` with a stable buffer records alloc-free: display text goes through `Ui::intern`, not a fresh `String`.
#[test]
fn text_edit_alloc_free() {
    let mut buf = String::from("the quick brown fox jumps over the lazy dog");
    Audit::new().run(move |ui| {
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
    Audit::new().run(move |ui| {
        let trigger = Button::new()
            .id(trigger_id)
            .label("Actions")
            .show(ui)
            .snapshot();
        if needs_open {
            ContextMenu::open(ui, trigger_id, Vec2::new(40.0, 40.0));
            needs_open = false;
        }
        ContextMenu::on(&trigger).show(ui, |ui, popup| {
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
    Audit::new().text().run(move |ui| {
        ui.set_focus(editor_id);
        TextEdit::new(&mut document)
            .id(editor_id)
            .multiline(true)
            .select_all_on_focus(true)
            .size((Sizing::fixed(360.0), Sizing::fixed(500.0)))
            .show(ui);
    });
}

#[test]
fn state_map_counter_alloc_free() {
    let id = WidgetId::from_hash("counter");
    Audit::new().run(move |ui| {
        Block::new().id_salt("counter").show(ui);
        ui.with_state::<u32, _>(id, |_, n| *n = n.wrapping_add(1));
    });
}

/// Scroll with overflow: pins `PostArrangeRegistry` bucket reuse and in-place `ScrollHook::run`.
#[test]
fn scroll_overflow_alloc_free() {
    Audit::new().run(|ui| {
        Scroll::vertical()
            .id_salt("scroll")
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                Block::new()
                    .id_salt("tall")
                    .size((Sizing::fixed(180.0), Sizing::fixed(800.0)))
                    .show(ui);
            });
    });
}

/// Scroll with content fitting the viewport: pins the hook's `overflow == new_overflow` early exit.
#[test]
fn scroll_fits_alloc_free() {
    Audit::new().run(|ui| {
        Scroll::vertical()
            .id_salt("scroll")
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                Block::new()
                    .id_salt("short")
                    .size((Sizing::fixed(180.0), Sizing::fixed(40.0)))
                    .show(ui);
            });
    });
}

/// The value and toggle widgets the frame fixture's tree lacks. The spinner
/// repaints paint-only, so the scene requests a repaint to keep frames recorded.
#[test]
fn value_and_toggle_widgets_alloc_free() {
    let mut on = true;
    let mut choice = 1u8;
    let mut amount = 0.5f64;
    Audit::new().text().run(|ui| {
        ui.request_repaint();
        Panel::vstack()
            .auto_id()
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                Spinner::new().id_salt("spin").show(ui);
                Separator::horizontal().id_salt("sep").show(ui);
                ProgressBar::new(0.42).id_salt("bar").show(ui);
                Slider::new(&mut amount, 0.0..=1.0)
                    .id_salt("slide")
                    .show(ui);
                Switch::new(&mut on).id_salt("switch").show(ui);
                Checkbox::new(&mut on).id_salt("check").show(ui);
                RadioButton::new(&mut choice, 1u8).id_salt("radio").show(ui);
            });
    });
}

/// A tooltip held over its trigger; it records only while up, which each frame asserts.
#[test]
fn tooltip_bubble_alloc_free() {
    let host = WidgetId::from_hash("tip-host");
    let scene = |ui: &mut Ui| {
        let trigger = Button::new().id(host).label("hover").show(ui).snapshot();
        Tooltip::on(&trigger, "a tooltip body")
            .delay(Duration::ZERO)
            .show(ui)
    };
    let mut h = new_ui();
    let _ = h.frame(|ui| {
        scene(ui);
    });
    h.move_onto(host);
    Audit::new().run_frames(|| {
        let mut visible = false;
        let _ = h.frame(|ui| visible = scene(ui).visible);
        assert!(visible, "the bubble is up");
    });
}

/// A spinner left alone only repaints, so this measures the paint-only frame.
#[test]
fn spinner_paint_only_alloc_free() {
    Audit::new().paint_only().run(|ui| {
        Spinner::new().id_salt("spin").show(ui);
    });
}

/// The two side-layer overlays, held open; warmed and measured in whole ring revolutions.
#[test]
fn overlays_alloc_free() {
    Audit::new().text().run(|ui| {
        Panel::vstack()
            .auto_id()
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                Popup::new(Anchor::at_point(Vec2::new(40.0, 40.0)))
                    .id_salt("pop")
                    .show(ui, |ui, _handle| {
                        Text::new("popup body").id_salt("pop-text").show(ui);
                    });
                Modal::new().id_salt("modal").show(ui, |ui, _| {
                    Text::new("modal body").id_salt("modal-text").show(ui);
                });
            });
    });
}
