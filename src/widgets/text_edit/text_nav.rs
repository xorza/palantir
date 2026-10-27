//! [`TextNav`]: the caret chords a field binds, which differ per platform.

use crate::common::platform::Platform;
use crate::input::keyboard::key::Key;
use crate::input::keyboard::key_press::KeyPress;
use crate::input::keyboard::modifiers::Modifiers;

/// The word, line and document chords of one platform's text fields. On
/// macOS `Modifiers::ctrl` is Cmd, the line chord, so the word chord moves to
/// Alt.
#[derive(Clone, Copy, Debug)]
pub(super) struct TextNav(Platform);

impl TextNav {
    pub(super) const fn new(platform: Platform) -> Self {
        Self(platform)
    }

    /// The modifier that turns a horizontal arrow into a word motion, and Backspace or
    /// Delete into a word delete.
    pub(super) const fn word_chord(self) -> Modifiers {
        match self.0 {
            Platform::Mac => Modifiers::ALT,
            Platform::Windows | Platform::Linux => Modifiers::CTRL,
        }
    }

    /// [`Self::word_chord`] held without the other of Ctrl and Alt.
    pub(super) const fn is_word(self, modifiers: Modifiers) -> bool {
        let chord = self.word_chord();
        modifiers.ctrl == chord.ctrl && modifiers.alt == chord.alt
    }

    /// The line chord, macOS only: Cmd with Left / Right or Backspace.
    pub(super) const fn is_line(self, modifiers: Modifiers) -> bool {
        matches!(self.0, Platform::Mac) && modifiers.ctrl && !modifiers.alt
    }

    /// Ctrl with Home / End everywhere, Cmd with Up / Down on macOS.
    pub(super) const fn is_document(self, keypress: KeyPress) -> bool {
        let mods = keypress.mods;
        if !mods.ctrl || mods.alt {
            return false;
        }
        match keypress.key {
            Key::Home | Key::End => true,
            Key::ArrowUp | Key::ArrowDown => matches!(self.0, Platform::Mac),
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::common::platform::Platform;
    use crate::input::keyboard::key::Key;
    use crate::input::keyboard::key_press::KeyPress;
    use crate::input::keyboard::modifiers::Modifiers;
    use crate::widgets::text_edit::text_nav::TextNav;

    /// Each chord on each platform: the rows read word, line, then document
    /// for Mac, Windows and Linux.
    #[test]
    fn every_chord_binds_on_the_platforms_that_own_it() {
        const PLATFORMS: [Platform; 3] = [Platform::Mac, Platform::Windows, Platform::Linux];
        let ctrl_alt = Modifiers {
            alt: true,
            ..Modifiers::CTRL
        };
        let word: [(Modifiers, [bool; 3]); 4] = [
            (Modifiers::NONE, [false, false, false]),
            (Modifiers::CTRL, [false, true, true]),
            (Modifiers::ALT, [true, false, false]),
            (ctrl_alt, [false, false, false]),
        ];
        let line: [(Modifiers, [bool; 3]); 3] = [
            (Modifiers::CTRL, [true, false, false]),
            (Modifiers::ALT, [false, false, false]),
            (ctrl_alt, [false, false, false]),
        ];
        let document: [(KeyPress, [bool; 3]); 6] = [
            (
                KeyPress::with(Key::Home, Modifiers::CTRL),
                [true, true, true],
            ),
            (
                KeyPress::with(Key::End, Modifiers::CTRL),
                [true, true, true],
            ),
            (
                KeyPress::with(Key::ArrowUp, Modifiers::CTRL),
                [true, false, false],
            ),
            (
                KeyPress::with(Key::ArrowDown, Modifiers::CTRL),
                [true, false, false],
            ),
            (KeyPress::with(Key::Home, ctrl_alt), [false, false, false]),
            (
                KeyPress::with(Key::Home, Modifiers::NONE),
                [false, false, false],
            ),
        ];
        for (i, platform) in PLATFORMS.into_iter().enumerate() {
            let nav = TextNav::new(platform);
            for (mods, want) in word {
                assert_eq!(nav.is_word(mods), want[i], "word {mods:?} on {platform:?}");
            }
            for (mods, want) in line {
                assert_eq!(nav.is_line(mods), want[i], "line {mods:?} on {platform:?}");
            }
            for (press, want) in document {
                assert_eq!(
                    nav.is_document(press),
                    want[i],
                    "document {press:?} on {platform:?}"
                );
            }
            assert!(nav.is_word(nav.word_chord()), "{platform:?}");
        }
    }
}
