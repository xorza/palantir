//! The rules a player plays an [`Action`] by, written once over a [`Stage`]: a harness on a
//! stepped clock and a live window on real time play the same steps.

use crate::driver::action::Action;
use crate::driver::error::PlayError;
use crate::driver::locator::Locator;
use crate::input::input_event::InputEvent;
use crate::input::keyboard::key::Key;
use crate::input::keyboard::modifiers::Modifiers;
use crate::input::pointer::PointerButton;
use crate::input::shortcut::ShortcutMods;
use glam::Vec2;
use std::fmt::Debug;
use std::time::Duration;

/// A window a player acts on: where its locators land, its input door, and its frames.
pub(crate) trait Stage<X> {
    /// The one point `at` names in the last frame.
    fn point(&mut self, at: &Locator<X>) -> Result<Vec2, PlayError>;

    /// Send `event`, which the next frame plays.
    fn input(&mut self, event: InputEvent<'_>);

    /// One frame that saw every event sent so far.
    fn frame(&mut self) -> Result<(), PlayError>;

    /// Frames until one saw every event sent and asks no repaint, naming `step` when none does.
    fn settle(&mut self, step: &dyn Debug) -> Result<(), PlayError>;

    /// Let `dt` pass, frames running as they do.
    fn wait(&mut self, dt: Duration) -> Result<(), PlayError>;

    /// Wait out what is left of the double-click window since the last press, so the next press
    /// starts a new run of clicks.
    fn space_press(&mut self) -> Result<(), PlayError>;

    /// Take now, a frame after a press, as that press's time.
    fn note_press(&mut self) -> Result<(), PlayError>;
}

/// Play `action` on `stage` and settle. Every locator resolves before any input goes in, and
/// nothing between a press or a modifier and its release can fail but a closed window, which
/// releases them anyway, so an error leaves nothing held for the next step.
pub(crate) fn play<X: Debug>(
    stage: &mut impl Stage<X>,
    action: &Action<X>,
) -> Result<(), PlayError> {
    match action {
        Action::Move(at) => {
            let point = stage.point(at)?;
            stage.input(InputEvent::PointerMoved(point));
        }
        Action::Click { at, button, mods } => {
            let point = stage.point(at)?;
            hover(stage, point, action)?;
            stage.space_press()?;
            press_and_release(stage, *button, *mods)?;
            stage.note_press()?;
        }
        Action::DoubleClick(at) => {
            let point = stage.point(at)?;
            hover(stage, point, action)?;
            stage.space_press()?;
            for _ in 0..2 {
                press_and_release(stage, PointerButton::Left, ShortcutMods::NONE)?;
            }
            stage.note_press()?;
        }
        Action::Drag {
            from,
            to,
            button,
            mods,
            steps,
        } => {
            let start = stage.point(from)?;
            let end = stage.point(to)?;
            hover(stage, start, action)?;
            stage.space_press()?;
            stage.input(InputEvent::ModifiersChanged(mods.held()));
            stage.input(InputEvent::PointerPressed(*button));
            // A frame between the moves, as a hand moves.
            stage.frame()?;
            let steps = (*steps).max(1);
            for step in 1..=steps {
                let point = start.lerp(end, step as f32 / steps as f32);
                stage.input(InputEvent::PointerMoved(point));
                stage.frame()?;
            }
            stage.input(InputEvent::PointerReleased(*button));
            stage.frame()?;
            stage.input(InputEvent::ModifiersChanged(Modifiers::NONE));
            stage.note_press()?;
        }
        Action::Scroll { at, delta } => {
            let point = stage.point(at)?;
            hover(stage, point, action)?;
            stage.input(InputEvent::ScrollPixels(*delta));
        }
        Action::Pinch { at, factor } => {
            // The window drops a zoom that is not a positive factor, which would pass with no
            // sign.
            if !(factor.is_finite() && *factor > 0.0) {
                return Err(PlayError::BadPinch { factor: *factor });
            }
            let point = stage.point(at)?;
            hover(stage, point, action)?;
            stage.input(InputEvent::Zoom(*factor));
        }
        Action::Key(shortcut) => {
            stage.input(InputEvent::ModifiersChanged(shortcut.mods.held()));
            stage.input(InputEvent::KeyDown {
                key: shortcut.key,
                repeat: false,
                physical: Key::Other,
                text: shortcut.typed(),
            });
            stage.frame()?;
            // A hand lets go, so no key reads as held after the step.
            stage.input(InputEvent::KeyUp { key: shortcut.key });
            stage.input(InputEvent::ModifiersChanged(Modifiers::NONE));
        }
        Action::Type(text) => stage.input(InputEvent::ImeCommit(text)),
        Action::Wait(dt) => stage.wait(*dt)?,
    }
    stage.settle(action)
}

/// Move the pointer to `point` and settle, so the next press hits what it hovers.
fn hover<X: Debug>(
    stage: &mut impl Stage<X>,
    point: Vec2,
    action: &Action<X>,
) -> Result<(), PlayError> {
    stage.input(InputEvent::PointerMoved(point));
    stage.settle(action)
}

/// Press and release `button` where the pointer is, with `mods` held, a frame each.
fn press_and_release<X>(
    stage: &mut impl Stage<X>,
    button: PointerButton,
    mods: ShortcutMods,
) -> Result<(), PlayError> {
    stage.input(InputEvent::ModifiersChanged(mods.held()));
    stage.input(InputEvent::PointerPressed(button));
    let pressed = stage.frame();
    stage.input(InputEvent::PointerReleased(button));
    let released = stage.frame();
    stage.input(InputEvent::ModifiersChanged(Modifiers::NONE));
    pressed.and(released)
}
