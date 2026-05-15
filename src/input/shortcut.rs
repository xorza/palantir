//! Platform-aware keyboard shortcuts. One value drives both display
//! ("⌘C" on macOS, "Ctrl+C" elsewhere) and matching against incoming
//! [`KeyboardEvent::Down`] events, so call sites stop hardcoding the
//! OS split.
//!
//! ## Conventions
//!
//! - [`Mods`] is the *shortcut* vocabulary, distinct from [`Modifiers`]
//!   (the event-state vocabulary). `cmd` resolves to Cmd on macOS and
//!   Ctrl on Win/Linux — the platform's "primary command" modifier.
//!   Raw-Ctrl on macOS is rare enough that the escape hatch is
//!   "match a `KeyboardEvent::Down` directly"; egui takes the same position.
//! - [`Shortcut::matches`] compares the modifier set *exactly*: Cmd+A
//!   does NOT match Cmd+Shift+A. `Char` keys compare ignore-case
//!   because [`Key::Char`] arrives post-shift-layout.
//! - [`Shortcut::label`] returns `Cow::Borrowed` from a const table
//!   for the hot set (`cmd[+shift] + ASCII letter`). Rare combos
//!   allocate once via `Display`.
//!
//! [`KeyboardEvent::Down`]: crate::input::keyboard::KeyboardEvent::Down

use crate::common::platform::{PLATFORM, Platform};
use crate::input::keyboard::{Key, KeyPress, Modifiers};
use std::borrow::Cow;
use std::fmt;

/// Modifier set for declaring shortcuts. `cmd` is the platform
/// "primary command" key — Cmd on macOS, Ctrl on Win/Linux. `shift`
/// and `alt` are literal. Raw Ctrl-on-macOS is not modelled; callers
/// needing it match the keyboard event directly.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Mods {
    pub cmd: bool,
    pub shift: bool,
    pub alt: bool,
}

impl Mods {
    pub const NONE: Self = Self {
        cmd: false,
        shift: false,
        alt: false,
    };
    pub const CMD: Self = Self {
        cmd: true,
        shift: false,
        alt: false,
    };
    pub const SHIFT: Self = Self {
        cmd: false,
        shift: true,
        alt: false,
    };
    pub const ALT: Self = Self {
        cmd: false,
        shift: false,
        alt: true,
    };
    pub const CMD_SHIFT: Self = Self {
        cmd: true,
        shift: true,
        alt: false,
    };
    pub const CMD_ALT: Self = Self {
        cmd: true,
        shift: false,
        alt: true,
    };

    /// Project event-state [`Modifiers`] into shortcut vocabulary.
    /// `cmd = meta` on macOS, `cmd = ctrl` elsewhere.
    pub fn from_event(m: Modifiers) -> Self {
        let cmd = if matches!(PLATFORM, Platform::Mac) {
            m.meta
        } else {
            m.ctrl
        };
        Self {
            cmd,
            shift: m.shift,
            alt: m.alt,
        }
    }
}

/// A keyboard shortcut: modifier set + key. Construct via the
/// `const fn` helpers ([`Shortcut::cmd`], [`Shortcut::cmd_shift`],
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

    /// `Cmd+<c>` on macOS, `Ctrl+<c>` elsewhere. `c` should be
    /// uppercase ASCII (matching is case-insensitive, but the label
    /// uses what you pass).
    pub const fn cmd(c: char) -> Self {
        Self::new(Mods::CMD, Key::Char(c))
    }

    pub const fn cmd_shift(c: char) -> Self {
        Self::new(Mods::CMD_SHIFT, Key::Char(c))
    }

    pub const fn cmd_alt(c: char) -> Self {
        Self::new(Mods::CMD_ALT, Key::Char(c))
    }

    /// True iff `kp` matches this shortcut. Modifier comparison is
    /// exact (`cmd+a` ≠ `cmd+shift+a`); `Char` keys compare
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

    /// Platform-native label. macOS uses glyph notation
    /// (`⌃⌥⇧⌘<key>`); Win/Linux uses `Ctrl+Shift+Alt+<key>`. The
    /// `cmd[+shift] + ASCII letter` hot set is a borrowed const;
    /// rarer combinations format via [`Display`] and allocate once.
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
            // Canonical macOS order: ⌃ ⌥ ⇧ ⌘ <key>. We don't model
            // raw Ctrl, so just option / shift / cmd.
            if self.mods.alt {
                f.write_str("⌥")?;
            }
            if self.mods.shift {
                f.write_str("⇧")?;
            }
            if self.mods.cmd {
                f.write_str("⌘")?;
            }
            write_key(f, self.key)
        } else {
            let mut first = true;
            let sep = |f: &mut fmt::Formatter<'_>, first: &mut bool| -> fmt::Result {
                if !*first {
                    f.write_str("+")?;
                }
                *first = false;
                Ok(())
            };
            if self.mods.cmd {
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
}

fn write_key(f: &mut fmt::Formatter<'_>, key: Key) -> fmt::Result {
    match key {
        Key::Char(c) => f.write_fmt(format_args!("{}", c.to_ascii_uppercase())),
        Key::ArrowLeft => f.write_str("←"),
        Key::ArrowRight => f.write_str("→"),
        Key::ArrowUp => f.write_str("↑"),
        Key::ArrowDown => f.write_str("↓"),
        Key::Backspace => f.write_str(if matches!(PLATFORM, Platform::Mac) {
            "⌫"
        } else {
            "Backspace"
        }),
        Key::Delete => f.write_str(if matches!(PLATFORM, Platform::Mac) {
            "⌦"
        } else {
            "Delete"
        }),
        Key::Home => f.write_str("Home"),
        Key::End => f.write_str("End"),
        Key::PageUp => f.write_str("PgUp"),
        Key::PageDown => f.write_str("PgDn"),
        Key::Enter => f.write_str(if matches!(PLATFORM, Platform::Mac) {
            "⏎"
        } else {
            "Enter"
        }),
        Key::Tab => f.write_str(if matches!(PLATFORM, Platform::Mac) {
            "⇥"
        } else {
            "Tab"
        }),
        Key::Escape => f.write_str("Esc"),
        Key::Other => f.write_str("?"),
    }
}

/// Const-table fast path for `cmd[+shift] + ASCII letter` — the
/// shortcuts menus actually display. Returns `None` for anything
/// outside this set; the slow path falls back to `Display`.
const fn label_const(s: Shortcut) -> Option<&'static str> {
    let Key::Char(c) = s.key else { return None };
    if !c.is_ascii_alphabetic() {
        return None;
    }
    let upper = c.to_ascii_uppercase();
    // Two tables indexed by ASCII letter, one per common mod set.
    if matches!(s.mods, Mods::CMD) {
        return Some(cmd_label(upper));
    }
    if matches!(s.mods, Mods::CMD_SHIFT) {
        return Some(cmd_shift_label(upper));
    }
    None
}

const fn cmd_label(c: char) -> &'static str {
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

const fn cmd_shift_label(c: char) -> &'static str {
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

    #[test]
    fn cmd_matches_platform_primary_modifier() {
        let cut = Shortcut::cmd('X');
        let platform_cmd = if cfg!(target_os = "macos") {
            Modifiers {
                meta: true,
                ..Modifiers::NONE
            }
        } else {
            Modifiers {
                ctrl: true,
                ..Modifiers::NONE
            }
        };
        assert!(cut.matches(kp(platform_cmd, Key::Char('x'))));
        assert!(cut.matches(kp(platform_cmd, Key::Char('X'))));
    }

    #[test]
    fn other_platform_primary_does_not_match() {
        let cut = Shortcut::cmd('X');
        // Cmd on Linux, Ctrl on macOS — opposite of the platform primary.
        let other = if cfg!(target_os = "macos") {
            Modifiers {
                ctrl: true,
                ..Modifiers::NONE
            }
        } else {
            Modifiers {
                meta: true,
                ..Modifiers::NONE
            }
        };
        assert!(!cut.matches(kp(other, Key::Char('x'))));
    }

    #[test]
    fn extra_modifier_rejects_match() {
        let cut = Shortcut::cmd('A');
        let primary = Mods::CMD;
        // Cmd+Shift+A must not match plain Cmd+A.
        let mods = if cfg!(target_os = "macos") {
            Modifiers {
                meta: true,
                shift: true,
                ..Modifiers::NONE
            }
        } else {
            Modifiers {
                ctrl: true,
                shift: true,
                ..Modifiers::NONE
            }
        };
        assert_eq!(Mods::from_event(mods), Mods::CMD_SHIFT);
        assert!(!cut.matches(kp(mods, Key::Char('A'))));
        assert_eq!(cut.mods, primary);
    }

    #[test]
    fn cmd_shift_matches() {
        let s = Shortcut::cmd_shift('K');
        let mods = if cfg!(target_os = "macos") {
            Modifiers {
                meta: true,
                shift: true,
                ..Modifiers::NONE
            }
        } else {
            Modifiers {
                ctrl: true,
                shift: true,
                ..Modifiers::NONE
            }
        };
        assert!(s.matches(kp(mods, Key::Char('K'))));
    }

    #[test]
    fn label_hot_path_borrowed() {
        let s = Shortcut::cmd('C').label();
        assert!(matches!(s, Cow::Borrowed(_)));
        let expected = if cfg!(target_os = "macos") {
            "⌘C"
        } else {
            "Ctrl+C"
        };
        assert_eq!(s, expected);
    }

    #[test]
    fn label_shift_combo_borrowed() {
        let s = Shortcut::cmd_shift('K').label();
        assert!(matches!(s, Cow::Borrowed(_)));
        let expected = if cfg!(target_os = "macos") {
            "⇧⌘K"
        } else {
            "Ctrl+Shift+K"
        };
        assert_eq!(s, expected);
    }

    #[test]
    fn label_non_letter_key_falls_back_to_display() {
        let s = Shortcut::new(Mods::CMD, Key::ArrowLeft).label();
        assert!(matches!(s, Cow::Owned(_)));
        let expected = if cfg!(target_os = "macos") {
            "⌘←"
        } else {
            "Ctrl+←"
        };
        assert_eq!(s, expected);
    }

    #[test]
    fn macos_modifier_order_is_canonical() {
        if !cfg!(target_os = "macos") {
            return;
        }
        // ⌥ ⇧ ⌘ K — option, shift, cmd, then key.
        let s = Shortcut::new(
            Mods {
                cmd: true,
                shift: true,
                alt: true,
            },
            Key::Char('K'),
        );
        assert_eq!(s.to_string(), "⌥⇧⌘K");
    }
}
