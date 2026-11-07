use crate::internals::panic_probe;
use crate::window::window_directory::WindowDirectory;
use crate::window::window_token::WindowToken;

#[test]
fn a_directory_holds_the_live_windows() {
    let directory = WindowDirectory::default();

    directory.add(WindowToken(1));
    directory.add(WindowToken(2));
    assert!(directory.contains(WindowToken(1)));
    assert!(directory.contains(WindowToken(2)));

    directory.remove(WindowToken(1));
    assert!(!directory.contains(WindowToken(1)));
    assert!(directory.contains(WindowToken(2)));
}

/// Two frame loops under one token, or dropping a never-added token, is a host bug the directory refuses.
#[test]
fn a_duplicate_add_or_an_unknown_remove_panics() {
    let dir = WindowDirectory::default();
    dir.add(WindowToken(1));
    panic_probe::assert_panics_with("already contains WindowToken(1)", || {
        dir.add(WindowToken(1));
    });
    panic_probe::assert_panics_with("must be in the window directory", || {
        dir.remove(WindowToken(2));
    });
    assert!(
        dir.contains(WindowToken(1)),
        "the failed calls changed nothing"
    );
}
