//! One widget or small composition per fixture, each painting the same tree every frame on a strict-zero budget.

use crate::harness;
use crate::harness::Audit;
use std::rc::Rc;
use std::time::Duration;

use palantir::glam::Vec2;
use palantir::{
    Anchor, AnimationSpec, Block, Button, Checkbox, Chrome, ColorCoords, ColorField, ColorPicker,
    ColorScheme, Configure, Easing, Expander, ExpanderTheme, Menu, MenuItem, Modal, Panel, Popup,
    ProgressBar, RadioButton, RgbaF32, Scroll, Separator, Shortcut, Sizing, Slider, SlotDefaults,
    Spinner, SwatchRow, Switch, Text, TextEdit, Theme, Tooltip, Ui, WidgetId,
};

#[test]
fn empty_frame_alloc_free() {
    Audit::new().run(|_ui| {});
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

/// The whole panel: field, both strips, swatch row and hex field.
#[test]
fn color_picker_alloc_free() {
    let mut color = RgbaF32::hex(0x4cd3ff);
    Audit::new().text().run(|ui| {
        ColorPicker::new(&mut color)
            .alpha(true)
            .swatches(SwatchRow::History)
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

/// A settled section, open, closed, and closed with its body kept: a shut section must not keep asking for frames, and a kept body records every frame, which would show a per-frame `Vec`.
#[test]
fn expander_alloc_free() {
    for (open, keep_body) in [(false, false), (true, false), (false, true)] {
        Audit::new().run(move |ui| {
            Expander::new("section")
                .auto_id()
                .start_open(open)
                .keep_body(keep_body)
                .show(ui, |ui| {
                    Text::new("body").auto_id().show(ui);
                });
        });
    }
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
            animation: AnimationSpec::duration(Duration::from_secs(60), Easing::Linear),
            ..base.defaults
        },
        ..base
    };
    let mut h = harness::new_ui();
    let id = WidgetId::from_hash("section");
    let section = |ui: &mut Ui| {
        Expander::new("section")
            .id(id)
            .start_open(true)
            .style(&theme)
            .show(ui, |ui| {
                Text::new("body").auto_id().show(ui);
            })
            .openness
    };
    for _ in 0..4 {
        h.frame(|ui| {
            section(ui);
        });
    }
    h.frame(|ui| {
        Expander::close(ui, id);
        section(ui);
    });
    Audit::new().warmup(32).run_frames(|| {
        h.advance(Duration::from_millis(16));
        let mut openness = 0.0;
        h.frame(|ui| openness = section(ui));
        assert!(
            openness > 0.0 && openness < 1.0,
            "the frame lands mid-reveal, got openness {openness}",
        );
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
                .chrome(Chrome::fill(RgbaF32::WHITE))
                .size((Sizing::fixed(w), Sizing::fixed(40.0)))
                .show(ui);
        });
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
            Menu::open(ui, trigger_id, Vec2::new(40.0, 40.0));
            needs_open = false;
        }
        Menu::context(&trigger).show(ui, |ui, _| {
            MenuItem::new("Copy").shortcut(Shortcut::ctrl('C')).show(ui);
            MenuItem::new("Select all")
                .shortcut(Shortcut::ctrl('A'))
                .show(ui);
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

/// A settled lend takes the stored row back and builds no default: `Heaped`'s `Default`
/// allocates, so a default built per lend would fail the audit.
#[test]
fn state_map_counter_alloc_free() {
    #[derive(Debug)]
    struct Heaped(Vec<u32>);
    impl Default for Heaped {
        fn default() -> Self {
            Self(Vec::with_capacity(4))
        }
    }

    let id = WidgetId::from_hash("counter");
    let heaped = WidgetId::from_hash("heaped");
    Audit::new().run(move |ui| {
        Block::new().id_salt("counter").show(ui);
        ui.with_state::<u32, _>(id, |_, n| *n = n.wrapping_add(1));
        ui.with_state::<Heaped, _>(heaped, |_, h| h.0.clear());
    });
}

/// A scroll whose content overflows the viewport, and one whose content fits it.
#[test]
fn scroll_alloc_free() {
    for height in [800.0, 40.0] {
        Audit::new().run(move |ui| {
            Scroll::vertical()
                .id_salt("scroll")
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    Block::new()
                        .id_salt("content")
                        .size((Sizing::fixed(180.0), Sizing::fixed(height)))
                        .show(ui);
                });
        });
    }
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
    let mut h = harness::new_ui();
    let _ = h.frame(|ui| {
        scene(ui);
    });
    h.hover(host);
    Audit::new().run_frames(|| {
        let mut visible = false;
        let _ = h.frame(|ui| visible = scene(ui).visible);
        assert!(visible, "the bubble is up");
    });
}

/// A theme transition blends into buffers it keeps, so past the window's first runs nothing
/// allocates, also when the app holds every theme it sets. Each 64-frame cycle sets a theme four
/// times against the 300 ms default's 19 frames of 16 ms: at 0 the light one, at 4 back to the dark
/// one it left, a turn, at 8 a third one while that turn runs, which starts a run from the blend
/// shown and lands about frame 27, and at 40 the dark one from rest, which lands before the cycle
/// ends.
#[test]
fn theme_transition_alloc_free() {
    let themes = [
        Rc::new(Theme::stock(ColorScheme::Dark)),
        Rc::new(Theme::stock(ColorScheme::Light)),
        Rc::new(Theme::stock(ColorScheme::Light)),
    ];
    let mut h = harness::new_ui();
    h.ui_mut()
        .set_theme_for(ColorScheme::Dark, Rc::clone(&themes[0]));
    let mut scene = |ui: &mut Ui| {
        Button::new().id_salt("apply").label("Apply").show(ui);
        Checkbox::new(&mut true)
            .label("Check")
            .id_salt("check")
            .show(ui);
        Text::new("label").id_salt("label").show(ui);
    };
    h.prime(2, &mut scene);
    let mut frame = 0usize;
    let (mut blends, mut rests) = (0usize, 0usize);
    Audit::new().warmup(64).frames(128).run_frames(|| {
        let set = match frame % 64 {
            0 => Some(1),
            4 | 40 => Some(0),
            8 => Some(2),
            _ => None,
        };
        if let Some(set) = set {
            h.ui_mut()
                .set_theme_for(ColorScheme::Dark, Rc::clone(&themes[set]));
        }
        frame += 1;
        h.advance(Duration::from_millis(16));
        h.frame(&mut scene);
        let at_rest = themes.iter().any(|theme| Rc::ptr_eq(theme, h.ui().theme()));
        if at_rest {
            rests += 1;
        } else {
            blends += 1;
        }
    });
    assert!(blends > 0 && rests > 0, "{blends} blends, {rests} rests");
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
                Popup::new()
                    .anchor(Anchor::at_point(Vec2::new(40.0, 40.0)))
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
