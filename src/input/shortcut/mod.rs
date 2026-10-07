//! Keyboard shortcuts: one value drives both display ("Ctrl+C") and matching
//! against [`KeyPress`] events.
//!
//! - The primary command modifier (`ShortcutMods::ctrl`) is **Cmd on macOS, Ctrl
//!   elsewhere**, so one binding fires on ⌘S and Ctrl+S; for raw Ctrl on macOS
//!   match a `KeyPress` directly.
//! - [`Shortcut::matches`] compares modifiers *exactly* (Ctrl+A does not match
//!   Ctrl+Shift+A); `Char` keys compare ignore-case since [`Key::Char`] arrives
//!   post-shift-layout.
//!
//! [`KeyPress`]: crate::KeyPress

use crate::common::platform::{PLATFORM, Platform};
use crate::input::keyboard::key::Key;
use crate::input::keyboard::key_press::KeyPress;
use crate::input::keyboard::modifiers::Modifiers;
use std::fmt;

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
    /// The Windows / Super key; never set by a chord meant for macOS.
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

impl From<Modifiers> for ShortcutMods {
    fn from(m: Modifiers) -> Self {
        // Exhaustive, so a new `Modifiers` field is a compile error here;
        // `mac_ctrl` is dropped on purpose, [`Shortcut::matches`] reads it off the
        // press.
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

    /// `Ctrl+<c>`; `c` should be uppercase ASCII (matching ignores case, the label
    /// uses what you pass).
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
        if press.mods.mac_ctrl && !self.mods.has_command() {
            return false;
        }
        if self.matches_key(press.key, press.mods) {
            return true;
        }
        self.mods.has_command()
            && press
                .layout_retry()
                .is_some_and(|physical| self.matches_key(physical, press.mods))
    }

    /// Logical-key match with **no** layout fallback; [`Self::matches`] layers that
    /// on.
    fn matches_key(self, key: Key, mods: Modifiers) -> bool {
        if ShortcutMods::from(mods) != self.mods {
            return false;
        }
        match (self.key, key) {
            (Key::Char(a), Key::Char(b)) => a.eq_ignore_ascii_case(&b),
            (a, b) => a == b,
        }
    }
}

/// Platform-native label: macOS glyphs (`⌥⇧⌘<key>`), else `Ctrl+Shift+Alt+<key>`.
impl fmt::Display for Shortcut {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if matches!(PLATFORM, Platform::Mac) {
            // Canonical macOS order ⌥ ⇧ ⌘ <key>; the primary modifier is Cmd there,
            // so it sits last.
            if self.mods.alt {
                f.write_str("⌥")?;
            }
            if self.mods.shift {
                f.write_str("⇧")?;
            }
            if self.mods.ctrl {
                f.write_str("⌘")?;
            }
            return write_key(f, self.key);
        }
        let mut first = true;
        let sep = |f: &mut fmt::Formatter<'_>, first: &mut bool| -> fmt::Result {
            if !*first {
                f.write_str("+")?;
            }
            *first = false;
            Ok(())
        };
        if self.mods.ctrl {
            sep(f, &mut first)?;
            f.write_str("Ctrl")?;
        }
        if self.mods.shift {
            sep(f, &mut first)?;
            f.write_str("Shift")?;
        }
        if self.mods.alt {
            sep(f, &mut first)?;
            f.write_str("Alt")?;
        }
        if self.mods.meta {
            sep(f, &mut first)?;
            f.write_str(if matches!(PLATFORM, Platform::Windows) {
                "Win"
            } else {
                "Super"
            })?;
        }
        sep(f, &mut first)?;
        write_key(f, self.key)
    }
}

fn write_key(f: &mut fmt::Formatter<'_>, key: Key) -> fmt::Result {
    let mac = matches!(PLATFORM, Platform::Mac);
    match key {
        Key::Char(c) => f.write_fmt(format_args!("{}", c.to_ascii_uppercase())),
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
