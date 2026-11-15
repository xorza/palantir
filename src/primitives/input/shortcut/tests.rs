use crate::primitives::input::shortcut::*;

fn kp(mods: Modifiers, key: Key) -> KeyPress {
    KeyPress::with(key, mods)
}

/// The primary command modifier held. `Modifiers::CTRL` is the platform-normalized command bit (winit maps Cmd → ctrl on macOS).
fn primary_mod() -> Modifiers {
    Modifiers::CTRL
}

fn primary_shift_mod() -> Modifiers {
    Modifiers::CTRL | Modifiers::SHIFT
}

/// A chord reads a held set flag by flag through their shared bits: each held flag is its chord
/// twin, raw macOS Control has none, and a whole set maps to the union of its twins.
#[test]
fn a_held_set_reads_as_its_chord_twin() {
    let twins = [
        (Modifiers::CTRL, ShortcutMods::CTRL),
        (Modifiers::SHIFT, ShortcutMods::SHIFT),
        (Modifiers::ALT, ShortcutMods::ALT),
        (Modifiers::META, ShortcutMods::META),
    ];
    for (held, chord) in twins {
        assert_eq!(ShortcutMods::from_modifiers(held), chord, "{held:?}");
    }
    assert_eq!(
        ShortcutMods::from_modifiers(Modifiers::MAC_CTRL),
        ShortcutMods::NONE
    );
    assert_eq!(
        ShortcutMods::from_modifiers(Modifiers::ALL),
        ShortcutMods::ALL,
        "every held flag but raw Control has a twin"
    );
}

/// Which presses a chord matches. The command modifier matches either case of its letter and
/// beside a held macOS Control; an extra or other modifier refuses it, and macOS Control refuses a
/// bare key. A non-Latin layout reaches a command chord through the physical key, which neither
/// an ASCII logical key nor a press without a command modifier consults.
#[test]
fn a_chord_matches_by_its_modifiers_and_key() {
    let press = |key: Key, mods: Modifiers, physical: Key, text: char| KeyPress {
        key,
        mods,
        repeat: false,
        physical,
        text: KeyText::from_char(text),
    };
    let non_latin_z = |mods| press(Key::Char('ζ'), mods, Key::Char('z'), 'ζ');
    let cut = Shortcut::ctrl('X');
    let undo = Shortcut::ctrl('Z');
    let cases = [
        (
            "command, lowercase",
            cut,
            kp(primary_mod(), Key::Char('x')),
            true,
        ),
        (
            "command, uppercase",
            cut,
            kp(primary_mod(), Key::Char('X')),
            true,
        ),
        (
            "command beside macOS Control",
            cut,
            kp(primary_mod() | Modifiers::MAC_CTRL, Key::Char('x')),
            true,
        ),
        (
            "Alt for the command",
            cut,
            kp(Modifiers::ALT, Key::Char('x')),
            false,
        ),
        (
            "an extra Shift",
            Shortcut::ctrl('A'),
            kp(primary_shift_mod(), Key::Char('A')),
            false,
        ),
        (
            "command and Shift",
            Shortcut::new(ShortcutMods::CTRL | ShortcutMods::SHIFT, Key::Char('K')),
            kp(primary_shift_mod(), Key::Char('K')),
            true,
        ),
        (
            "macOS Control on a bare letter",
            Shortcut::key(Key::Char('z')),
            kp(Modifiers::MAC_CTRL, Key::Char('z')),
            false,
        ),
        (
            "macOS Control on a bare Tab",
            Shortcut::key(Key::Tab),
            kp(Modifiers::MAC_CTRL, Key::Tab),
            false,
        ),
        (
            "non-Latin layout, command",
            undo,
            non_latin_z(primary_mod()),
            true,
        ),
        (
            "non-Latin layout, no command",
            Shortcut::key(Key::Char('z')),
            non_latin_z(Modifiers::NONE),
            false,
        ),
        (
            "Dvorak semicolon at physical Z",
            undo,
            press(Key::Char(';'), primary_mod(), Key::Char('z'), ';'),
            false,
        ),
    ];
    for (label, chord, key_press, matches) in cases {
        assert_eq!(chord.matches(key_press), matches, "{label}");
    }
    assert_eq!(
        ShortcutMods::from_modifiers(primary_shift_mod()),
        ShortcutMods::CTRL | ShortcutMods::SHIFT
    );
}

/// Every label on every platform, hand-written: macOS writes glyphs in its canonical order ⌥ ⇧ ⌘ with the primary modifier last and glyphs for the editing keys; Windows and Linux join names with `+` in the order Ctrl, Shift, Alt, then Win or Super. On macOS the Super key is Command, so `META` shows as ⌘, once even beside `CTRL`.
#[test]
fn the_label_follows_each_platforms_convention() {
    let all = ShortcutMods::CTRL | ShortcutMods::SHIFT | ShortcutMods::ALT;
    let meta = ShortcutMods::META;
    // (shortcut, macOS, Windows, Linux)
    let rows = [
        (Shortcut::ctrl('C'), "⌘C", "Ctrl+C", "Ctrl+C"),
        // A lowercase ASCII letter shows uppercase.
        (Shortcut::ctrl('c'), "⌘C", "Ctrl+C", "Ctrl+C"),
        (
            Shortcut::new(ShortcutMods::CTRL | ShortcutMods::SHIFT, Key::Char('K')),
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
            Shortcut::new(meta | ShortcutMods::CTRL, Key::Char('L')),
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
        (
            Modifiers::CTRL | Modifiers::SHIFT,
            ShortcutMods::CTRL | ShortcutMods::SHIFT,
        ),
    ] {
        assert_eq!(ShortcutMods::from_modifiers(held), declared, "{held:?}");
    }
    let mac_control = Modifiers::SHIFT | Modifiers::MAC_CTRL;
    assert_eq!(
        ShortcutMods::from_modifiers(mac_control),
        ShortcutMods::SHIFT
    );
}

/// A Super chord converts to its shortcut twin. On Windows and Linux it fires under Super; on macOS, where Super is Command, under Command, which the host reports as `CTRL`, so there it is the same chord with or without `CTRL`. A bare key fires under neither.
#[test]
fn the_meta_modifier_is_super_and_command_on_macos() {
    let held_super = Modifiers::META;
    let declared = ShortcutMods::META;
    assert_eq!(ShortcutMods::from_modifiers(held_super), declared);
    assert!(declared.has_command());
    let super_l = Shortcut::new(declared, Key::Char('L'));
    let ctrl_super_l = Shortcut::new(declared | ShortcutMods::CTRL, Key::Char('L'));
    let bare_l = Shortcut::key(Key::Char('L'));
    let press = |mods| kp(mods, Key::Char('l'));
    let held_ctrl_super = Modifiers::CTRL | Modifiers::META;

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

/// A plain key converts to its bare chord, which matches it with no modifier held and not with
/// one.
#[test]
fn a_key_converts_to_its_bare_chord() {
    let enter = Shortcut::from(Key::Enter);
    assert_eq!(enter, Shortcut::key(Key::Enter));
    assert!(enter.matches(kp(Modifiers::NONE, Key::Enter)));
    assert!(!enter.matches(kp(Modifiers::CTRL, Key::Enter)));
}

/// Each modifier by each of its names, every named key, one character in either case, and the
/// plus key parse to their shortcut; text that names no key, an unknown modifier and an unknown
/// word are refused with their case.
#[test]
fn shortcuts_parse_from_text() {
    let parse = |text: &str| text.parse::<Shortcut>();
    assert_eq!(parse("ctrl+z"), Ok(Shortcut::ctrl('z')));
    assert_eq!(
        parse("Cmd+Shift+Z"),
        Ok(Shortcut::new(
            ShortcutMods::CTRL | ShortcutMods::SHIFT,
            Key::Char('z')
        ))
    );
    assert_eq!(
        parse(" alt + meta + f2 "),
        Ok(Shortcut::new(
            ShortcutMods::ALT | ShortcutMods::META,
            Key::F2
        ))
    );
    assert_eq!(parse("super+a"), parse("win+a"));
    assert_eq!(parse("ctrl++"), Ok(Shortcut::ctrl('+')));
    assert_eq!(parse("+"), Ok(Shortcut::key(Key::Char('+'))));
    assert_eq!(parse("A"), Ok(Shortcut::key(Key::Char('a'))));
    assert_eq!(parse("é"), Ok(Shortcut::key(Key::Char('é'))));
    for (text, key) in [
        ("enter", Key::Enter),
        ("return", Key::Enter),
        ("escape", Key::Escape),
        ("esc", Key::Escape),
        ("tab", Key::Tab),
        ("backspace", Key::Backspace),
        ("delete", Key::Delete),
        ("del", Key::Delete),
        ("left", Key::ArrowLeft),
        ("right", Key::ArrowRight),
        ("up", Key::ArrowUp),
        ("down", Key::ArrowDown),
        ("home", Key::Home),
        ("end", Key::End),
        ("pageup", Key::PageUp),
        ("pagedown", Key::PageDown),
        ("space", Key::Char(' ')),
        ("f1", Key::F1),
        ("f12", Key::F12),
    ] {
        assert_eq!(parse(text), Ok(Shortcut::key(key)), "{text}");
    }
    assert_eq!(parse(""), Err(ParseShortcutError::NoKey));
    assert_eq!(parse("ctrl+"), Err(ParseShortcutError::NoKey));
    assert_eq!(parse("hyper+a"), Err(ParseShortcutError::Modifier));
    assert_eq!(parse("ctrl+banana"), Err(ParseShortcutError::Key));
}

/// Modifiers alone parse as a shortcut reads the ones before its key: each by each of its names,
/// case-blind and trimmed, a repeat folding in; empty text, an empty name and an unknown one are
/// refused.
#[test]
fn modifiers_parse_from_text() {
    let parse = |text: &str| text.parse::<ShortcutMods>();
    assert_eq!(parse("ctrl"), Ok(ShortcutMods::CTRL));
    assert_eq!(parse("cmd"), Ok(ShortcutMods::CTRL));
    assert_eq!(
        parse(" Shift + ALT "),
        Ok(ShortcutMods::SHIFT | ShortcutMods::ALT)
    );
    assert_eq!(parse("meta"), parse("super"));
    assert_eq!(parse("win"), Ok(ShortcutMods::META));
    assert_eq!(parse("shift+shift"), Ok(ShortcutMods::SHIFT));
    for refused in ["", "ctrl+", "+shift", "hyper"] {
        assert_eq!(
            parse(refused),
            Err(ParseShortcutError::Modifier),
            "{refused:?}"
        );
    }
    assert_eq!(
        "ctrl+shift+z".parse::<Shortcut>().map(|s| s.mods),
        parse("ctrl+shift"),
        "a shortcut's modifiers are their own parse"
    );
}

/// A Windows or Linux label parses back to its shortcut, for every key a label names and every
/// modifier: the label and the parse speak one language. Space labels as a word, as a blank
/// would leave `Ctrl+` dangling; `Key::Other`, which names no key, labels as `Unknown` and parses
/// to no shortcut, where a `?` would parse as the question-mark key.
#[test]
fn a_label_parses_back_to_its_shortcut() {
    let keys = NAMED_KEYS
        .into_iter()
        .filter(|&key| key != Key::Other)
        .chain([Key::Char('q'), Key::Char('7')]);
    for key in NAMED_KEYS {
        assert!(KeyName::of(key).is_some(), "{key:?} is named");
    }
    let mods = [
        ShortcutMods::NONE,
        ShortcutMods::CTRL | ShortcutMods::SHIFT,
        ShortcutMods::ALT | ShortcutMods::META,
    ];
    for platform in [Platform::Linux, Platform::Windows] {
        for key in keys.clone() {
            for mods in mods {
                let shortcut = Shortcut::new(mods, key);
                let label = shortcut.label(platform).to_string();
                assert_eq!(label.parse(), Ok(shortcut), "{label} on {platform:?}");
            }
        }
    }
    let space = Shortcut::new(ShortcutMods::CTRL, Key::Char(' '));
    assert_eq!(space.label(Platform::Linux).to_string(), "Ctrl+Space");
    assert_eq!(space.label(Platform::Mac).to_string(), "⌘Space");
    let other = Shortcut::new(ShortcutMods::CTRL, Key::Other);
    assert_eq!(other.label(Platform::Windows).to_string(), "Ctrl+Unknown");
    assert_eq!(
        "Ctrl+Unknown".parse::<Shortcut>(),
        Err(ParseShortcutError::Key)
    );
}

/// A press types its character, upper case with Shift, and nothing with a command modifier or
/// for a named key.
#[test]
fn a_shortcut_types_what_a_layout_gives() {
    for (shortcut, text) in [
        ("a", "a"),
        ("shift+a", "A"),
        ("shift+é", "É"),
        ("space", " "),
        ("ctrl+a", ""),
        ("alt+shift+a", ""),
        ("enter", ""),
    ] {
        let parsed: Shortcut = shortcut.parse().unwrap();
        assert_eq!(parsed.typed(), KeyText::new(text), "{shortcut}");
    }
}
