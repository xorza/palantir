use crate::input::keyboard::key_text::KeyText;
use crate::input::shortcut::*;

fn kp(mods: Modifiers, key: Key) -> KeyPress {
    KeyPress::with(key, mods)
}

/// The primary command modifier held. `Modifiers::ctrl` is the platform-normalized command bit (winit maps Cmd → ctrl on macOS).
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
    // A Russian layout reports physical Z as Cyrillic 'я'; the physical char 'z' recovers the chord.
    let undo = Shortcut::ctrl('Z');
    let russian_z = KeyPress {
        key: Key::Char('я'),
        mods: primary_mod(),
        repeat: false,
        physical: Key::Char('z'),
        text: KeyText::from_char('я'),
    };
    assert!(undo.matches(russian_z), "Cmd+Z fires on a Russian layout");

    // The non-ASCII gate leaves ASCII layouts on the logical path: a Dvorak chord with an ASCII logical char never consults `physical`.
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
    // No command modifier ⇒ no physical fallback (typing, not a chord).
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
    // macOS Control plus a bare key is a chord, so a bare-key shortcut rejects it, as it does Ctrl or Alt.
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

/// Every label on every platform, hand-written: macOS writes glyphs in its canonical order ⌥ ⇧ ⌘ with the primary modifier last and glyphs for the editing keys; Windows and Linux join names with `+` in the order Ctrl, Shift, Alt, then Win or Super. On macOS the Super key is Command, so `meta` shows as ⌘, once even beside `ctrl`.
#[test]
fn the_label_follows_each_platforms_convention() {
    let all = ShortcutMods {
        ctrl: true,
        shift: true,
        alt: true,
        meta: false,
    };
    let meta = ShortcutMods {
        meta: true,
        ..ShortcutMods::NONE
    };
    // (shortcut, macOS, Windows, Linux)
    let rows = [
        (Shortcut::ctrl('C'), "⌘C", "Ctrl+C", "Ctrl+C"),
        // A lowercase ASCII letter shows uppercase.
        (Shortcut::ctrl('c'), "⌘C", "Ctrl+C", "Ctrl+C"),
        (
            Shortcut::ctrl_shift('K'),
            "⇧⌘K",
            "Ctrl+Shift+K",
            "Ctrl+Shift+K",
        ),
        (
            Shortcut::new(ShortcutMods::CTRL, Key::ArrowLeft),
            "⌘←",
            "Ctrl+←",
            "Ctrl+←",
        ),
        (
            Shortcut::new(all, Key::Char('K')),
            "⌥⇧⌘K",
            "Ctrl+Shift+Alt+K",
            "Ctrl+Shift+Alt+K",
        ),
        (
            Shortcut::new(meta, Key::Char('L')),
            "⌘L",
            "Win+L",
            "Super+L",
        ),
        (
            Shortcut::new(ShortcutMods { ctrl: true, ..meta }, Key::Char('L')),
            "⌘L",
            "Ctrl+Win+L",
            "Ctrl+Super+L",
        ),
        (Shortcut::key(Key::Enter), "⏎", "Enter", "Enter"),
        (Shortcut::key(Key::Backspace), "⌫", "Backspace", "Backspace"),
        (Shortcut::key(Key::Delete), "⌦", "Delete", "Delete"),
        (
            Shortcut::new(ShortcutMods::SHIFT, Key::Tab),
            "⇧⇥",
            "Shift+Tab",
            "Shift+Tab",
        ),
        (
            Shortcut::new(ShortcutMods::ALT, Key::F4),
            "⌥F4",
            "Alt+F4",
            "Alt+F4",
        ),
    ];
    for (shortcut, mac, windows, linux) in rows {
        for (platform, want) in [
            (Platform::Mac, mac),
            (Platform::Windows, windows),
            (Platform::Linux, linux),
        ] {
            assert_eq!(
                shortcut.label(platform).to_string(),
                want,
                "{shortcut:?} on {platform:?}",
            );
        }
        let native = match PLATFORM {
            Platform::Mac => mac,
            Platform::Windows => windows,
            Platform::Linux => linux,
        };
        assert_eq!(
            shortcut.to_string(),
            native,
            "`Display` is this build's label"
        );
    }
}

/// The two modifier types name the same sets and each event-state set converts to its shortcut twin; raw macOS Control has no twin, so Control+Shift reads as bare Shift.
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

/// A Super chord converts to its shortcut twin. On Windows and Linux it fires under Super; on macOS, where Super is Command, under Command, which the host reports as `ctrl`, so there it is the same chord with or without `ctrl`. A bare key fires under neither.
#[test]
fn the_meta_modifier_is_super_and_command_on_macos() {
    let held_super = Modifiers {
        meta: true,
        ..Modifiers::NONE
    };
    let declared = ShortcutMods {
        meta: true,
        ..ShortcutMods::NONE
    };
    assert_eq!(ShortcutMods::from(held_super), declared);
    assert!(declared.has_command());
    let super_l = Shortcut::new(declared, Key::Char('L'));
    let ctrl_super_l = Shortcut::new(
        ShortcutMods {
            ctrl: true,
            ..declared
        },
        Key::Char('L'),
    );
    let bare_l = Shortcut::key(Key::Char('L'));
    let press = |mods| kp(mods, Key::Char('l'));
    let held_ctrl_super = Modifiers {
        meta: true,
        ..Modifiers::CTRL
    };

    for platform in [Platform::Windows, Platform::Linux] {
        assert!(
            super_l.matches_on(press(held_super), platform),
            "{platform:?}"
        );
        assert!(
            !super_l.matches_on(press(Modifiers::CTRL), platform),
            "{platform:?}"
        );
        assert!(
            ctrl_super_l.matches_on(press(held_ctrl_super), platform),
            "{platform:?}"
        );
        assert!(
            !ctrl_super_l.matches_on(press(Modifiers::CTRL), platform),
            "{platform:?}"
        );
        assert!(
            !bare_l.matches_on(press(held_super), platform),
            "{platform:?}"
        );
    }
    assert!(super_l.matches_on(press(Modifiers::CTRL), Platform::Mac));
    assert!(ctrl_super_l.matches_on(press(Modifiers::CTRL), Platform::Mac));
    assert!(!super_l.matches_on(press(Modifiers::NONE), Platform::Mac));
    assert!(!bare_l.matches_on(press(Modifiers::CTRL), Platform::Mac));
}
