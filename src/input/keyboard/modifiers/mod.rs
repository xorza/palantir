//! Which modifier keys are held: a level the input state carries, not an edge.

use crate::common::platform::Platform;

/// Modifier-key state, sent as `InputEvent::ModifiersChanged` when the held set changes. `ctrl` is the primary command modifier, normalized at the input boundary (Cmd on macOS, Ctrl elsewhere); `mac_ctrl` is the raw macOS Control key, for rare Mac-specific bindings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Modifiers {
    /// The primary command modifier is held: Cmd (⌘) on macOS, Ctrl elsewhere.
    pub ctrl: bool,
    /// Shift is held.
    pub shift: bool,
    /// Alt is held.
    pub alt: bool,
    /// The raw macOS Control key is held; always `false` off macOS. For Mac-specific bindings.
    pub mac_ctrl: bool,
    /// The Windows / Super key is held (W3C `metaKey`; `super` is a Rust keyword). Always `false` on macOS, where Command lands in [`Self::ctrl`].
    pub meta: bool,
}

impl Modifiers {
    /// No modifier.
    pub const NONE: Self = Self {
        ctrl: false,
        shift: false,
        alt: false,
        mac_ctrl: false,
        meta: false,
    };
    /// Shift only.
    pub const SHIFT: Self = Self {
        shift: true,
        ..Self::NONE
    };
    /// Ctrl only.
    pub const CTRL: Self = Self {
        ctrl: true,
        ..Self::NONE
    };
    /// Alt only.
    pub const ALT: Self = Self {
        alt: true,
        ..Self::NONE
    };
    /// Ctrl and Shift.
    pub const CTRL_SHIFT: Self = Self {
        ctrl: true,
        shift: true,
        ..Self::NONE
    };

    /// True if any command modifier (primary ctrl, alt, raw macOS Control, Super) is held: the "shortcut, not text" predicate. Shift alone doesn't count.
    pub const fn has_command(self) -> bool {
        self.ctrl || self.alt || self.mac_ctrl || self.meta
    }

    /// Whether a press under these modifiers on `platform` composes text rather than commanding:
    ///
    /// - macOS: Option composes (`@` is Option+L on a German layout), so only Cmd (`ctrl` here) and raw Control command.
    /// - Windows and Linux: Ctrl+Alt is AltGr and composes; Ctrl or Alt alone, or anything with Super, commands.
    pub(crate) const fn compose_text(self, platform: Platform) -> bool {
        match platform {
            Platform::Mac => !self.ctrl && !self.mac_ctrl,
            Platform::Windows | Platform::Linux => self.ctrl == self.alt && !self.meta,
        }
    }
}

#[cfg(test)]
mod tests;
