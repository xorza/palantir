//! Keyboard focus: where Tab and Shift+Tab land and in which domain, scopes that keep the press, the focus ring, overlays.

use crate::input::key_class::KeyFilter;
use crate::input::keyboard::key::Key;
use crate::input::keyboard::modifiers::Modifiers;
use crate::input::shortcut::{Shortcut, ShortcutMods};
use crate::internals::harness::UiHarness;
use crate::primitives::identity::widget_id::WidgetId;
use crate::scene::layer::Layer;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widgets::block::Block;
use crate::widgets::panel::Panel;
use glam::UVec2;

const SURFACE: UVec2 = UVec2::new(200, 200);

fn id(name: &str) -> WidgetId {
    WidgetId::from_hash(name)
}

fn stop(ui: &mut Ui, name: &str) {
    Block::new().id(id(name)).focusable(true).show(ui);
}

#[derive(Clone, Copy, Debug)]
struct Overlays {
    popup: bool,
    modal: bool,
    menu: bool,
}

const NONE: Overlays = Overlays {
    popup: false,
    modal: false,
    menu: false,
};

/// `Main` holds, in record order: `a`; `b` at index −1; `c`; a disabled stop; a focusable non-stop; `d`: Tab order
/// `b, a, c, d`. Each overlay holds two stops of its own.
fn scene(overlays: Overlays) -> impl FnMut(&mut Ui) {
    move |ui: &mut Ui| {
        Panel::vstack().auto_id().show(ui, |ui| {
            stop(ui, "a");
            Block::new()
                .id(id("b"))
                .focusable(true)
                .tab_index(-1)
                .show(ui);
            stop(ui, "c");
            Block::new()
                .id(id("disabled"))
                .focusable(true)
                .disabled(true)
                .show(ui);
            Block::new()
                .id(id("no-stop"))
                .focusable(true)
                .tab_stop(false)
                .show(ui);
            stop(ui, "d");
        });
        for (open, layer, names) in [
            (overlays.popup, Layer::Popup, ["p1", "p2"]),
            (overlays.modal, Layer::Modal, ["m1", "m2"]),
            (overlays.menu, Layer::Menu, ["u1", "u2"]),
        ] {
            if open {
                ui.layer(layer).show(|ui| {
                    Panel::vstack().auto_id().show(ui, |ui| {
                        for name in names {
                            stop(ui, name);
                        }
                    });
                });
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum Press {
    Tab(&'static str),
    Back(&'static str),
}

/// One Tab (or Shift+Tab) press and a recorded frame; the trickle queue admits one command key per frame.
fn press(h: &mut UiHarness, back: bool, record: &mut impl FnMut(&mut Ui)) {
    h.set_modifiers(if back {
        Modifiers::SHIFT
    } else {
        Modifiers::NONE
    });
    h.key(Key::Tab);
    h.frame(&mut *record);
}

#[derive(Debug)]
struct Walk {
    label: &'static str,
    overlays: Overlays,
    from: Option<&'static str>,
    presses: &'static [Press],
}

#[test]
fn tab_walks_the_stops_of_its_domain_in_order() {
    use Press::{Back, Tab};
    let walks = [
        Walk {
            label: "Main in index order, then record order, wrapping",
            overlays: NONE,
            from: None,
            presses: &[Tab("b"), Tab("a"), Tab("c"), Tab("d"), Tab("b")],
        },
        Walk {
            label: "Shift+Tab walks back and wraps",
            overlays: NONE,
            from: Some("a"),
            presses: &[Back("b"), Back("d"), Back("c")],
        },
        Walk {
            label: "Shift+Tab from nothing enters at the last",
            overlays: NONE,
            from: None,
            presses: &[Back("d")],
        },
        Walk {
            label: "a popup without focus takes no Tab",
            overlays: Overlays {
                popup: true,
                ..NONE
            },
            from: Some("c"),
            presses: &[Tab("d"), Tab("b")],
        },
        Walk {
            label: "a popup holding focus traps it",
            overlays: Overlays {
                popup: true,
                ..NONE
            },
            from: Some("p1"),
            presses: &[Tab("p2"), Tab("p1"), Back("p2")],
        },
        Walk {
            label: "a modal pulls focus in from behind it",
            overlays: Overlays {
                modal: true,
                ..NONE
            },
            from: Some("a"),
            presses: &[Tab("m1"), Tab("m2"), Tab("m1")],
        },
        Walk {
            label: "a modal outranks a popup holding focus",
            overlays: Overlays {
                popup: true,
                modal: true,
                ..NONE
            },
            from: Some("p1"),
            presses: &[Back("m2")],
        },
        Walk {
            label: "a menu holding focus traps it above a modal",
            overlays: Overlays {
                popup: false,
                modal: true,
                menu: true,
            },
            from: Some("u1"),
            presses: &[Tab("u2"), Tab("u1")],
        },
    ];
    for walk in walks {
        let mut h = UiHarness::new(SURFACE);
        let mut record = scene(walk.overlays);
        h.frame(&mut record);
        if let Some(from) = walk.from {
            h.set_focus(id(from));
        }
        h.frame(&mut record);
        for (step, &press_once) in walk.presses.iter().enumerate() {
            let (back, want) = match press_once {
                Tab(want) => (false, want),
                Back(want) => (true, want),
            };
            press(&mut h, back, &mut record);
            assert_eq!(h.focus(), Some(id(want)), "{}: press {step}", walk.label);
        }
    }
}

/// A scope on the focus path that takes `FOCUS` keeps Tab; one that does not (`ACCEL` alone) leaves it to traversal.
#[test]
fn a_scope_that_takes_focus_keeps_tab() {
    for (filter, keeps) in [
        (KeyFilter::ACCEL | KeyFilter::FOCUS, true),
        (KeyFilter::ACCEL, false),
    ] {
        let mut h = UiHarness::new(SURFACE);
        let mut record = |ui: &mut Ui| {
            Panel::vstack()
                .id(id("root"))
                .input_scope(filter)
                .show(ui, |ui| {
                    stop(ui, "a");
                    stop(ui, "b");
                    ui.key_pressed(Shortcut::new(ShortcutMods::NONE, Key::Tab))
                })
                .inner
        };
        h.frame(|ui| {
            record(ui);
        });
        h.set_focus(id("a"));
        h.frame(|ui| {
            record(ui);
        });
        h.key(Key::Tab);
        let read = h.frame_value(&mut record);
        assert_eq!(read, keeps, "{filter:?}: the root reads Tab");
        let want = if keeps { "a" } else { "b" };
        assert_eq!(h.focus(), Some(id(want)), "{filter:?}: focus");
    }
}

/// The focus ring shows only for keyboard focus (a Tab move, not a pointer press) and never moves the layout.
#[test]
fn the_focus_ring_shows_only_for_keyboard_focus() {
    use crate::input::sense::Sense;
    use crate::primitives::paint::background::Background;
    use crate::primitives::paint::color::RgbaF32;
    use crate::primitives::paint::stroke::Stroke;
    use crate::shape::paint::shape_stroke::ShapeStroke;

    let mut record = |ui: &mut Ui| {
        Panel::vstack().auto_id().show(ui, |ui| {
            Block::new()
                .id(id("a"))
                .size((60.0, 20.0))
                .padding(3.0)
                .sense(Sense::CLICK)
                .focusable(true)
                .background(Background::rounded(RgbaF32::WHITE, 4.0.into()))
                .show(ui);
            Block::new()
                .id(id("b"))
                .size((60.0, 20.0))
                .sense(Sense::CLICK)
                .focusable(true)
                .show(ui);
        });
    };
    let rings = |h: &UiHarness| {
        ["a", "b"].map(|name| {
            let node = h.node_of(id(name)).expect("recorded").node;
            h.ui.tree(Layer::Main)
                .chrome(node)
                .is_some_and(|row| row.ring)
        })
    };
    let padding = |h: &UiHarness| {
        let node = h.node_of(id("a")).expect("recorded").node;
        h.ui.tree(Layer::Main).records.layout()[node.idx()].padding
    };

    let mut h = UiHarness::new(SURFACE);
    h.frame(&mut record);
    let unringed = padding(&h);

    press(&mut h, false, &mut record);
    assert_eq!(rings(&h), [true, false], "Tab rings the stop it reaches");
    let theme = &h.ui.theme().focus_ring;
    assert_eq!(
        h.ui.tree(Layer::Main).focus_ring,
        ShapeStroke::from(Stroke::new(theme.color, theme.width)),
    );
    assert_eq!(padding(&h), unringed, "the ring moves no layout");

    let b = h.center_of(id("b"));
    h.click_at(b);
    h.frame(&mut record);
    assert_eq!(h.focus(), Some(id("b")));
    assert_eq!(
        rings(&h),
        [false, false],
        "a press that moves focus drops the ring"
    );

    press(&mut h, false, &mut record);
    assert_eq!(rings(&h), [true, false], "Tab brings it back");
    h.set_focus(id("b"));
    h.frame(&mut record);
    assert_eq!(
        rings(&h),
        [false, true],
        "set_focus keeps the keyboard's ring"
    );
}

fn dialog_scene(open: bool, stops: bool) -> impl FnMut(&mut Ui) {
    use crate::widgets::modal::Modal;
    move |ui: &mut Ui| {
        Panel::vstack().auto_id().show(ui, |ui| {
            stop(ui, "a");
            stop(ui, "b");
        });
        if open {
            Modal::new().id(id("dialog")).show(ui, |ui, _| {
                if stops {
                    stop(ui, "m1");
                    stop(ui, "m2");
                }
            });
        }
    }
}

/// A dialog takes focus as it appears and returns it to its opener as it closes; focus moved elsewhere while open
/// stays there, and a dialog with no stop moves nothing.
#[test]
fn a_dialog_takes_focus_as_it_opens_and_gives_it_back() {
    let mut h = UiHarness::new(SURFACE);
    h.frame(dialog_scene(false, true));
    h.set_focus(id("a"));
    h.frame(dialog_scene(false, true));

    h.frame(dialog_scene(true, true));
    assert_eq!(
        h.focus(),
        Some(id("m1")),
        "the opening frame moves focus in"
    );
    press(&mut h, false, &mut dialog_scene(true, true));
    assert_eq!(
        h.focus(),
        Some(id("m2")),
        "a later frame does not ask again"
    );
    h.frame(dialog_scene(false, true));
    assert_eq!(h.focus(), Some(id("a")), "closing gives it back");

    h.frame(dialog_scene(true, true));
    h.set_focus(id("b"));
    h.frame(dialog_scene(true, true));
    press(&mut h, false, &mut dialog_scene(true, true));
    assert_eq!(h.focus(), Some(id("m1")));
    h.frame(dialog_scene(false, true));
    assert_eq!(
        h.focus(),
        Some(id("a")),
        "back to the widget it opened from"
    );

    h.set_focus(id("a"));
    h.frame(dialog_scene(true, true));
    assert_eq!(h.focus(), Some(id("m1")));
    h.set_focus(id("b"));
    h.frame(dialog_scene(true, true));
    h.frame(dialog_scene(false, true));
    assert_eq!(
        h.focus(),
        Some(id("b")),
        "a focus moved away is not overridden"
    );

    h.set_focus(id("a"));
    h.frame(dialog_scene(true, false));
    assert_eq!(
        h.focus(),
        Some(id("a")),
        "a dialog without a stop moves nothing"
    );
    h.frame(dialog_scene(false, false));
    assert_eq!(h.focus(), Some(id("a")));
}

/// Arrows along an arrow group walk and wrap its stops; across it, or with a modifier, they move nothing. A scope
/// inside the group that takes arrows keeps them; one on the group's own node (an overlay's claim) does not.
#[test]
fn arrows_walk_an_arrow_group() {
    use crate::primitives::layout::axis::Axis;

    let group = |own_scope: KeyFilter| {
        move |ui: &mut Ui| {
            Panel::vstack()
                .id(id("group"))
                .arrow_focus(Axis::Y)
                .input_scope(own_scope)
                .show(ui, |ui| {
                    stop(ui, "a");
                    stop(ui, "b");
                    Block::new()
                        .id(id("field"))
                        .focusable(true)
                        .input_scope(KeyFilter::CARET)
                        .show(ui);
                });
        }
    };
    let arrow = |h: &mut UiHarness, mods: Modifiers, key: Key, record: &mut dyn FnMut(&mut Ui)| {
        h.set_modifiers(mods);
        h.key(key);
        h.frame(&mut *record);
    };
    for own in [KeyFilter::NONE, KeyFilter::ALL.difference(KeyFilter::FOCUS)] {
        let mut record = group(own);
        let mut h = UiHarness::new(SURFACE);
        h.frame(&mut record);
        h.set_focus(id("a"));
        h.frame(&mut record);
        for (mods, key, want) in [
            (Modifiers::NONE, Key::ArrowDown, "b"),
            (Modifiers::NONE, Key::ArrowRight, "b"),
            (Modifiers::SHIFT, Key::ArrowDown, "b"),
            (Modifiers::NONE, Key::ArrowUp, "a"),
            (Modifiers::NONE, Key::ArrowUp, "field"),
            (Modifiers::NONE, Key::ArrowDown, "field"),
        ] {
            arrow(&mut h, mods, key, &mut record);
            assert_eq!(h.focus(), Some(id(want)), "{own:?}: {mods:?} {key:?}");
        }
    }
}
