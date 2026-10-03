use crate::internals::panic_probe;
use crate::window::window_directory::WindowDirectory;
use crate::window::window_token::WindowToken;

#[test]
fn directory_clones_observe_the_same_live_windows() {
    let host = WindowDirectory::default();
    let recorder = host.clone();

    host.add(WindowToken(1));
    host.add(WindowToken(2));
    assert!(recorder.contains(WindowToken(1)));
    assert!(recorder.contains(WindowToken(2)));

    host.remove(WindowToken(1));
    assert!(!recorder.contains(WindowToken(1)));
    assert!(recorder.contains(WindowToken(2)));
}

/// Two drivers under one token, or a drop of a token never added, is
/// a host bug the directory refuses rather than records.
#[test]
fn a_duplicate_add_or_an_unknown_remove_panics() {
    let directory = WindowDirectory::default();
    directory.add(WindowToken(1));
    panic_probe::assert_panics_with("already contains WindowToken(1)", || {
        directory.add(WindowToken(1))
    });
    panic_probe::assert_panics_with("must be in the window directory", || {
        directory.remove(WindowToken(2))
    });
    assert!(
        directory.contains(WindowToken(1)),
        "the failed calls changed nothing"
    );
}
