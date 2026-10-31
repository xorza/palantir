//! [`AppliedLevels`]: the window settings the platform holds.

use crate::primitives::geometry::rect::Rect;
use crate::theme::color_scheme::ColorScheme;
use crate::window::cursor_icon::CursorIcon;

/// The cursor, the IME area and the decorations' scheme the native window holds, so each reaches
/// the platform only on a change.
#[derive(Debug)]
pub(super) struct AppliedLevels {
    cursor: CursorIcon,
    /// The scheme the title bar and frame were last given; the window opens in it.
    color_scheme: ColorScheme,
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
    /// The levels of a window that opened with its decorations in `color_scheme`.
    pub(super) fn new(color_scheme: ColorScheme) -> Self {
        Self {
            cursor: CursorIcon::default(),
            color_scheme,
            ime_area: None,
        }
    }

    /// Adopt the decorations' `color_scheme` and answer whether the platform must be told.
    pub(super) fn set_color_scheme(&mut self, color_scheme: ColorScheme) -> bool {
        let changed = self.color_scheme != color_scheme;
        self.color_scheme = color_scheme;
        changed
    }

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
