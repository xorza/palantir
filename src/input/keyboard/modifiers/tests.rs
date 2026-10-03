use crate::common::platform::Platform;
use crate::input::keyboard::modifiers::Modifiers;

#[test]
fn any_command_excludes_shift() {
    assert!(
        !Modifiers {
            shift: true,
            ..Modifiers::NONE
        }
        .any_command()
    );
    assert!(
        Modifiers {
            ctrl: true,
            ..Modifiers::NONE
        }
        .any_command()
    );
    assert!(
        Modifiers {
            alt: true,
            ..Modifiers::NONE
        }
        .any_command()
    );
}

/// Every modifier combination on every platform, hand-derived from the
/// platform rules: (shift, ctrl, alt, mac_ctrl) → composes on
/// (Mac, Win, Linux).
#[test]
fn compose_text_follows_each_platforms_rule() {
    let rows = [
        // Bare and shifted keys type everywhere.
        ((false, false, false, false), [true, true, true]),
        ((true, false, false, false), [true, true, true]),
        // Ctrl alone (Cmd on macOS) commands everywhere.
        ((false, true, false, false), [false, false, false]),
        // Alt alone: Option composes on macOS, a mnemonic elsewhere.
        ((false, false, true, false), [true, false, false]),
        ((true, false, true, false), [true, false, false]),
        // Ctrl+Alt: Cmd+Option commands on macOS, AltGr composes elsewhere.
        ((false, true, true, false), [false, true, true]),
        // Raw Control on macOS commands.
        ((false, false, false, true), [false, true, true]),
    ];
    for ((shift, ctrl, alt, mac_ctrl), expected) in rows {
        let mods = Modifiers {
            shift,
            ctrl,
            alt,
            mac_ctrl,
        };
        let got = [Platform::Mac, Platform::Win, Platform::Linux].map(|p| mods.compose_text(p));
        assert_eq!(got, expected, "{mods:?}");
    }
}
