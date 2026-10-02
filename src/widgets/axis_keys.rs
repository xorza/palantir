//! Keyboard travel along one unit axis.

use crate::input::keyboard::key::Key;
use crate::input::shortcut::{Shortcut, ShortcutMods};
use crate::ui::Ui;

/// The keys that walk one `0..1` axis: a pair that steps, an optional pair
/// that pages, and an optional pair that jumps to the ends.
///
/// One type for every axis a colour widget drives — the field's two and a
/// bar's one — so the step sizes, the `Shift` multiplier and the rule that
/// every chord is sampled are stated once.
#[derive(Clone, Copy, Debug)]
pub(crate) struct AxisKeys {
    pub(crate) step: KeyPair,
    pub(crate) page: Option<KeyPair>,
    pub(crate) ends: Option<KeyPair>,
}

/// The key toward 0 and the key toward 1.
#[derive(Clone, Copy, Debug)]
pub(crate) struct KeyPair {
    pub(crate) back: Key,
    pub(crate) forward: Key,
}

/// Where this frame's presses send the axis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct AxisTravel {
    /// Unclamped: a step past an end is the caller's to clamp or wrap.
    pub(crate) to: f32,
    /// An end key landed, so `to` is exactly 0 or 1 and no step applies.
    pub(crate) jumped: bool,
}

/// What one press moves, what `Shift` multiplies it by, and what a page
/// key moves.
const STEP: f32 = 0.005;
const COARSE: f32 = 10.0;
const PAGE: f32 = 0.1;

impl AxisKeys {
    /// Where this frame's presses send an axis now at `at`.
    ///
    /// Every chord is sampled rather than short-circuited: `key_pressed` both
    /// reads the press and keeps the chord subscribed for the wake gate, so
    /// one firing must not drop another's subscription that frame.
    pub(crate) fn travel(self, ui: &mut Ui, at: f32) -> AxisTravel {
        let mut to = at;
        for (key, sign) in self.step.signed() {
            let coarse = ui.key_pressed(Shortcut::new(ShortcutMods::SHIFT, key));
            let plain = ui.key_pressed(Shortcut::key(key));
            if coarse {
                to += sign * STEP * COARSE;
            } else if plain {
                to += sign * STEP;
            }
        }
        if let Some(page) = self.page {
            for (key, sign) in page.signed() {
                if ui.key_pressed(Shortcut::key(key)) {
                    to += sign * PAGE;
                }
            }
        }
        let mut jumped = false;
        if let Some(ends) = self.ends {
            let start = ui.key_pressed(Shortcut::key(ends.back));
            let end = ui.key_pressed(Shortcut::key(ends.forward));
            if start || end {
                to = if start { 0.0 } else { 1.0 };
                jumped = true;
            }
        }
        AxisTravel { to, jumped }
    }
}

impl KeyPair {
    const fn signed(self) -> [(Key, f32); 2] {
        [(self.back, -1.0), (self.forward, 1.0)]
    }
}
