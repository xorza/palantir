use crate::common::platform::Platform;
use crate::input::keyboard::modifiers::Modifiers;

#[test]
fn any_command_excludes_shift() {
    assert!(!Modifiers::SHIFT.has_command());
    assert!(Modifiers::CTRL.has_command());
    assert!(Modifiers::ALT.has_command());
    let meta = Modifiers {
        meta: true,
        ..Modifiers::NONE
    };
    assert!(meta.has_command(), "the Windows / Super key commands");
}

/// Every modifier combination per platform, hand-derived: (shift, ctrl, alt, mac_ctrl, meta) → composes on (Mac, Windows, Linux). `meta` is never set on macOS.
#[test]
fn compose_text_follows_each_platforms_rule() {
    let rows = [
        ((false, false, false, false, false), [true, true, true]),
        ((true, false, false, false, false), [true, true, true]),
        ((false, true, false, false, false), [false, false, false]),
        // Alt alone: Option composes on macOS, a mnemonic elsewhere.
        ((false, false, true, false, false), [true, false, false]),
        ((true, false, true, false, false), [true, false, false]),
        // Ctrl+Alt: Cmd+Option commands on macOS, AltGr composes elsewhere.
        ((false, true, true, false, false), [false, true, true]),
        ((false, false, false, true, false), [false, true, true]),
        // Super alone, and Super with AltGr, command off macOS.
        ((false, false, false, false, true), [true, false, false]),
        ((false, true, true, false, true), [false, false, false]),
    ];
    for ((shift, ctrl, alt, mac_ctrl, meta), expected) in rows {
        let mods = Modifiers {
            ctrl,
            shift,
            alt,
            mac_ctrl,
            meta,
        };
        let got = [Platform::Mac, Platform::Windows, Platform::Linux].map(|p| mods.compose_text(p));
        assert_eq!(got, expected, "{mods:?}");
    }
}
