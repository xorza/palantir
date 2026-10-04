//! Which modifier keys are held, as a level the input state carries
//! between events rather than an edge.

use crate::common::platform::Platform;

/// Modifier-key state. Sent as a standalone `InputEvent::ModifiersChanged`
/// whenever the held set changes; widgets read the latest snapshot from the
/// input state.
///
/// `ctrl` is the **primary command modifier**, already normalized at
/// the input boundary: it's the Cmd (⌘)
/// key on macOS and the physical Ctrl key on Windows/Linux. Consumers
/// never disambiguate platforms for normal shortcuts — there's one
/// command bit.
///
/// `mac_ctrl` is the **raw macOS Control key**, surfaced separately
/// for the rare Mac-specific binding (Ctrl-click → context menu,
/// emacs-style Ctrl-A in a field). It's only ever set on macOS; on
/// Windows/Linux the physical Ctrl *is* the primary, so it lands in
/// `ctrl` and `mac_ctrl` stays `false`. Most code should ignore it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Modifiers {
    /// The **primary command modifier** is held — Cmd (⌘) on macOS, Ctrl
    /// on Windows and Linux. Normalized at the input boundary, so
    /// consumers never branch on platform.
    pub ctrl: bool,
    /// Either Shift key is held.
    pub shift: bool,
    /// Either Alt / Option key is held.
    pub alt: bool,
    /// The raw macOS Control key is held. Always `false` off macOS, where
    /// the physical Ctrl is the primary and lands in [`Self::ctrl`]. Only
    /// for Mac-specific bindings; most code should ignore it.
    pub mac_ctrl: bool,
    /// The Windows / Super key is held — the W3C `KeyboardEvent.metaKey`
    /// name, since `super` is a Rust keyword. Always `false` on macOS, where
    /// Command is the primary and lands in [`Self::ctrl`].
    pub meta: bool,
}

impl Modifiers {
    /// Nothing held.
    pub const NONE: Self = Self {
        ctrl: false,
        shift: false,
        alt: false,
        mac_ctrl: false,
        meta: false,
    };
    /// Shift alone.
    pub const SHIFT: Self = Self {
        shift: true,
        ..Self::NONE
    };
    /// The primary command key alone.
    pub const CTRL: Self = Self {
        ctrl: true,
        ..Self::NONE
    };
    /// Alt / Option alone.
    pub const ALT: Self = Self {
        alt: true,
        ..Self::NONE
    };
    /// The primary command key plus Shift.
    pub const CTRL_SHIFT: Self = Self {
        ctrl: true,
        shift: true,
        ..Self::NONE
    };

    /// True if any command modifier (primary ctrl, alt, raw macOS Control,
    /// or the Windows / Super key) is held — the canonical "this is a
    /// shortcut, not text" predicate. Shift alone doesn't count
    /// (shift+letter is just the capitalized letter).
    pub const fn has_command(self) -> bool {
        self.ctrl || self.alt || self.mac_ctrl || self.meta
    }

    /// Whether a press under these modifiers on `platform` composes text
    /// rather than commanding — the platform's own rule, which the host
    /// does not restate:
    ///
    /// - macOS: Option composes (`@` is Option+L on a German layout), so
    ///   only Cmd (`ctrl` here) and raw Control make a command.
    /// - Windows and Linux: Ctrl+Alt is AltGr, which composes; Ctrl alone
    ///   or Alt alone is a command (a shortcut, a menu mnemonic), and so is
    ///   anything with the Windows / Super key.
    pub(crate) const fn compose_text(self, platform: Platform) -> bool {
        match platform {
            Platform::Mac => !self.ctrl && !self.mac_ctrl,
            Platform::Win | Platform::Linux => self.ctrl == self.alt && !self.meta,
        }
    }
}

#[cfg(test)]
mod tests;
