//! Keyboard shortcuts: one value drives both display ("Ctrl+C") and matching
//! against [`KeyPress`] events.
//!
//! - The primary command modifier (`ShortcutMods::ctrl`) is **Cmd on macOS, Ctrl
//!   elsewhere**, so one binding fires on ⌘S and Ctrl+S; for raw Ctrl on macOS
//!   match a `KeyPress` directly.
//! - The Super modifier (`ShortcutMods::meta`) is the Windows / Super key, and **Cmd
//!   on macOS**, where that key is Command: there it is the same key as the primary
//!   modifier, as Electron's `Super` and VS Code's `meta` read it.
//! - [`Shortcut::matches`] compares modifiers *exactly* (Ctrl+A does not match
//!   Ctrl+Shift+A); `Char` keys compare ignore-case since [`Key::Char`] arrives
//!   post-shift-layout.
//!
//! [`KeyPress`]: crate::KeyPress

use crate::common::platform::{PLATFORM, Platform};
use crate::input::keyboard::key::Key;
use crate::input::keyboard::key_press::KeyPress;
use crate::input::keyboard::key_text::KeyText;
use crate::input::keyboard::modifiers::Modifiers;
use crate::input::shortcut::error::ParseShortcutError;
use std::fmt::{self, Write as _};
use std::str::FromStr;

pub(crate) mod error;

/// Modifier set for declaring shortcuts: `ctrl` is the primary command key (Cmd on
/// macOS, Ctrl elsewhere), `shift` and `alt` are literal. Distinct from event-state
/// [`Modifiers`], which also carries `mac_ctrl`: a held macOS Control is ignored by
/// a chord declaring a command modifier and rejects every other, as
/// [`Modifiers::has_command`] classes it.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct ShortcutMods {
    /// The primary command key: Cmd on macOS, Ctrl on Windows and Linux.
    pub ctrl: bool,
    /// Shift, literally.
    pub shift: bool,
    /// Alt / Option, literally.
    pub alt: bool,
    /// The Windows / Super key; on macOS, where that key is Command, the same key as
    /// [`Self::ctrl`].
    pub meta: bool,
}

impl ShortcutMods {
    /// True if this chord declares any command modifier, as
    /// [`Modifiers::has_command`](crate::Modifiers::has_command) asks of what is
    /// *held*. Shift alone does not count (`Shift+Z` is a capital Z). No
    /// `mac_ctrl`: a chord is declared once for every platform.
    pub const fn has_command(self) -> bool {
        self.ctrl || self.alt || self.meta
    }

    /// The chord modifiers of a held set. Lossy, so not a `From`: raw macOS Control has no twin,
    /// and [`Shortcut::matches`] reads it off the press.
    const fn from_modifiers(m: Modifiers) -> Self {
        // Exhaustive, so a new `Modifiers` field is a compile error here.
        let Modifiers {
            ctrl,
            shift,
            alt,
            mac_ctrl: _,
            meta,
        } = m;
        Self {
            ctrl,
            shift,
            alt,
            meta,
        }
    }

    /// The set a press holds to fire this chord on `platform`: on macOS the host reports
    /// Command as `ctrl`, so a declared `meta` folds into it.
    const fn held_on(self, platform: Platform) -> Self {
        match platform {
            Platform::Mac => Self {
                ctrl: self.ctrl || self.meta,
                meta: false,
                ..self
            },
            Platform::Windows | Platform::Linux => self,
        }
    }

    /// The modifiers a person holds on this platform to press this chord, as a host reports
    /// them.
    pub(crate) const fn held(self) -> Modifiers {
        let Self {
            ctrl,
            shift,
            alt,
            meta,
        } = self.held_on(PLATFORM);
        Modifiers {
            ctrl,
            shift,
            alt,
            mac_ctrl: false,
            meta,
        }
    }

    /// No modifiers: a bare key.
    pub const NONE: Self = Self {
        ctrl: false,
        shift: false,
        alt: false,
        meta: false,
    };
    /// Shift alone.
    pub const SHIFT: Self = Self {
        ctrl: false,
        shift: true,
        alt: false,
        meta: false,
    };
    /// Primary command key alone.
    pub const CTRL: Self = Self {
        ctrl: true,
        shift: false,
        alt: false,
        meta: false,
    };
    /// Alt / Option alone.
    pub const ALT: Self = Self {
        ctrl: false,
        shift: false,
        alt: true,
        meta: false,
    };
    /// Primary command key plus Shift.
    pub const CTRL_SHIFT: Self = Self {
        ctrl: true,
        shift: true,
        alt: false,
        meta: false,
    };
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

    /// `Ctrl+Shift+<c>`; same casing as [`Self::ctrl`].
    pub const fn ctrl_shift(c: char) -> Self {
        Self::new(ShortcutMods::CTRL_SHIFT, Key::Char(c))
    }

    /// True iff `press` matches. Modifiers compare exactly, `Char` keys
    /// ignore-case, `repeat` is ignored, and a held macOS Control rejects a
    /// shortcut with no command modifier.
    ///
    /// Non-Latin fallback: a command chord's letter arrives as the *active
    /// layout's* character (Cyrillic `'я'` for the physical Z), which never matches
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
        if press.mods.mac_ctrl && !chord.mods.has_command() {
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

    /// What a press of this shortcut types, as a layout gives it: its character, upper case with
    /// Shift, and nothing for a named key or with a command modifier held.
    pub(crate) fn typed(self) -> KeyText {
        match self.key {
            Key::Char(_) if self.mods.has_command() => KeyText::EMPTY,
            Key::Char(c) if self.mods.shift => KeyText::new(&c.to_uppercase().to_string()),
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
/// `ui.shortcut_pressed(Key::Delete)`.
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
                "ctrl" | "cmd" => mods.ctrl = true,
                "shift" => mods.shift = true,
                "alt" => mods.alt = true,
                "meta" | "super" | "win" => mods.meta = true,
                _ => return Err(ParseShortcutError::Modifier),
            }
        }
        Ok(Self::new(mods, parse_key(key_text.trim())?))
    }
}

/// A named key, as a lower-case word or a Windows or Linux label writes it, or one character.
fn parse_key(text: &str) -> Result<Key, ParseShortcutError> {
    Ok(match text.to_ascii_lowercase().as_str() {
        "" => return Err(ParseShortcutError::NoKey),
        "enter" | "return" => Key::Enter,
        "escape" | "esc" => Key::Escape,
        "tab" => Key::Tab,
        "backspace" => Key::Backspace,
        "delete" | "del" => Key::Delete,
        "left" | "←" => Key::ArrowLeft,
        "right" | "→" => Key::ArrowRight,
        "up" | "↑" => Key::ArrowUp,
        "down" | "↓" => Key::ArrowDown,
        "home" => Key::Home,
        "end" => Key::End,
        "pageup" | "pgup" => Key::PageUp,
        "pagedown" | "pgdn" => Key::PageDown,
        "space" => Key::Char(' '),
        "f1" => Key::F1,
        "f2" => Key::F2,
        "f3" => Key::F3,
        "f4" => Key::F4,
        "f5" => Key::F5,
        "f6" => Key::F6,
        "f7" => Key::F7,
        "f8" => Key::F8,
        "f9" => Key::F9,
        "f10" => Key::F10,
        "f11" => Key::F11,
        "f12" => Key::F12,
        _ => {
            let mut chars = text.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) => Key::Char(c.to_ascii_lowercase()),
                _ => return Err(ParseShortcutError::Key),
            }
        }
    })
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
        if matches!(self.platform, Platform::Mac) {
            // Canonical macOS order ⌥ ⇧ ⌘ <key>; the primary modifier is Cmd there,
            // so it sits last.
            if mods.alt {
                f.write_str("⌥")?;
            }
            if mods.shift {
                f.write_str("⇧")?;
            }
            if mods.ctrl {
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
        if mods.ctrl {
            sep(f, &mut first)?;
            f.write_str("Ctrl")?;
        }
        if mods.shift {
            sep(f, &mut first)?;
            f.write_str("Shift")?;
        }
        if mods.alt {
            sep(f, &mut first)?;
            f.write_str("Alt")?;
        }
        if mods.meta {
            sep(f, &mut first)?;
            f.write_str(if matches!(self.platform, Platform::Windows) {
                "Win"
            } else {
                "Super"
            })?;
        }
        sep(f, &mut first)?;
        write_key(f, key, self.platform)
    }
}

fn write_key(f: &mut fmt::Formatter<'_>, key: Key, platform: Platform) -> fmt::Result {
    let mac = matches!(platform, Platform::Mac);
    match key {
        Key::Char(c) => f.write_char(c.to_ascii_uppercase()),
        Key::ArrowLeft => f.write_str("←"),
        Key::ArrowRight => f.write_str("→"),
        Key::ArrowUp => f.write_str("↑"),
        Key::ArrowDown => f.write_str("↓"),
        Key::Backspace => f.write_str(if mac { "⌫" } else { "Backspace" }),
        Key::Delete => f.write_str(if mac { "⌦" } else { "Delete" }),
        Key::Home => f.write_str("Home"),
        Key::End => f.write_str("End"),
        Key::PageUp => f.write_str("PgUp"),
        Key::PageDown => f.write_str("PgDn"),
        Key::Enter => f.write_str(if mac { "⏎" } else { "Enter" }),
        Key::Tab => f.write_str(if mac { "⇥" } else { "Tab" }),
        Key::Escape => f.write_str("Esc"),
        Key::F1 => f.write_str("F1"),
        Key::F2 => f.write_str("F2"),
        Key::F3 => f.write_str("F3"),
        Key::F4 => f.write_str("F4"),
        Key::F5 => f.write_str("F5"),
        Key::F6 => f.write_str("F6"),
        Key::F7 => f.write_str("F7"),
        Key::F8 => f.write_str("F8"),
        Key::F9 => f.write_str("F9"),
        Key::F10 => f.write_str("F10"),
        Key::F11 => f.write_str("F11"),
        Key::F12 => f.write_str("F12"),
        Key::Other => f.write_str("?"),
    }
}

#[cfg(test)]
mod tests;
