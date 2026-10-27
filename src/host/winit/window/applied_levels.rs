//! [`AppliedLevels`]: the window settings the platform holds.

use crate::primitives::geometry::rect::Rect;
use crate::window::cursor_icon::CursorIcon;

/// The cursor and the IME area the native window holds, so each reaches the platform only on a
/// change.
#[derive(Debug, Default)]
pub(super) struct AppliedLevels {
    cursor: CursorIcon,
    /// In physical px, so a scale change moves the candidate list; `None` while IME is off.
    ime_area: Option<Rect>,
}

/// What the platform's input method must do to show an IME area.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum ImeChange {
    /// Turn it on, beside this area.
    Enable(Rect),
    /// Move it to this area.
    Move(Rect),
    /// Turn it off.
    Disable,
}

impl AppliedLevels {
    /// Adopt `cursor` and answer whether the platform must be told.
    pub(super) fn set_cursor(&mut self, cursor: CursorIcon) -> bool {
        let changed = self.cursor != cursor;
        self.cursor = cursor;
        changed
    }

    /// Adopt the physical IME `area` and answer what the platform must do, if anything.
    pub(super) fn set_ime_area(&mut self, area: Option<Rect>) -> Option<ImeChange> {
        if self.ime_area == area {
            return None;
        }
        let change = match area {
            Some(rect) if self.ime_area.is_none() => ImeChange::Enable(rect),
            Some(rect) => ImeChange::Move(rect),
            None => ImeChange::Disable,
        };
        self.ime_area = area;
        Some(change)
    }
}
