//! [`Action`]: one step a person takes at a window, which a harness or a live window's remote
//! plays.

use crate::driver::locator::Locator;
use crate::input::pointer::PointerButton;
use crate::input::shortcut::{Shortcut, ShortcutMods};
use glam::Vec2;
use std::convert::Infallible;
use std::time::Duration;

/// One step of input, as a person at the window takes it. A player runs frames until the window
/// settles after each, so the next step reads what this one changed.
#[derive(Clone, Debug, PartialEq)]
pub enum Action<X = Infallible> {
    /// Move the pointer there, as a hover.
    Move(Locator<X>),
    /// Press and release `button` there, holding `mods`.
    Click {
        /// Where the press lands.
        at: Locator<X>,
        /// The button pressed.
        button: PointerButton,
        /// The modifiers held through the press and the release.
        mods: ShortcutMods,
    },
    /// Two left clicks there, close enough in time and place to make one double click.
    DoubleClick(Locator<X>),
    /// Press `button` at `from`, move to `to` in `steps` even moves, a frame each, and release
    /// there.
    Drag {
        /// Where the press lands.
        from: Locator<X>,
        /// Where the release lands.
        to: Locator<X>,
        /// The button held.
        button: PointerButton,
        /// The modifiers held through the drag.
        mods: ShortcutMods,
        /// The moves from `from` to `to`; 0 moves as 1.
        steps: u32,
    },
    /// Scroll there by `delta` logical pixels; positive `y` scrolls content down.
    Scroll {
        /// Where the pointer rests while it scrolls.
        at: Locator<X>,
        /// The scroll, in logical pixels.
        delta: Vec2,
    },
    /// Pinch there by `factor`: above 1 zooms in.
    Pinch {
        /// Where the pointer rests while it pinches.
        at: Locator<X>,
        /// The zoom step, finite and above 0.
        factor: f32,
    },
    /// Press and release one key with its modifiers, as `ctrl+z` or `enter`.
    Key(Shortcut),
    /// Type text into whatever holds the keyboard focus, as an input method commits it.
    Type(String),
    /// Let this much time pass, frames running, as a person waits for an animation.
    Wait(Duration),
}

/// Shorthands for the steps of a window an app adds no places to, so `Action::click("Open")`
/// needs no type named; an app's own places build the variants.
impl Action {
    /// The moves a drag takes when its caller names none: enough to cross any drag threshold in
    /// small steps.
    pub const DRAG_STEPS: u32 = 8;

    /// A left click with no modifiers at `at`.
    pub fn click(at: impl Into<Locator>) -> Self {
        Self::Click {
            at: at.into(),
            button: PointerButton::Left,
            mods: ShortcutMods::NONE,
        }
    }

    /// A press of `shortcut`.
    pub const fn key(shortcut: Shortcut) -> Self {
        Self::Key(shortcut)
    }
}
