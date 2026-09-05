//! Arrow-key travel along one unit axis.

use crate::input::keyboard::key::Key;
use crate::input::shortcut::{Mods, Shortcut};
use crate::ui::Ui;

/// The two arrow keys that walk one `0..1` axis.
///
/// One type for every axis a colour widget drives — the field's two and a
/// bar's one — so the step, the `Shift` multiplier and the rule that every
/// chord is sampled are stated once.
#[derive(Clone, Copy, Debug)]
pub(crate) struct AxisKeys {
    pub(crate) back: Key,
    pub(crate) forward: Key,
}

/// What one press moves, and what `Shift` multiplies it by.
const STEP: f32 = 0.005;
const COARSE: f32 = 10.0;

impl AxisKeys {
    /// Signed travel this frame's presses ask for; zero when none landed.
    ///
    /// Every chord is sampled rather than short-circuited: `key_pressed` both
    /// reads the press and keeps the chord subscribed for the wake gate, so
    /// one firing must not drop another's subscription that frame.
    pub(crate) fn travel(self, ui: &mut Ui) -> f32 {
        let mut travel = 0.0;
        for (key, sign) in [(self.back, -1.0), (self.forward, 1.0)] {
            let coarse = ui.key_pressed(Shortcut::new(Mods::SHIFT, key));
            let plain = ui.key_pressed(Shortcut::key(key));
            if coarse {
                travel += sign * STEP * COARSE;
            } else if plain {
                travel += sign * STEP;
            }
        }
        travel
    }
}
