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
    /// Either Shift key is held.
    pub shift: bool,
    /// The **primary command modifier** is held — Cmd (⌘) on macOS, Ctrl
    /// on Windows and Linux. Normalized at the input boundary, so
    /// consumers never branch on platform.
    pub ctrl: bool,
    /// Either Alt / Option key is held.
    pub alt: bool,
    /// The raw macOS Control key is held. Always `false` off macOS, where
    /// the physical Ctrl is the primary and lands in [`Self::ctrl`]. Only
    /// for Mac-specific bindings; most code should ignore it.
    pub mac_ctrl: bool,
}

impl Modifiers {
    /// Nothing held.
    pub const NONE: Self = Self {
        shift: false,
        ctrl: false,
        alt: false,
        mac_ctrl: false,
    };

    /// True if any command modifier (primary ctrl, alt, or raw macOS
    /// Control) is held — the canonical "this is a shortcut, not text"
    /// predicate. Shift alone doesn't count (shift+letter is just the
    /// capitalized letter).
    pub const fn any_command(self) -> bool {
        self.ctrl || self.alt || self.mac_ctrl
    }

    /// Whether a press under these modifiers on `platform` composes text
    /// rather than commanding — the platform's own rule, which the host
    /// does not restate:
    ///
    /// - macOS: Option composes (`@` is Option+L on a German layout), so
    ///   only Cmd (`ctrl` here) and raw Control make a command.
    /// - Windows and Linux: Ctrl+Alt is AltGr, which composes; Ctrl alone
    ///   or Alt alone is a command (a shortcut, a menu mnemonic).
    pub(crate) const fn compose_text(self, platform: Platform) -> bool {
        match platform {
            Platform::Mac => !self.ctrl && !self.mac_ctrl,
            Platform::Win | Platform::Linux => self.ctrl == self.alt,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::common::platform::Platform;
    use crate::input::keyboard::modifiers::Modifiers;

    #[test]
    fn any_command_excludes_shift() {
        assert!(
            !Modifiers {
                shift: true,
                ..Modifiers::NONE
            }
            .any_command()
        );
        assert!(
            Modifiers {
                ctrl: true,
                ..Modifiers::NONE
            }
            .any_command()
        );
        assert!(
            Modifiers {
                alt: true,
                ..Modifiers::NONE
            }
            .any_command()
        );
    }

    /// Every modifier combination on every platform, hand-derived from the
    /// platform rules: (shift, ctrl, alt, mac_ctrl) → composes on
    /// (Mac, Win, Linux).
    #[test]
    fn compose_text_follows_each_platforms_rule() {
        let rows = [
            // Bare and shifted keys type everywhere.
            ((false, false, false, false), [true, true, true]),
            ((true, false, false, false), [true, true, true]),
            // Ctrl alone (Cmd on macOS) commands everywhere.
            ((false, true, false, false), [false, false, false]),
            // Alt alone: Option composes on macOS, a mnemonic elsewhere.
            ((false, false, true, false), [true, false, false]),
            ((true, false, true, false), [true, false, false]),
            // Ctrl+Alt: Cmd+Option commands on macOS, AltGr composes elsewhere.
            ((false, true, true, false), [false, true, true]),
            // Raw Control on macOS commands.
            ((false, false, false, true), [false, true, true]),
        ];
        for ((shift, ctrl, alt, mac_ctrl), expected) in rows {
            let mods = Modifiers {
                shift,
                ctrl,
                alt,
                mac_ctrl,
            };
            let got = [Platform::Mac, Platform::Win, Platform::Linux].map(|p| mods.compose_text(p));
            assert_eq!(got, expected, "{mods:?}");
        }
    }
}
