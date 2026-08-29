//! Keyboard shortcuts. One value drives both display ("Ctrl+C") and
//! matching against incoming [`KeyboardEvent::Down`] events, so call
//! sites stop hardcoding the modifier vocabulary.
//!
//! ## Conventions
//!
//! - The primary command modifier (`Mods::ctrl`) maps to the
//!   platform's convention: **Cmd on macOS, Ctrl on Win/Linux** — one
//!   binding fires on ⌘S on a Mac and Ctrl+S elsewhere. Raw Ctrl on
//!   macOS is the rare case; match a `KeyboardEvent::Down` directly
//!   for it.
//! - [`Mods`] is the *shortcut* vocabulary, distinct from [`Modifiers`]
//!   (the event-state vocabulary, which keeps `ctrl` and `cmd` as
//!   separate physical keys).
//! - [`Shortcut::matches`] compares the modifier set *exactly*: Ctrl+A
//!   does NOT match Ctrl+Shift+A. `Char` keys compare ignore-case
//!   because [`Key::Char`] arrives post-shift-layout.
//! - `Display` formats a platform-native label (`"Ctrl+C"` / `"⌘C"`).
//!   Menu rows stream it into [`crate::Ui`]'s retained formatting storage.
//!
//! [`KeyboardEvent::Down`]: crate::KeyboardEvent::Down

use crate::common::platform::{PLATFORM, Platform};
use crate::input::keyboard::key::Key;
use crate::input::keyboard::key_press::KeyPress;
use crate::input::keyboard::modifiers::Modifiers;
use std::fmt;

/// Modifier set for declaring shortcuts. `ctrl` is the primary command
/// key — Cmd on macOS, Ctrl on Win/Linux (see [`Mods::from_event`]);
/// `shift` and `alt` are literal.
///
/// Distinct from event-state [`Modifiers`] on purpose: that type also
/// carries `mac_ctrl` (the raw macOS Control), which shortcut matching
/// must *ignore*. Comparing a `Modifiers` directly would let a held
/// macOS Control break an otherwise-matching chord, so [`Mods`] is the
/// 3-field projection the matcher compares against.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Mods {
    /// The primary command key — Cmd on macOS, Ctrl on Windows and Linux.
    pub ctrl: bool,
    /// Shift, literally.
    pub shift: bool,
    /// Alt / Option, literally.
    pub alt: bool,
}

/// The named sets are the ones the crate's own constructors reach for;
/// any other combination is a struct literal, which is all `Mods` is.
impl Mods {
    /// True if this chord declares any command modifier — the same
    /// question [`Modifiers::any_command`](crate::Modifiers::any_command)
    /// asks of what is *held*, on the side that declares it. Shift alone
    /// does not count: `Shift+Z` is a capital Z, not a chord.
    ///
    /// No `mac_ctrl` here because a chord is declared once and matched on
    /// every platform: `ctrl` *is* Cmd on macOS, and raw Control is a
    /// thing a keyboard reports rather than a thing an app asks for.
    pub const fn any_command(self) -> bool {
        self.ctrl || self.alt
    }

    /// No modifiers — a bare key.
    pub const NONE: Self = Self {
        ctrl: false,
        shift: false,
        alt: false,
    };
    /// Primary command key alone.
    pub const CTRL: Self = Self {
        ctrl: true,
        shift: false,
        alt: false,
    };
    /// Primary command key plus Shift.
    pub const CTRL_SHIFT: Self = Self {
        ctrl: true,
        shift: true,
        alt: false,
    };

    /// Project event-state [`Modifiers`] into shortcut vocabulary. A
    /// 1:1 copy — `Modifiers::ctrl` is already the platform-normalized
    /// primary command bit (Cmd on macOS, Ctrl elsewhere), folded in at
    /// the platform input boundary, so there's nothing
    /// to disambiguate here.
    pub fn from_event(m: Modifiers) -> Self {
        // Destructured exhaustively so a modifier added to `Modifiers`
        // is a compile error here rather than one that silently never
        // reaches shortcut matching. `mac_ctrl` is dropped on purpose —
        // a `Shortcut` cannot express it, and
        // [`Modifiers::any_command`] is what reads it instead.
        let Modifiers {
            ctrl,
            shift,
            alt,
            mac_ctrl: _,
        } = m;
        Self { ctrl, shift, alt }
    }
}

/// A keyboard shortcut: modifier set + key. Construct via the
/// `const fn` helpers ([`Shortcut::ctrl`], [`Shortcut::ctrl_shift`],
/// [`Shortcut::new`]) so bindings can live in `const` items
/// alongside menu definitions.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Shortcut {
    /// Modifier set. Matched **exactly** — `Ctrl+A` never fires on
    /// `Ctrl+Shift+A`.
    pub mods: Mods,
    /// The key. `Char` compares ignore-case, since it arrives post-shift.
    pub key: Key,
}

impl Shortcut {
    /// Any modifier set plus any key. The other constructors are shorthands
    /// over this one.
    pub const fn new(mods: Mods, key: Key) -> Self {
        Self { mods, key }
    }

    /// Bare key, no modifiers. For watches like
    /// `Shortcut::key(Key::Escape)` and event triggers like
    /// `Shortcut::key(Key::Enter)` that don't carry a chord.
    pub const fn key(key: Key) -> Self {
        Self::new(Mods::NONE, key)
    }

    /// `Ctrl+<c>`. `c` should be uppercase ASCII (matching is
    /// case-insensitive, but the label uses what you pass).
    pub const fn ctrl(c: char) -> Self {
        Self::new(Mods::CTRL, Key::Char(c))
    }

    /// `Ctrl+Shift+<c>`. Same casing convention as [`Self::ctrl`].
    pub const fn ctrl_shift(c: char) -> Self {
        Self::new(Mods::CTRL_SHIFT, Key::Char(c))
    }

    /// True iff `kp` matches this shortcut. Modifier comparison is
    /// exact (`ctrl+a` ≠ `ctrl+shift+a`); `Char` keys compare
    /// ignore-case to absorb shift-layout effects. The `repeat` flag is
    /// ignored.
    ///
    /// Non-Latin-layout fallback: a command chord's letter key arrives as the
    /// *active layout's* character (Cyrillic `'я'` for the physical Z on a
    /// Russian layout), which never matches the ASCII shortcut. When the
    /// logical key is a **non-ASCII** `Char` and the chord carries a command
    /// modifier, retry against the layout-independent [physical key]
    /// ([`KeyPress::physical`]) so `Cmd/Ctrl+Z` fires on any layout. The
    /// non-ASCII gate leaves Dvorak / AZERTY untouched — their keys still
    /// produce ASCII letters, in their own intended positions.
    pub fn matches(self, kp: KeyPress) -> bool {
        if self.matches_key(kp.key, kp.mods) {
            return true;
        }
        self.mods.any_command()
            && kp
                .layout_retry()
                .is_some_and(|physical| self.matches_key(physical, kp.mods))
    }

    /// Logical-key match: exact modifiers + ignore-case `Char`, with **no**
    /// layout fallback. The building block [`Self::matches`] layers the
    /// non-Latin physical-key fallback onto. Crate-internal on purpose —
    /// external callers go through [`Self::matches`] so they get the
    /// layout-correct path rather than this logical-only one.
    fn matches_key(self, key: Key, mods: Modifiers) -> bool {
        if Mods::from_event(mods) != self.mods {
            return false;
        }
        match (self.key, key) {
            (Key::Char(a), Key::Char(b)) => a.eq_ignore_ascii_case(&b),
            (a, b) => a == b,
        }
    }
}

/// Platform-native label. macOS uses glyph notation (`⌥⇧⌘<key>`);
/// Win/Linux uses `Ctrl+Shift+Alt+<key>`. The primary modifier renders
/// as ⌘ on macOS (it *is* Cmd there).
impl fmt::Display for Shortcut {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if matches!(PLATFORM, Platform::Mac) {
            // Canonical macOS order: ⌥ ⇧ ⌘ <key>. The primary command
            // modifier (`mods.ctrl`) is Cmd on macOS, so it renders as
            // ⌘ and sits last (right before the key).
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
