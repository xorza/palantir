//! Keyboard shortcuts: one value drives both display ("Ctrl+C") and matching
//! against [`KeyPress`] events.
//!
//! - The primary command modifier (`ShortcutMods::CTRL`) is **Cmd on macOS, Ctrl
//!   elsewhere**, so one binding fires on ⌘S and Ctrl+S; for raw Ctrl on macOS
//!   match a `KeyPress` directly.
//! - The Super modifier (`ShortcutMods::META`) is the Windows / Super key, and **Cmd
//!   on macOS**, where that key is Command: there it is the same key as the primary
//!   modifier, as Electron's `Super` and VS Code's `meta` read it.
//! - [`Shortcut::matches`] compares modifiers *exactly* (Ctrl+A does not match
//!   Ctrl+Shift+A); `Char` keys compare ignore-case since [`Key::Char`] arrives
//!   post-shift-layout.
//!
//! [`KeyPress`]: crate::KeyPress

use crate::common::platform::{PLATFORM, Platform};
use crate::primitives::input::input_event::InputEvent;
use crate::primitives::input::keyboard::key::Key;
use crate::primitives::input::keyboard::key_press::KeyPress;
use crate::primitives::input::keyboard::key_text::KeyText;
use crate::primitives::input::keyboard::modifiers::Modifiers;
use crate::primitives::input::shortcut::error::ParseShortcutError;
use std::fmt::{self, Write as _};
use std::str::FromStr;

pub(crate) mod error;

flag_set! {
    /// Modifier set for declaring shortcuts: [`Self::CTRL`] is the primary command key (Cmd on
    /// macOS, Ctrl elsewhere), [`Self::SHIFT`] and [`Self::ALT`] are literal. Distinct from
    /// event-state [`Modifiers`], which also carries [`Modifiers::MAC_CTRL`]: a held macOS
    /// Control is ignored by a chord declaring a command modifier and rejects every other, as
    /// [`Modifiers::has_command`] classes it.
    pub struct ShortcutMods: u8 {
        /// The primary command key: Cmd on macOS, Ctrl on Windows and Linux.
        const CTRL = 1 << 0;
        /// Shift, literally.
        const SHIFT = 1 << 1;
        /// Alt / Option, literally.
        const ALT = 1 << 2;
        /// The Windows / Super key; on macOS, where that key is Command, the same key as
        /// [`Self::CTRL`].
        const META = 1 << 4;
    }
}

impl ShortcutMods {
    /// True if this chord declares any command modifier, as
    /// [`Modifiers::has_command`](crate::Modifiers::has_command) asks of what is
    /// *held*. Shift alone does not count (`Shift+Z` is a capital Z). No
    /// `MAC_CTRL`: a chord is declared once for every platform.
    pub const fn has_command(self) -> bool {
        self.intersects(Self::CTRL.union(Self::ALT).union(Self::META))
    }

    /// The chord modifiers of a held set. Lossy, so not a `From`: raw macOS Control has no twin,
    /// and [`Shortcut::matches`] reads it off the press. The two sets share their bits, and a
    /// test holds each flag to its twin.
    const fn from_modifiers(m: Modifiers) -> Self {
        Self(m.difference(Modifiers::MAC_CTRL).bits())
    }

    /// The set a press holds to fire this chord on `platform`: on macOS the host reports
    /// Command as `CTRL`, so a declared `META` folds into it.
    const fn held_on(self, platform: Platform) -> Self {
        if platform.commands_with_super() && self.contains(Self::META) {
            self.difference(Self::META).union(Self::CTRL)
        } else {
            self
        }
    }

    /// The modifiers a person holds on this platform to press this chord, as a host reports
    /// them.
    pub(crate) const fn held(self) -> Modifiers {
        Modifiers::from_bits_truncate(self.held_on(PLATFORM).0)
    }
}

/// A keyboard shortcut: modifier set + key; the `const fn` constructors let
/// bindings live in `const` items.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Shortcut {
    /// Modifier set, matched **exactly**.
    pub mods: ShortcutMods,
    /// The key. `Char` compares ignore-case.
    pub key: Key,
}

impl Shortcut {
    /// Any modifier set plus any key; the other constructors are shorthands.
    pub const fn new(mods: ShortcutMods, key: Key) -> Self {
        Self { mods, key }
    }

    /// Bare key, no modifiers, e.g. `Shortcut::key(Key::Escape)`.
    pub const fn key(key: Key) -> Self {
        Self::new(ShortcutMods::NONE, key)
    }

    /// `Ctrl+<c>`. Matching ignores ASCII case, and the label shows an ASCII letter
    /// uppercase.
    pub const fn ctrl(c: char) -> Self {
        Self::new(ShortcutMods::CTRL, Key::Char(c))
    }

    /// True iff `press` matches. Modifiers compare exactly, `Char` keys
    /// ignore-case, `repeat` is ignored, and a held macOS Control rejects a
    /// shortcut with no command modifier.
    ///
    /// Non-Latin fallback: a command chord's letter arrives as the *active
    /// layout's* character (a non-Latin letter for the physical Z), which never matches
    /// ASCII. For a **non-ASCII** `Char` with a command modifier, retry against the
    /// layout-independent [physical key] ([`KeyPress::physical`]); Dvorak / AZERTY
    /// still produce ASCII and are untouched.
    pub fn matches(self, press: KeyPress) -> bool {
        self.matches_on(press, PLATFORM)
    }

    /// [`Self::matches`] on `platform`, so one host can check every platform's rule.
    fn matches_on(self, press: KeyPress, platform: Platform) -> bool {
        let chord = Self {
            mods: self.mods.held_on(platform),
            ..self
        };
        if press.mods.contains(Modifiers::MAC_CTRL) && !chord.mods.has_command() {
            return false;
        }
        if chord.matches_key(press.key, press.mods) {
            return true;
        }
        chord.mods.has_command()
            && press
                .layout_retry()
                .is_some_and(|physical| chord.matches_key(physical, press.mods))
    }

    /// The label `platform` shows. Takes the platform, as [`Modifiers::compose_text`]
    /// does, so one host can check every platform's label.
    fn label(self, platform: Platform) -> impl fmt::Display {
        ShortcutLabel {
            shortcut: self,
            platform,
        }
    }

    /// The event a press of this shortcut sends from a keyboard with no physical key, typing
    /// [`Self::typed`]; the modifiers go in their own event before it.
    pub(crate) fn key_down(self) -> InputEvent<'static> {
        InputEvent::KeyDown {
            key: self.key,
            repeat: false,
            physical: Key::Other,
            text: self.typed(),
        }
    }

    /// What a press of this shortcut types, as a layout gives it: its character, upper case with
    /// Shift, and nothing for a named key or with a command modifier held.
    pub(crate) fn typed(self) -> KeyText {
        match self.key {
            Key::Char(_) if self.mods.has_command() => KeyText::EMPTY,
            Key::Char(c) if self.mods.contains(ShortcutMods::SHIFT) => {
                KeyText::new(&c.to_uppercase().to_string())
            }
            key => KeyText::of_key(key),
        }
    }

    /// Logical-key match with **no** layout fallback; [`Self::matches`] layers that
    /// on.
    fn matches_key(self, key: Key, mods: Modifiers) -> bool {
        ShortcutMods::from_modifiers(mods) == self.mods && self.key.same_key(key)
    }
}

/// The bare key, no modifiers: `ui.shortcut_pressed(Key::Delete)` is
/// `ui.shortcut_pressed(Shortcut::key(Key::Delete))`.
impl From<Key> for Shortcut {
    fn from(key: Key) -> Self {
        Self::key(key)
    }
}

/// Modifiers and one key joined by `+`, case-blind, as a Windows or Linux label writes them:
/// `ctrl+shift+z`, `Ctrl+PgUp`, `alt+f4`, `ctrl++` for the plus key. `ctrl` and `cmd` are the
/// primary command key, `meta`, `super` and `win` the Super key. A key is a named key or one
/// character, which a shortcut holds in ASCII lower case, as its matching ignores the case.
impl FromStr for Shortcut {
    type Err = ParseShortcutError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        if text.trim() == "+" {
            return Ok(Self::key(Key::Char('+')));
        }
        // `ctrl++` names the plus key: the last `+` after a separator is the key itself.
        let (mods_text, key_text) = match text.strip_suffix("++") {
            Some(mods) => (Some(mods), "+"),
            None => match text.rsplit_once('+') {
                Some((mods, key)) => (Some(mods), key),
                None => (None, text),
            },
        };
        let mut mods = ShortcutMods::NONE;
        for named in mods_text.into_iter().flat_map(|mods| mods.split('+')) {
            match named.trim().to_ascii_lowercase().as_str() {
                "ctrl" | "cmd" => mods.insert(ShortcutMods::CTRL),
                "shift" => mods.insert(ShortcutMods::SHIFT),
                "alt" => mods.insert(ShortcutMods::ALT),
                "meta" | "super" | "win" => mods.insert(ShortcutMods::META),
                _ => return Err(ParseShortcutError::Modifier),
            }
        }
        Ok(Self::new(mods, parse_key(key_text.trim())?))
    }
}

/// How a shortcut names a key with a word or a glyph rather than its character: what its label
/// writes, on macOS and elsewhere, and the other words the parse accepts for it.
#[derive(Debug)]
struct KeyName {
    label: &'static str,
    mac_label: &'static str,
    aliases: &'static [&'static str],
}

impl KeyName {
    const fn new(label: &'static str) -> Self {
        Self {
            label,
            mac_label: label,
            aliases: &[],
        }
    }

    const fn mac_label(self, mac_label: &'static str) -> Self {
        Self { mac_label, ..self }
    }

    const fn aliases(self, aliases: &'static [&'static str]) -> Self {
        Self { aliases, ..self }
    }

    /// The name of `key`, `None` for a printing character, which labels as itself. Space is a
    /// word on every platform, as a macOS menu writes `⌃Space`. A key named here joins
    /// [`NAMED_KEYS`], so its label parses back.
    const fn of(key: Key) -> Option<Self> {
        Some(match key {
            Key::ArrowLeft => Self::new("←").aliases(&["left"]),
            Key::ArrowRight => Self::new("→").aliases(&["right"]),
            Key::ArrowUp => Self::new("↑").aliases(&["up"]),
            Key::ArrowDown => Self::new("↓").aliases(&["down"]),
            Key::Backspace => Self::new("Backspace").mac_label("⌫"),
            Key::Delete => Self::new("Delete").mac_label("⌦").aliases(&["del"]),
            Key::Home => Self::new("Home"),
            Key::End => Self::new("End"),
            Key::PageUp => Self::new("PgUp").aliases(&["pageup"]),
            Key::PageDown => Self::new("PgDn").aliases(&["pagedown"]),
            Key::Enter => Self::new("Enter").mac_label("⏎").aliases(&["return"]),
            Key::Tab => Self::new("Tab").mac_label("⇥"),
            Key::Escape => Self::new("Esc").aliases(&["escape"]),
            Key::Char(' ') => Self::new("Space"),
            Key::F1 => Self::new("F1"),
            Key::F2 => Self::new("F2"),
            Key::F3 => Self::new("F3"),
            Key::F4 => Self::new("F4"),
            Key::F5 => Self::new("F5"),
            Key::F6 => Self::new("F6"),
            Key::F7 => Self::new("F7"),
            Key::F8 => Self::new("F8"),
            Key::F9 => Self::new("F9"),
            Key::F10 => Self::new("F10"),
            Key::F11 => Self::new("F11"),
            Key::F12 => Self::new("F12"),
            Key::Other => Self::new("Unknown"),
            Key::Char(_) => return None,
        })
    }
}

/// Every key [`KeyName::of`] names, which the parse reads its names from.
const NAMED_KEYS: [Key; 27] = [
    Key::ArrowLeft,
    Key::ArrowRight,
    Key::ArrowUp,
    Key::ArrowDown,
    Key::Backspace,
    Key::Delete,
    Key::Home,
    Key::End,
    Key::PageUp,
    Key::PageDown,
    Key::Enter,
    Key::Tab,
    Key::Escape,
    Key::Char(' '),
    Key::F1,
    Key::F2,
    Key::F3,
    Key::F4,
    Key::F5,
    Key::F6,
    Key::F7,
    Key::F8,
    Key::F9,
    Key::F10,
    Key::F11,
    Key::F12,
    Key::Other,
];

/// A named key as its Windows or Linux label or an alias writes it, case-blind, or one
/// character. `Key::Other`'s label names no key a press can match, so it does not parse.
fn parse_key(text: &str) -> Result<Key, ParseShortcutError> {
    if text.is_empty() {
        return Err(ParseShortcutError::NoKey);
    }
    let named = NAMED_KEYS.into_iter().find(|&key| {
        KeyName::of(key).is_some_and(|name| {
            name.label.eq_ignore_ascii_case(text)
                || name
                    .aliases
                    .iter()
                    .any(|alias| alias.eq_ignore_ascii_case(text))
        })
    });
    match named {
        Some(Key::Other) => Err(ParseShortcutError::Key),
        Some(key) => Ok(key),
        None => {
            let mut chars = text.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) => Ok(Key::Char(c.to_ascii_lowercase())),
                _ => Err(ParseShortcutError::Key),
            }
        }
    }
}

/// Platform-native label for the platform this build targets: macOS glyphs (`⌥⇧⌘<key>`), else
/// `Ctrl+Shift+Alt+<key>`.
impl fmt::Display for Shortcut {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.label(PLATFORM).fmt(f)
    }
}

/// What [`Shortcut::label`] returns.
#[derive(Clone, Copy, Debug)]
struct ShortcutLabel {
    shortcut: Shortcut,
    platform: Platform,
}

impl fmt::Display for ShortcutLabel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Shortcut { mods, key } = self.shortcut;
        let mods = mods.held_on(self.platform);
        if self.platform.labels_with_symbols() {
            // Canonical macOS order ⌥ ⇧ ⌘ <key>; the primary modifier is Cmd there,
            // so it sits last.
            if mods.contains(ShortcutMods::ALT) {
                f.write_str("⌥")?;
            }
            if mods.contains(ShortcutMods::SHIFT) {
                f.write_str("⇧")?;
            }
            if mods.contains(ShortcutMods::CTRL) {
                f.write_str("⌘")?;
            }
            return write_key(f, key, self.platform);
        }
        let mut first = true;
        let sep = |f: &mut fmt::Formatter<'_>, first: &mut bool| -> fmt::Result {
            if !*first {
                f.write_str("+")?;
            }
            *first = false;
            Ok(())
        };
        if mods.contains(ShortcutMods::CTRL) {
            sep(f, &mut first)?;
            f.write_str("Ctrl")?;
        }
        if mods.contains(ShortcutMods::SHIFT) {
            sep(f, &mut first)?;
            f.write_str("Shift")?;
        }
        if mods.contains(ShortcutMods::ALT) {
            sep(f, &mut first)?;
            f.write_str("Alt")?;
        }
        if mods.contains(ShortcutMods::META) {
            sep(f, &mut first)?;
            f.write_str(self.platform.super_label())?;
        }
        sep(f, &mut first)?;
        write_key(f, key, self.platform)
    }
}

fn write_key(f: &mut fmt::Formatter<'_>, key: Key, platform: Platform) -> fmt::Result {
    match (KeyName::of(key), key) {
        (Some(name), _) if platform.labels_with_symbols() => f.write_str(name.mac_label),
        (Some(name), _) => f.write_str(name.label),
        (None, Key::Char(c)) => f.write_char(c.to_ascii_uppercase()),
        (None, key) => unreachable!("only a printing character has no name: {key:?}"),
    }
}

#[cfg(test)]
mod tests;
