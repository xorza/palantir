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
//! - [`Shortcut::label`] returns `Cow::Borrowed` from a const table
//!   for the hot set (`ctrl[+shift] + ASCII letter`). Rare combos
//!   allocate once via `Display`.
//!
//! [`KeyboardEvent::Down`]: crate::input::keyboard::KeyboardEvent::Down

use crate::common::platform::{PLATFORM, Platform};
use crate::input::keyboard::{Key, KeyPress, Modifiers};
use std::borrow::Cow;
use std::fmt;

/// Modifier set for declaring shortcuts. `ctrl` is the primary command
/// key — Cmd on macOS, Ctrl on Win/Linux (see [`Mods::from_event`]);
/// `shift` and `alt` are literal.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Mods {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

impl Mods {
    pub const NONE: Self = Self {
        ctrl: false,
        shift: false,
        alt: false,
    };
    pub const CTRL: Self = Self {
        ctrl: true,
        shift: false,
        alt: false,
    };
    pub const SHIFT: Self = Self {
        ctrl: false,
        shift: true,
        alt: false,
    };
    pub const ALT: Self = Self {
        ctrl: false,
        shift: false,
        alt: true,
    };
    pub const CTRL_SHIFT: Self = Self {
        ctrl: true,
        shift: true,
        alt: false,
    };
    pub const CTRL_ALT: Self = Self {
        ctrl: true,
        shift: false,
        alt: true,
    };

    /// Project event-state [`Modifiers`] into shortcut vocabulary. A
    /// 1:1 copy — `Modifiers::ctrl` is already the platform-normalized
    /// primary command bit (Cmd on macOS, Ctrl elsewhere), folded in at
    /// the input boundary by `modifiers_from_winit`, so there's nothing
    /// to disambiguate here.
    pub fn from_event(m: Modifiers) -> Self {
        Self {
            ctrl: m.ctrl,
            shift: m.shift,
            alt: m.alt,
        }
    }
}

/// A keyboard shortcut: modifier set + key. Construct via the
/// `const fn` helpers ([`Shortcut::ctrl`], [`Shortcut::ctrl_shift`],
/// [`Shortcut::new`]) so bindings can live in `const` items
/// alongside menu definitions.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Shortcut {
    pub mods: Mods,
    pub key: Key,
}

impl Shortcut {
    pub const fn new(mods: Mods, key: Key) -> Self {
        Self { mods, key }
    }

    /// Bare key, no modifiers. For subscriptions like
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

    pub const fn ctrl_shift(c: char) -> Self {
        Self::new(Mods::CTRL_SHIFT, Key::Char(c))
    }

    pub const fn ctrl_alt(c: char) -> Self {
        Self::new(Mods::CTRL_ALT, Key::Char(c))
    }

    /// True iff `kp` matches this shortcut. Modifier comparison is
    /// exact (`ctrl+a` ≠ `ctrl+shift+a`); `Char` keys compare
    /// ignore-case to absorb shift-layout effects. Delegates to
    /// [`Self::matches_key`] — the `repeat` flag is ignored.
    pub fn matches(self, kp: KeyPress) -> bool {
        self.matches_key(kp.key, kp.mods)
    }

    /// As [`Self::matches`] but takes the `(key, mods)` pair
    /// directly. Used by subscription wake-gate checks that don't
    /// have a `KeyPress` in hand.
    pub fn matches_key(self, key: Key, mods: Modifiers) -> bool {
        if Mods::from_event(mods) != self.mods {
            return false;
        }
        match (self.key, key) {
            (Key::Char(a), Key::Char(b)) => a.eq_ignore_ascii_case(&b),
            (a, b) => a == b,
        }
    }

    /// Platform-native label. macOS uses glyph notation (`⌥⇧⌘<key>`);
    /// Win/Linux uses `Ctrl+Shift+Alt+<key>`. The primary modifier
    /// renders as ⌘ on macOS (it *is* Cmd there). The `ctrl[+shift] +
    /// ASCII letter` hot set is a borrowed const; rarer combinations
    /// format via [`Display`] and allocate once.
    pub fn label(self) -> Cow<'static, str> {
        if let Some(s) = label_const(self) {
            return Cow::Borrowed(s);
        }
        Cow::Owned(self.to_string())
    }
}

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

/// Const-table fast path for `ctrl[+shift] + ASCII letter` — the
/// shortcuts menus actually display. Returns `None` for anything
/// outside this set; the slow path falls back to `Display`.
const fn label_const(s: Shortcut) -> Option<&'static str> {
    let Key::Char(c) = s.key else { return None };
    if !c.is_ascii_alphabetic() {
        return None;
    }
    let upper = c.to_ascii_uppercase();
    // Two tables indexed by ASCII letter, one per common mod set.
    if matches!(s.mods, Mods::CTRL) {
        return Some(ctrl_label(upper));
    }
    if matches!(s.mods, Mods::CTRL_SHIFT) {
        return Some(ctrl_shift_label(upper));
    }
    None
}

const fn ctrl_label(c: char) -> &'static str {
    if matches!(PLATFORM, Platform::Mac) {
        match c {
            'A' => "⌘A",
            'B' => "⌘B",
            'C' => "⌘C",
            'D' => "⌘D",
            'E' => "⌘E",
            'F' => "⌘F",
            'G' => "⌘G",
            'H' => "⌘H",
            'I' => "⌘I",
            'J' => "⌘J",
            'K' => "⌘K",
            'L' => "⌘L",
            'M' => "⌘M",
            'N' => "⌘N",
            'O' => "⌘O",
            'P' => "⌘P",
            'Q' => "⌘Q",
            'R' => "⌘R",
            'S' => "⌘S",
            'T' => "⌘T",
            'U' => "⌘U",
            'V' => "⌘V",
            'W' => "⌘W",
            'X' => "⌘X",
            'Y' => "⌘Y",
            'Z' => "⌘Z",
            _ => "?",
        }
    } else {
        match c {
            'A' => "Ctrl+A",
            'B' => "Ctrl+B",
            'C' => "Ctrl+C",
            'D' => "Ctrl+D",
            'E' => "Ctrl+E",
            'F' => "Ctrl+F",
            'G' => "Ctrl+G",
            'H' => "Ctrl+H",
            'I' => "Ctrl+I",
            'J' => "Ctrl+J",
            'K' => "Ctrl+K",
            'L' => "Ctrl+L",
            'M' => "Ctrl+M",
            'N' => "Ctrl+N",
            'O' => "Ctrl+O",
            'P' => "Ctrl+P",
            'Q' => "Ctrl+Q",
            'R' => "Ctrl+R",
            'S' => "Ctrl+S",
            'T' => "Ctrl+T",
            'U' => "Ctrl+U",
            'V' => "Ctrl+V",
            'W' => "Ctrl+W",
            'X' => "Ctrl+X",
            'Y' => "Ctrl+Y",
            'Z' => "Ctrl+Z",
            _ => "?",
        }
    }
}

const fn ctrl_shift_label(c: char) -> &'static str {
    if matches!(PLATFORM, Platform::Mac) {
        match c {
            'A' => "⇧⌘A",
            'B' => "⇧⌘B",
            'C' => "⇧⌘C",
            'D' => "⇧⌘D",
            'E' => "⇧⌘E",
            'F' => "⇧⌘F",
            'G' => "⇧⌘G",
            'H' => "⇧⌘H",
            'I' => "⇧⌘I",
            'J' => "⇧⌘J",
            'K' => "⇧⌘K",
            'L' => "⇧⌘L",
            'M' => "⇧⌘M",
            'N' => "⇧⌘N",
            'O' => "⇧⌘O",
            'P' => "⇧⌘P",
            'Q' => "⇧⌘Q",
            'R' => "⇧⌘R",
            'S' => "⇧⌘S",
            'T' => "⇧⌘T",
            'U' => "⇧⌘U",
            'V' => "⇧⌘V",
            'W' => "⇧⌘W",
            'X' => "⇧⌘X",
            'Y' => "⇧⌘Y",
            'Z' => "⇧⌘Z",
            _ => "?",
        }
    } else {
        match c {
            'A' => "Ctrl+Shift+A",
            'B' => "Ctrl+Shift+B",
            'C' => "Ctrl+Shift+C",
            'D' => "Ctrl+Shift+D",
            'E' => "Ctrl+Shift+E",
            'F' => "Ctrl+Shift+F",
            'G' => "Ctrl+Shift+G",
            'H' => "Ctrl+Shift+H",
            'I' => "Ctrl+Shift+I",
            'J' => "Ctrl+Shift+J",
            'K' => "Ctrl+Shift+K",
            'L' => "Ctrl+Shift+L",
            'M' => "Ctrl+Shift+M",
            'N' => "Ctrl+Shift+N",
            'O' => "Ctrl+Shift+O",
            'P' => "Ctrl+Shift+P",
            'Q' => "Ctrl+Shift+Q",
            'R' => "Ctrl+Shift+R",
            'S' => "Ctrl+Shift+S",
            'T' => "Ctrl+Shift+T",
            'U' => "Ctrl+Shift+U",
            'V' => "Ctrl+Shift+V",
            'W' => "Ctrl+Shift+W",
            'X' => "Ctrl+Shift+X",
            'Y' => "Ctrl+Shift+Y",
            'Z' => "Ctrl+Shift+Z",
            _ => "?",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kp(mods: Modifiers, key: Key) -> KeyPress {
        KeyPress {
            key,
            mods,
            repeat: false,
        }
    }

    /// The primary command modifier held. `Modifiers::ctrl` is already
    /// the platform-normalized command bit (the winit boundary maps Cmd
    /// → ctrl on macOS), so tests construct it directly with no
    /// platform branch.
    fn primary_mod() -> Modifiers {
        Modifiers {
            ctrl: true,
            ..Modifiers::NONE
        }
    }

    fn primary_shift_mod() -> Modifiers {
        Modifiers {
            shift: true,
            ..primary_mod()
        }
    }

    #[test]
    fn primary_modifier_matches() {
        let cut = Shortcut::ctrl('X');
        assert!(cut.matches(kp(primary_mod(), Key::Char('x'))));
        assert!(cut.matches(kp(primary_mod(), Key::Char('X'))));
    }

    #[test]
    fn alt_alone_does_not_match_ctrl() {
        let cut = Shortcut::ctrl('X');
        // A non-command modifier must not satisfy a ctrl shortcut.
        let alt = Modifiers {
            alt: true,
            ..Modifiers::NONE
        };
        assert!(!cut.matches(kp(alt, Key::Char('x'))));
    }

    #[test]
    fn extra_modifier_rejects_match() {
        let cut = Shortcut::ctrl('A');
        // Ctrl+Shift+A must not match plain Ctrl+A.
        let mods = primary_shift_mod();
        assert_eq!(Mods::from_event(mods), Mods::CTRL_SHIFT);
        assert!(!cut.matches(kp(mods, Key::Char('A'))));
        assert_eq!(cut.mods, Mods::CTRL);
    }

    #[test]
    fn ctrl_shift_matches() {
        let s = Shortcut::ctrl_shift('K');
        assert!(s.matches(kp(primary_shift_mod(), Key::Char('K'))));
    }

    #[test]
    fn label_hot_path_borrowed() {
        let s = Shortcut::ctrl('C').label();
        assert!(matches!(s, Cow::Borrowed(_)));
        let expected = match PLATFORM {
            Platform::Mac => "⌘C",
            _ => "Ctrl+C",
        };
        assert_eq!(s, expected);
    }

    #[test]
    fn label_shift_combo_borrowed() {
        let s = Shortcut::ctrl_shift('K').label();
        assert!(matches!(s, Cow::Borrowed(_)));
        let expected = match PLATFORM {
            Platform::Mac => "⇧⌘K",
            _ => "Ctrl+Shift+K",
        };
        assert_eq!(s, expected);
    }

    #[test]
    fn label_non_letter_key_falls_back_to_display() {
        let s = Shortcut::new(Mods::CTRL, Key::ArrowLeft).label();
        assert!(matches!(s, Cow::Owned(_)));
        let expected = match PLATFORM {
            Platform::Mac => "⌘←",
            _ => "Ctrl+←",
        };
        assert_eq!(s, expected);
    }

    #[test]
    fn modifier_order_is_canonical() {
        // Ctrl+Shift+Alt+K. Mac order ⌥ ⇧ ⌘ then key (primary=⌘ last).
        // Else: Ctrl+Shift+Alt+K.
        let s = Shortcut::new(
            Mods {
                ctrl: true,
                shift: true,
                alt: true,
            },
            Key::Char('K'),
        );
        let expected = match PLATFORM {
            Platform::Mac => "⌥⇧⌘K",
            _ => "Ctrl+Shift+Alt+K",
        };
        assert_eq!(s.to_string(), expected);
    }
}
