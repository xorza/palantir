//! Keyboard travel along one unit axis.

use crate::input::keyboard::key::Key;
use crate::input::shortcut::{Shortcut, ShortcutMods};
use crate::ui::Ui;

/// The keys that walk one `0..1` axis: a step pair, optional page pair and optional end-jump pair.
#[derive(Clone, Copy, Debug)]
pub(crate) struct AxisKeys {
    pub(crate) step: KeyPair,
    pub(crate) page: Option<KeyPair>,
    pub(crate) ends: Option<KeyPair>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct KeyPair {
    pub(crate) back: Key,
    pub(crate) forward: Key,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct AxisTravel {
    /// Unclamped: a step past an end is the caller's to clamp or wrap.
    pub(crate) to: f32,
    /// An end key landed, so `to` is exactly 0 or 1 and no step applies.
    pub(crate) jumped: bool,
}

const STEP: f32 = 0.005;
const COARSE: f32 = 10.0;
const PAGE: f32 = 0.1;

impl AxisKeys {
    /// Where this frame's presses send an axis now at `at`. Every chord is sampled so `key_pressed` subscribes each for the wake gate.
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
