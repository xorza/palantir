use crate::window::vsync::Vsync;
use crate::window::window_commands::WindowCommands;
use crate::window::window_config::WindowConfig;
use crate::window::window_output::WindowOutput;
use crate::window::window_requests::WindowRequests;
use crate::window::window_token::WindowToken;

/// A close request becomes the window's own close command unless app
/// code vetoed it; the veto lasts one drain. The frame's commands move
/// onto `out` behind whatever it already held, and the levels come
/// back unchanged and stay with the recorder.
#[test]
fn drain_settles_the_close_and_moves_the_commands() {
    let me = WindowToken(3);
    let levels = WindowOutput {
        vsync: Vsync::Off,
        ..WindowOutput::default()
    };
    // (close requested, vetoed, closes out)
    for (requested, vetoed, closes) in [
        (false, false, &[][..]),
        (false, true, &[][..]),
        (true, true, &[][..]),
        (true, false, &[me][..]),
    ] {
        let mut requests = WindowRequests {
            close_vetoed: vetoed,
            levels,
            ..WindowRequests::default()
        };
        requests
            .commands
            .open(WindowToken(9), WindowConfig::new("child"));
        let mut out = WindowCommands::default();
        out.close(WindowToken(1));

        let got = requests.drain(me, requested, &mut out);
        let at = format!("requested {requested}, vetoed {vetoed}");
        assert_eq!(got, levels, "{at}: levels");
        assert_eq!(requests.levels, levels, "{at}: the recorder keeps them");
        assert!(!requests.close_vetoed, "{at}: the veto lasts one drain");
        assert!(requests.commands.opens.is_empty(), "{at}: commands moved");
        let opens: Vec<_> = out.opens.iter().map(|w| w.token).collect();
        assert_eq!(opens, [WindowToken(9)], "{at}: opens");
        let want: Vec<_> = [WindowToken(1)]
            .into_iter()
            .chain(closes.iter().copied())
            .collect();
        assert_eq!(out.closes, want, "{at}: closes after what out held");
    }
}
