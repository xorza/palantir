use crate::input::shortcut::*;

fn kp(mods: Modifiers, key: Key) -> KeyPress {
    KeyPress {
        key,
        mods,
        repeat: false,
        physical: Key::Other,
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
fn non_latin_layout_matches_command_chord_via_physical_key() {
    // A Russian layout reports the physical Z key as Cyrillic 'я'; the
    // physical char ('z') recovers the Cmd/Ctrl+Z chord.
    let undo = Shortcut::ctrl('Z');
    let russian_z = KeyPress {
        key: Key::Char('я'),
        mods: primary_mod(),
        repeat: false,
        physical: Key::Char('z'),
    };
    assert!(undo.matches(russian_z), "Cmd+Z fires on a Russian layout");

    // The non-ASCII gate leaves ASCII layouts on the logical path: a
    // Dvorak chord whose logical char is ASCII never consults `physical`,
    // so the physical position can't trigger the wrong shortcut.
    let dvorak_semicolon = KeyPress {
        key: Key::Char(';'),
        mods: primary_mod(),
        repeat: false,
        physical: Key::Char('z'), // physical Z position, but ';' under Dvorak
    };
    assert!(
        !undo.matches(dvorak_semicolon),
        "an ASCII logical key never falls back to the physical position"
    );
}

#[test]
fn non_latin_fallback_requires_a_command_modifier() {
    // No command modifier ⇒ no physical fallback (it's typing, not a chord).
    let bare = Shortcut::key(Key::Char('z'));
    let russian_z = KeyPress {
        key: Key::Char('я'),
        mods: Modifiers::NONE,
        repeat: false,
        physical: Key::Char('z'),
    };
    assert!(!bare.matches(russian_z));
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
fn label_ctrl_letter() {
    let s = Shortcut::ctrl('C').to_string();
    let expected = match PLATFORM {
        Platform::Mac => "⌘C",
        _ => "Ctrl+C",
    };
    assert_eq!(s, expected);
}

#[test]
fn label_ctrl_shift_letter() {
    let s = Shortcut::ctrl_shift('K').to_string();
    let expected = match PLATFORM {
        Platform::Mac => "⇧⌘K",
        _ => "Ctrl+Shift+K",
    };
    assert_eq!(s, expected);
}

#[test]
fn label_non_letter_key() {
    let s = Shortcut::new(Mods::CTRL, Key::ArrowLeft).to_string();
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
