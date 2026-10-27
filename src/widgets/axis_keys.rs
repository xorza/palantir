//! Keyboard travel along one axis, shared by every widget whose value the arrow keys move.

use crate::input::key_class::KeyFilter;
use crate::input::keyboard::key::Key;
use crate::input::shortcut::{Shortcut, ShortcutMods};
use crate::ui::Ui;
use crate::widget_core::widget::Widget;

/// The keys that move a value along one axis: step pairs (Shift makes a step ten), an optional
/// page pair and an optional pair that jumps to either end.
#[derive(Clone, Copy, Debug)]
pub(crate) struct AxisKeys {
    pub(crate) steps: &'static [KeyPair],
    pub(crate) page: Option<KeyPair>,
    pub(crate) ends: Option<KeyPair>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct KeyPair {
    pub(crate) back: Key,
    pub(crate) forward: Key,
}

/// What this frame's keys asked of an axis, counted rather than scaled: each widget moves by
/// its own unit, and an integer target adds whole steps without a round trip through `f64`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Travel {
    /// Signed steps, a Shift step counting ten.
    pub(crate) steps: i32,
    /// Signed pages.
    pub(crate) pages: i32,
    /// An end key landed; it replaces any step or page.
    pub(crate) end: Option<AxisEnd>,
}

/// Which end an end key jumps to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AxisEnd {
    Start,
    End,
}

/// Shift steps this many plain steps.
const COARSE: i32 = 10;

/// A step along a unit axis (a color coordinate): half a percent.
pub(crate) const UNIT_STEP: f32 = 0.005;
/// A page along a unit axis: a tenth.
pub(crate) const UNIT_PAGE: f32 = 0.1;

impl AxisKeys {
    /// What a widget walking an axis takes from an enclosing scope: the arrows, Home and End
    /// (`CARET`), and Page Up and Down (`PAGE`).
    pub(crate) const SCOPE: KeyFilter = KeyFilter::CARET.union(KeyFilter::PAGE);

    /// This frame's travel, read through `widget` so its own input scope grants the keys under
    /// an enclosing one. Every chord is sampled, not short-circuited, as reading also keeps it
    /// subscribed for the wake gate. With both end keys pressed, `End` wins.
    pub(crate) fn read(self, widget: &mut Widget, ui: &mut Ui) -> Travel {
        let mut travel = Travel::default();
        for pair in self.steps {
            for (key, sign) in pair.signed() {
                let coarse = widget.shortcut_pressed(ui, Shortcut::new(ShortcutMods::SHIFT, key));
                let plain = widget.shortcut_pressed(ui, key);
                if coarse {
                    travel.steps += sign * COARSE;
                } else if plain {
                    travel.steps += sign;
                }
            }
        }
        if let Some(page) = self.page {
            for (key, sign) in page.signed() {
                if widget.shortcut_pressed(ui, key) {
                    travel.pages += sign;
                }
            }
        }
        if let Some(ends) = self.ends {
            let start = widget.shortcut_pressed(ui, ends.back);
            let end = widget.shortcut_pressed(ui, ends.forward);
            if end {
                travel.end = Some(AxisEnd::End);
            } else if start {
                travel.end = Some(AxisEnd::Start);
            }
        }
        travel
    }
}

impl Travel {
    /// Whether no key moved the axis.
    pub(crate) const fn is_still(self) -> bool {
        self.steps == 0 && self.pages == 0 && self.end.is_none()
    }

    /// Where this travel sends a unit axis now at `at`, `step` and `page` long and ending at 0
    /// and 1. Unclamped: a step past an end is the caller's to clamp or wrap.
    pub(crate) fn on_unit_axis(self, at: f32, step: f32, page: f32) -> f32 {
        match self.end {
            Some(AxisEnd::Start) => 0.0,
            Some(AxisEnd::End) => 1.0,
            None => at + self.steps as f32 * step + self.pages as f32 * page,
        }
    }
}

impl KeyPair {
    const fn signed(self) -> [(Key, i32); 2] {
        [(self.back, -1), (self.forward, 1)]
    }
}
