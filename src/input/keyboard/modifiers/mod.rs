//! Which modifier keys are held: a level the input state carries, not an edge.

use crate::common::platform::Platform;

flag_set! {
    /// Modifier-key state, sent as `InputEvent::ModifiersChanged` when the held set changes.
    /// [`Self::CTRL`] is the primary command modifier, normalized at the input boundary (Cmd on
    /// macOS, Ctrl elsewhere); [`Self::MAC_CTRL`] is the raw macOS Control key, for rare
    /// Mac-specific bindings.
    pub struct Modifiers: u8, packed {
        /// The primary command modifier: Cmd (⌘) on macOS, Ctrl elsewhere.
        const CTRL = 1 << 0;
        /// Shift.
        const SHIFT = 1 << 1;
        /// Alt.
        const ALT = 1 << 2;
        /// The raw macOS Control key; never held off macOS. For Mac-specific bindings.
        const MAC_CTRL = 1 << 3;
        /// The Windows / Super key (W3C `metaKey`). Never held on macOS, where Command lands in
        /// [`Self::CTRL`].
        const META = 1 << 4;
    }
}

impl Modifiers {
    /// True if any command modifier (primary ctrl, alt, raw macOS Control, Super) is held: the "shortcut, not text" predicate. Shift alone doesn't count.
    pub const fn has_command(self) -> bool {
        self.intersects(
            Self::CTRL
                .union(Self::ALT)
                .union(Self::MAC_CTRL)
                .union(Self::META),
        )
    }

    /// Whether a press under these modifiers on `platform` composes text rather than commanding:
    ///
    /// - macOS: Option composes (`@` is Option+L on a German layout), so only Cmd (`CTRL` here) and raw Control command.
    /// - Windows and Linux: Ctrl+Alt is AltGr and composes; Ctrl or Alt alone, or anything with Super, commands.
    pub(crate) const fn compose_text(self, platform: Platform) -> bool {
        if platform.option_composes() {
            !self.intersects(Self::CTRL.union(Self::MAC_CTRL))
        } else {
            self.contains(Self::CTRL) == self.contains(Self::ALT) && !self.contains(Self::META)
        }
    }
}

#[cfg(test)]
mod tests;
