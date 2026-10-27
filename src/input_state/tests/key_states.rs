//! A key's three states, whatever the modifiers, beside the exact chord.

use crate::input::input_event::InputEvent;
use crate::input::keyboard::key::Key;
use crate::input::keyboard::key_text::KeyText;
use crate::input::keyboard::modifiers::Modifiers;
use crate::input::shortcut::Shortcut;
use crate::internals::ui_harness::UiHarness;
use crate::ui::Ui;
use glam::UVec2;

/// What a frame reads of `key`: pressed, held, released.
fn states(h: &mut UiHarness, key: Key) -> [bool; 3] {
    h.frame_value(|ui: &mut Ui| [ui.key_pressed(key), ui.key_held(key), ui.key_released(key)])
}

/// Ctrl+Esc reads as Esc pressed and held, and not as the bare chord; the next frame reads it
/// held alone; its release reads released alone, once.
#[test]
fn a_key_reads_pressed_held_and_released_whatever_the_modifiers() {
    let mut h = UiHarness::new(UVec2::new(64, 48));
    states(&mut h, Key::Escape);
    h.set_modifiers(Modifiers::CTRL);
    h.key(Key::Escape);
    let chord = h.frame_value(|ui: &mut Ui| {
        [
            ui.key_pressed(Key::Escape),
            ui.key_held(Key::Escape),
            ui.shortcut_pressed(Key::Escape),
            ui.shortcut_pressed(Shortcut::ctrl('a')),
        ]
    });
    assert_eq!(chord, [true, true, false, false]);
    assert_eq!(states(&mut h, Key::Escape), [false, true, false]);
    h.set_modifiers(Modifiers::NONE);
    assert_eq!(
        states(&mut h, Key::Escape),
        [false, true, false],
        "through a modifier change"
    );
    h.key_up(Key::Escape);
    assert_eq!(states(&mut h, Key::Escape), [false, false, true]);
    assert_eq!(states(&mut h, Key::Escape), [false, false, false]);
}

/// A tap whose press and release share a frame reads pressed and released, not held; a repeat
/// holds nothing more, so one release ends the hold; two physical keys of one logical key, as
/// both Enter keys are, hold until both come up; a second press of one physical key is a lost
/// release and holds nothing more; a release of a key never down reads nothing.
#[test]
fn a_hold_counts_each_press_once() {
    let mut h = UiHarness::new(UVec2::new(64, 48));
    let space = Key::Char(' ');
    states(&mut h, space);
    h.key(space);
    h.key_up(space);
    assert_eq!(states(&mut h, space), [true, false, true], "a tap");

    h.key(space);
    h.on_input(InputEvent::KeyDown {
        key: space,
        repeat: true,
        physical: Key::Other,
        text: KeyText::of_key(space),
    });
    states(&mut h, space);
    h.key_up(space);
    assert_eq!(states(&mut h, space), [false, false, true], "a repeat");

    h.key(Key::Enter);
    h.on_input(InputEvent::KeyDown {
        key: Key::Enter,
        repeat: false,
        physical: Key::Enter,
        text: KeyText::EMPTY,
    });
    states(&mut h, Key::Enter);
    h.key_up(Key::Enter);
    assert_eq!(
        states(&mut h, Key::Enter),
        [false, true, true],
        "one Enter still down"
    );
    h.key_up(Key::Enter);
    assert_eq!(states(&mut h, Key::Enter), [false, false, true]);

    h.key(Key::F2);
    h.key(Key::F2);
    states(&mut h, Key::F2);
    h.key_up(Key::F2);
    assert_eq!(
        states(&mut h, Key::F2),
        [false, false, true],
        "a lost release"
    );

    h.key_up(Key::Tab);
    assert_eq!(
        states(&mut h, Key::Tab),
        [false, false, false],
        "never down"
    );
}

/// Shift turns a letter's press into its capital, and the key still reads as the letter, as a
/// chord's letter does.
#[test]
fn a_letter_reads_whatever_shift_does_to_it() {
    let mut h = UiHarness::new(UVec2::new(64, 48));
    let w = Key::Char('w');
    states(&mut h, w);
    h.set_modifiers(Modifiers::SHIFT);
    h.key(Key::Char('W'));
    assert_eq!(states(&mut h, w), [true, true, false]);
    h.set_modifiers(Modifiers::NONE);
    h.key_up(Key::Char('W'));
    assert_eq!(states(&mut h, w), [false, false, true]);
}

/// The surface losing the keyboard ends every hold, with its release edge.
#[test]
fn focus_loss_releases_every_held_key() {
    let mut h = UiHarness::new(UVec2::new(64, 48));
    let both = |h: &mut UiHarness| {
        h.frame_value(|ui: &mut Ui| {
            [
                ui.key_held(Key::Char('w')),
                ui.key_released(Key::Char('w')),
                ui.key_held(Key::ArrowUp),
                ui.key_released(Key::ArrowUp),
            ]
        })
    };
    both(&mut h);
    h.key(Key::Char('w'));
    h.key(Key::ArrowUp);
    assert_eq!(both(&mut h), [true, false, true, false]);
    h.on_input(InputEvent::SurfaceFocusLost);
    assert_eq!(both(&mut h), [false, true, false, true]);
}

/// A reader of a key wakes a frame on its press and its release, with nothing focused; another
/// key's release wakes nothing.
#[test]
fn a_read_key_wakes_on_its_press_and_release() {
    let mut h = UiHarness::new(UVec2::new(64, 48));
    let space = Key::Char(' ');
    states(&mut h, space);
    assert!(h.key(space).repaint_requested, "the press");
    states(&mut h, space);
    assert!(h.key_up(space).repaint_requested, "the release");
    states(&mut h, space);
    h.key(Key::Char('q'));
    states(&mut h, space);
    assert!(
        !h.key_up(Key::Char('q')).repaint_requested,
        "a key no one reads"
    );
}
