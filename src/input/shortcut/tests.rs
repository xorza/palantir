use crate::input::keyboard::key_text::KeyText;
use crate::input::shortcut::*;

fn kp(mods: Modifiers, key: Key) -> KeyPress {
    KeyPress::with(key, mods)
}

/// The primary command modifier held. `Modifiers::ctrl` is already
/// the platform-normalized command bit (the winit boundary maps Cmd
/// → ctrl on macOS), so tests construct it directly with no
/// platform branch.
fn primary_mod() -> Modifiers {
    Modifiers::CTRL
}

fn primary_shift_mod() -> Modifiers {
    Modifiers::CTRL_SHIFT
}

#[test]
fn primary_modifier_matches() {
    let cut = Shortcut::ctrl('X');
    assert!(cut.matches(kp(primary_mod(), Key::Char('x'))));
    assert!(cut.matches(kp(primary_mod(), Key::Char('X'))));
    let with_mac_ctrl = Modifiers {
        mac_ctrl: true,
        ..primary_mod()
    };
    assert!(
        cut.matches(kp(with_mac_ctrl, Key::Char('x'))),
        "a held macOS Control must not break a chord that declares a command modifier",
    );
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
        text: KeyText::from_char('я'),
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
        text: KeyText::from_char(';'),
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
        text: KeyText::from_char('я'),
    };
    assert!(!bare.matches(russian_z));
}

#[test]
fn alt_alone_does_not_match_ctrl() {
    let cut = Shortcut::ctrl('X');
    // A non-command modifier must not satisfy a ctrl shortcut.
    let alt = Modifiers::ALT;
    assert!(!cut.matches(kp(alt, Key::Char('x'))));
}

#[test]
fn extra_modifier_rejects_match() {
    let cut = Shortcut::ctrl('A');
    // Ctrl+Shift+A must not match plain Ctrl+A.
    let mods = primary_shift_mod();
    assert_eq!(ShortcutMods::from(mods), ShortcutMods::CTRL_SHIFT);
    assert!(!cut.matches(kp(mods, Key::Char('A'))));
    assert_eq!(cut.mods, ShortcutMods::CTRL);
    // macOS Control plus a bare key is a chord, so a bare-key shortcut
    // rejects it, as it rejects Ctrl or Alt.
    let mac_ctrl = Modifiers {
        mac_ctrl: true,
        ..Modifiers::NONE
    };
    for key in [Key::Char('z'), Key::Tab] {
        assert!(
            !Shortcut::key(key).matches(kp(mac_ctrl, key)),
            "{key:?} with macOS Control held must not match the bare key",
        );
    }
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
    let s = Shortcut::new(ShortcutMods::CTRL, Key::ArrowLeft).to_string();
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
        ShortcutMods {
            ctrl: true,
            shift: true,
            alt: true,
            meta: false,
        },
        Key::Char('K'),
    );
    let expected = match PLATFORM {
        Platform::Mac => "⌥⇧⌘K",
        _ => "Ctrl+Shift+Alt+K",
    };
    assert_eq!(s.to_string(), expected);
}

/// The two modifier types name the same sets, and each event-state set
/// converts to its shortcut twin. The raw macOS Control has no twin, so it
/// drops out: Control+Shift held reads as a bare Shift chord.
#[test]
fn named_modifier_sets_convert_to_their_twins() {
    for (held, declared) in [
        (Modifiers::NONE, ShortcutMods::NONE),
        (Modifiers::SHIFT, ShortcutMods::SHIFT),
        (Modifiers::CTRL, ShortcutMods::CTRL),
        (Modifiers::ALT, ShortcutMods::ALT),
        (Modifiers::CTRL_SHIFT, ShortcutMods::CTRL_SHIFT),
    ] {
        assert_eq!(ShortcutMods::from(held), declared, "{held:?}");
    }
    let mac_control = Modifiers {
        mac_ctrl: true,
        ..Modifiers::SHIFT
    };
    assert_eq!(ShortcutMods::from(mac_control), ShortcutMods::SHIFT);
}

/// A Super chord converts to its shortcut twin, a bare-key shortcut does
/// not fire under a held Super, and the display names the key the way the
/// platform does: Win on Windows, Super on Linux.
#[test]
fn the_meta_modifier_reaches_matching_and_display() {
    let held = Modifiers {
        meta: true,
        ..Modifiers::NONE
    };
    let declared = ShortcutMods {
        meta: true,
        ..ShortcutMods::NONE
    };
    assert_eq!(ShortcutMods::from(held), declared);
    assert!(declared.has_command());
    let super_l = Shortcut::new(declared, Key::Char('L'));
    assert!(super_l.matches(kp(held, Key::Char('l'))));
    assert!(!Shortcut::key(Key::Char('L')).matches(kp(held, Key::Char('l'))));
    let shown = super_l.to_string();
    match PLATFORM {
        Platform::Windows => assert_eq!(shown, "Win+L"),
        Platform::Linux => assert_eq!(shown, "Super+L"),
        // macOS reports no Super key: Command is `ctrl` there, so a Super
        // chord is never held and its name is never read.
        Platform::Mac => {}
    }
}
