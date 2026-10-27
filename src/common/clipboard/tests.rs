use std::cell::RefCell;
use std::rc::Rc;

use crate::common::clipboard::internals::{Scripted, ScriptedState};
use crate::common::clipboard::{Clipboard, ClipboardUnavailable};
use crate::primitives::paint::image::Image;
use glam::UVec2;
use std::error;

#[test]
fn memory_clipboards_roundtrip_and_are_isolated() {
    let first = Clipboard::memory();
    let second = Clipboard::memory();

    first.set_text("clipboard-test-roundtrip-✓").unwrap();

    assert_eq!(first.text().unwrap(), "clipboard-test-roundtrip-✓");
    assert_eq!(second.text().unwrap(), "");
}

#[test]
fn clones_share_one_clipboard() {
    let first = Clipboard::memory();
    let second = first.clone();

    first.set_text("shared").unwrap();

    assert_eq!(second.text().unwrap(), "shared");
}

/// The error crosses the public surface, so it owes the impls a caller needs to box and print it.
#[test]
fn unavailable_reports_itself_as_an_error() {
    let boxed: Box<dyn error::Error> = Box::new(ClipboardUnavailable);
    assert_eq!(boxed.to_string(), "no clipboard backend could answer");
}

fn primary(text: &str) -> Rc<RefCell<ScriptedState>> {
    Rc::new(RefCell::new(ScriptedState {
        text: String::from(text),
        ..ScriptedState::default()
    }))
}

fn over(primary: &Rc<RefCell<ScriptedState>>) -> Clipboard {
    Clipboard::over_scripted(Scripted(Rc::clone(primary)))
}

#[cfg(feature = "system-clipboard")]
#[test]
fn system_reads_without_text_answer_empty() {
    use crate::common::clipboard::SystemBackend;

    let cases = [
        (Ok(String::from("text")), Ok(String::from("text"))),
        (Err(arboard::Error::ContentNotAvailable), Ok(String::new())),
        (
            Err(arboard::Error::ClipboardOccupied),
            Err(ClipboardUnavailable),
        ),
        (
            Err(arboard::Error::ClipboardNotSupported),
            Err(ClipboardUnavailable),
        ),
    ];
    for (read, expected) in cases {
        assert_eq!(SystemBackend::answer(read), expected);
    }
}

/// After a fallback-only write the fallback answers until the primary shows a later copy.
#[test]
fn failed_primary_write_holds_until_another_copy() {
    let primary = primary("stale");
    let clipboard = over(&primary);
    primary.borrow_mut().reject_writes = true;

    clipboard.set_text("fresh").unwrap();
    assert_eq!(primary.borrow().reads, 1);
    assert_eq!(clipboard.text().unwrap(), "fresh");
    assert_eq!(clipboard.text().unwrap(), "fresh");
    assert_eq!(primary.borrow().reads, 3);

    primary.borrow_mut().text = String::from("external");
    assert_eq!(clipboard.text().unwrap(), "external");

    primary.borrow_mut().text = String::from("later");
    assert_eq!(clipboard.text().unwrap(), "later");

    primary.borrow_mut().reject_writes = false;
    clipboard.set_text("replacement").unwrap();
    assert_eq!(primary.borrow().text, "replacement");
    assert_eq!(clipboard.text().unwrap(), "replacement");
}

/// A primary unreadable at write time takes its first later answer as the reference.
#[test]
fn unread_primary_takes_its_next_answer_as_the_reference() {
    let primary = primary("stale");
    let clipboard = over(&primary);
    primary.borrow_mut().reject_writes = true;
    primary.borrow_mut().reject_reads = true;

    clipboard.set_text("fresh").unwrap();
    assert_eq!(clipboard.text().unwrap(), "fresh");

    primary.borrow_mut().reject_reads = false;
    assert_eq!(clipboard.text().unwrap(), "fresh");

    primary.borrow_mut().text = String::from("external");
    assert_eq!(clipboard.text().unwrap(), "external");
}

/// Only a backend error falls back, and only to text mirrored from or written beside the primary.
#[test]
fn unreadable_primary_answers_from_a_current_fallback() {
    let primary = primary("seen");
    let clipboard = over(&primary);
    primary.borrow_mut().reject_reads = true;
    assert_eq!(clipboard.text(), Err(ClipboardUnavailable));

    primary.borrow_mut().reject_reads = false;
    assert_eq!(clipboard.text().unwrap(), "seen");

    primary.borrow_mut().text = String::new();
    assert_eq!(clipboard.text().unwrap(), "");

    primary.borrow_mut().reject_reads = true;
    assert_eq!(clipboard.text().unwrap(), "");

    clipboard.set_text("copied").unwrap();
    assert_eq!(clipboard.text().unwrap(), "copied");
}

/// An image copy replaces the text: in memory, through a primary that takes it, and through one
/// that refuses it, where the fallback answers empty until another application copies.
#[test]
fn an_image_copy_leaves_no_text() {
    let image = Image::blank(UVec2::new(2, 3));

    let memory = Clipboard::memory();
    memory.set_text("text").unwrap();
    memory.set_image(&image).unwrap();
    assert_eq!(memory.text().unwrap(), "");

    let taking = primary("text");
    let clipboard = over(&taking);
    clipboard.set_image(&image).unwrap();
    assert_eq!(taking.borrow().image, Some(UVec2::new(2, 3)));
    assert_eq!(clipboard.text().unwrap(), "");

    let refusing = primary("stale");
    let clipboard = over(&refusing);
    clipboard.set_text("copied").unwrap();
    refusing.borrow_mut().reject_writes = true;
    clipboard.set_image(&image).unwrap();
    assert_eq!(refusing.borrow().image, None);
    assert_eq!(clipboard.text().unwrap(), "");
    refusing.borrow_mut().text = String::from("external");
    assert_eq!(clipboard.text().unwrap(), "external");
}
