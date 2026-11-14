//! [`Action`]: one step a person takes at a window, which a harness or a live window's remote
//! plays.

use crate::driver::locator::Locator;
use crate::primitives::input::pointer::PointerButton;
use crate::primitives::input::shortcut::{Shortcut, ShortcutMods};
use crate::primitives::input::zoom_factor::ZoomFactor;
use glam::Vec2;
use std::convert::Infallible;
use std::num::NonZeroU32;
use std::time::Duration;

/// One step of input, as a person at the window takes it. A player runs frames until the window
/// settles after each, so the next step reads what this one changed. A step changes the modifiers
/// only when it names some, and lets them all go at its end; one that names none leaves held what
/// is held.
#[derive(Clone, Debug, PartialEq)]
pub enum Action<X = Infallible> {
    /// Move the pointer there.
    Hover(Locator<X>),
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
        /// The moves from `from` to `to`.
        steps: NonZeroU32,
    },
    /// Scroll there by `delta` logical pixels, as a touchpad does; positive `y` scrolls content
    /// down.
    ScrollPixels {
        /// Where the pointer rests while it scrolls.
        at: Locator<X>,
        /// The scroll, in logical pixels.
        delta: Vec2,
    },
    /// Scroll there by `delta` wheel notches, as a mouse wheel does; positive `y` scrolls content
    /// down.
    ScrollLines {
        /// Where the pointer rests while it scrolls.
        at: Locator<X>,
        /// The scroll, in notches.
        delta: Vec2,
    },
    /// Pinch there by `factor`: above 1 zooms in.
    Pinch {
        /// Where the pointer rests while it pinches.
        at: Locator<X>,
        /// The zoom step.
        factor: ZoomFactor,
    },
    /// Press and release one key with its modifiers, as `ctrl+z` or `enter`.
    Key(Shortcut),
    /// Press one key with its modifiers and leave the key down until a [`Self::KeyUp`] step, for
    /// an app that reads [`Ui::key_held`](crate::Ui::key_held). The modifiers come up at the end
    /// of the step.
    KeyDown(Shortcut),
    /// Release a key a [`Self::KeyDown`] step left down, with its modifiers held for the release.
    KeyUp(Shortcut),
    /// Type text as key taps, one a character, each typing it.
    Type(String),
    /// Commit text to whatever holds the keyboard focus, as an input method commits it: no key
    /// event.
    InsertText(String),
    /// Let this much time pass, frames running, as a person waits for an animation.
    Wait(Duration),
}

/// Shorthands for the steps of a window an app adds no places to, so `Action::click("Open")`
/// needs no type named; an app's own places build the variants.
impl Action {
    /// The moves a drag takes when its caller names none: enough to cross any drag threshold in
    /// small steps.
    pub const DRAG_STEPS: NonZeroU32 = NonZeroU32::new(8).unwrap();

    /// A left click with no modifiers at `at`.
    pub fn click(at: impl Into<Locator>) -> Self {
        Self::Click {
            at: at.into(),
            button: PointerButton::Left,
            mods: ShortcutMods::NONE,
        }
    }
}
