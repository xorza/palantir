//! The rules a player plays an [`Action`] by, written once over a [`Stage`]: a harness on a
//! stepped clock and a live window on real time play the same steps.

use crate::driver::action::Action;
use crate::driver::error::{PlayError, StepError};
use crate::driver::locator::{Locator, Reach};
use crate::input::capture::DRAG_THRESHOLD;
use crate::primitives::input::input_event::InputEvent;
use crate::primitives::input::keyboard::key::Key;
use crate::primitives::input::keyboard::modifiers::Modifiers;
use crate::primitives::input::pointer::PointerButton;
use crate::primitives::input::shortcut::{Shortcut, ShortcutMods};
use glam::Vec2;
use std::fmt::Debug;
use std::time::Duration;

/// How many of its waits for a wake a settle gives a busy app that keeps waking the window, past
/// which it ends with [`PlayError::StillBusy`]: ten minutes at a remote's default 30 s wait, as
/// long as one agent step should hold. Work longer than that is the agent's to wait for with
/// [`Action::Wait`] and settle again. Refused: no bound, a step that hangs while the app reports
/// progress forever.
pub(crate) const BUSY_SETTLE_WAITS: u32 = 20;

/// A window a player acts on: where its locators land, its input door, and its frames.
pub(crate) trait Stage<X> {
    /// The one point `at` names in the last frame, for `reach` ([`Locator::point_for`]).
    fn point(&mut self, at: &Locator<X>, reach: Reach) -> Result<Vec2, PlayError>;

    /// Send `event`, which the next frame plays.
    fn input(&mut self, event: InputEvent<'_>);

    /// One frame that saw every event sent so far.
    fn frame(&mut self) -> Result<(), PlayError>;

    /// Frames until one saw every event sent and asks no repaint.
    fn settle(&mut self) -> Result<(), PlayError>;

    /// Let `dt` pass, frames running as they do.
    fn wait(&mut self, dt: Duration) -> Result<(), PlayError>;

    /// Wait out what is left of the double-click interval since the last press, so the next press
    /// starts a new run of clicks.
    fn space_press(&mut self) -> Result<(), PlayError>;

    /// Take now, a frame after a press, as that press's time.
    fn note_press(&mut self);
}

/// Play `actions` in order on `stage`; the first that fails stops the rest.
pub(crate) fn play_all<X: Debug>(
    stage: &mut impl Stage<X>,
    actions: &[Action<X>],
) -> Result<(), StepError> {
    for (step, action) in actions.iter().enumerate() {
        play(stage, action).map_err(|error| StepError { step, error })?;
    }
    Ok(())
}

/// The frames recorded between the record frame ids `first` and `last`, as a player returns them.
/// A frame that only paints moves no id.
pub(crate) fn frames_recorded(first: u64, last: u64) -> u32 {
    u32::try_from(last.saturating_sub(first)).unwrap_or(u32::MAX)
}

/// Play `action` on `stage` and settle. Every locator resolves before any input goes in, and a
/// frame that fails while a step holds a press, a key or a modifier still lets the step release
/// them, as a remote's frame fails while a native dialog that step opened holds the window's
/// thread, so an error leaves nothing held for the next step. A [`Action::KeyDown`] step that
/// succeeds leaves its key held by intent. A step with no modifiers sends no change of them, so a
/// key a person holds at a live window stays held.
pub(crate) fn play<X: Debug>(
    stage: &mut impl Stage<X>,
    action: &Action<X>,
) -> Result<(), PlayError> {
    match action {
        Action::Hover(at) => {
            let point = stage.point(at, Reach::Hover)?;
            stage.input(InputEvent::PointerMoved(point));
        }
        Action::Click { at, button, mods } => {
            let point = stage.point(at, Reach::Press)?;
            hover(stage, point)?;
            stage.space_press()?;
            let clicked = press_and_release(stage, *button, *mods);
            stage.note_press();
            clicked?;
        }
        Action::DoubleClick(at) => {
            let point = stage.point(at, Reach::Press)?;
            hover(stage, point)?;
            stage.space_press()?;
            let clicked = press_and_release(stage, PointerButton::Left, ShortcutMods::NONE)
                .and_then(|()| press_and_release(stage, PointerButton::Left, ShortcutMods::NONE));
            stage.note_press();
            clicked?;
        }
        Action::Drag {
            from,
            to,
            button,
            mods,
            steps,
        } => {
            let start = stage.point(from, Reach::Press)?;
            let end = stage.point(to, Reach::Hover)?;
            // A shorter move is a click to the window, which would pass with no sign.
            let travel = start.distance(end);
            if travel < DRAG_THRESHOLD {
                return Err(PlayError::ShortDrag { travel });
            }
            hover(stage, start)?;
            stage.space_press()?;
            let released = under_mods(stage, *mods, |stage| {
                stage.input(InputEvent::PointerPressed(*button));
                let moved = drag_moves(stage, start, end, *steps);
                stage.input(InputEvent::PointerReleased(*button));
                moved?;
                stage.frame()
            });
            stage.note_press();
            released?;
        }
        Action::ScrollPixels { at, delta } => {
            let point = stage.point(at, Reach::Hover)?;
            hover(stage, point)?;
            stage.input(InputEvent::ScrollPixels(*delta));
        }
        Action::ScrollLines { at, delta } => {
            let point = stage.point(at, Reach::Hover)?;
            hover(stage, point)?;
            stage.input(InputEvent::ScrollLines(*delta));
        }
        Action::Pinch { at, factor } => {
            let point = stage.point(at, Reach::Hover)?;
            hover(stage, point)?;
            stage.input(InputEvent::Pinch(*factor));
        }
        Action::Key(shortcut) => {
            under_mods(stage, shortcut.mods, |stage| {
                let pressed = key_down(stage, *shortcut);
                // A hand lets go, so no key reads as held after the step.
                stage.input(InputEvent::KeyUp { key: shortcut.key });
                pressed
            })?;
        }
        Action::KeyDown(shortcut) => {
            under_mods(stage, shortcut.mods, |stage| {
                let pressed = key_down(stage, *shortcut);
                if pressed.is_err() {
                    stage.input(InputEvent::KeyUp { key: shortcut.key });
                }
                pressed
            })?;
        }
        Action::KeyUp(shortcut) => {
            under_mods(stage, shortcut.mods, |stage| {
                stage.input(InputEvent::KeyUp { key: shortcut.key });
            });
        }
        Action::Type(text) => {
            for c in text.chars() {
                let key = Key::Char(c);
                stage.input(Shortcut::key(key).key_down());
                stage.input(InputEvent::KeyUp { key });
            }
        }
        Action::InsertText(text) => stage.input(InputEvent::ImeCommit(text)),
        Action::Wait(dt) => stage.wait(*dt)?,
    }
    stage.settle()
}

/// Press `shortcut`'s key and run the frame that reads it held.
fn key_down<X>(stage: &mut impl Stage<X>, shortcut: Shortcut) -> Result<(), PlayError> {
    stage.input(shortcut.key_down());
    stage.frame()
}

/// Run `step` with `mods` held and let them go after it, whatever it returns. With no modifiers it
/// runs under what is held, and no change goes out.
fn under_mods<X, S: Stage<X>, R>(
    stage: &mut S,
    mods: ShortcutMods,
    step: impl FnOnce(&mut S) -> R,
) -> R {
    if mods == ShortcutMods::NONE {
        return step(stage);
    }
    stage.input(InputEvent::ModifiersChanged(mods.held()));
    let result = step(stage);
    stage.input(InputEvent::ModifiersChanged(Modifiers::NONE));
    result
}

/// A held drag's way from `start` to `end`: a frame after the press, then `steps` even moves, a
/// frame each, as a hand moves. The first frame that fails ends it, and no later frame runs.
fn drag_moves<X>(
    stage: &mut impl Stage<X>,
    start: Vec2,
    end: Vec2,
    steps: u32,
) -> Result<(), PlayError> {
    stage.frame()?;
    let steps = steps.max(1);
    for step in 1..=steps {
        let point = start.lerp(end, step as f32 / steps as f32);
        stage.input(InputEvent::PointerMoved(point));
        stage.frame()?;
    }
    Ok(())
}

/// Move the pointer to `point` and settle, so the next press hits what it hovers.
fn hover<X>(stage: &mut impl Stage<X>, point: Vec2) -> Result<(), PlayError> {
    stage.input(InputEvent::PointerMoved(point));
    stage.settle()
}

/// Press and release `button` where the pointer is, with `mods` held, a frame each. A press frame
/// that fails runs no release frame, which on a hung window would wait the whole bound again.
fn press_and_release<X>(
    stage: &mut impl Stage<X>,
    button: PointerButton,
    mods: ShortcutMods,
) -> Result<(), PlayError> {
    under_mods(stage, mods, |stage| {
        stage.input(InputEvent::PointerPressed(button));
        let pressed = stage.frame();
        stage.input(InputEvent::PointerReleased(button));
        pressed?;
        stage.frame()
    })
}
