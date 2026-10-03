use std::cell::RefCell;
use std::rc::Rc;

use crate::common::clipboard::{Backend, Clipboard, ClipboardUnavailable, MemoryBackend};

#[derive(Debug)]
struct PrimaryState {
    text: String,
    reject_writes: bool,
    reject_reads: bool,
    reads: usize,
}

#[derive(Clone, Debug)]
struct StaleBackend {
    state: Rc<RefCell<PrimaryState>>,
}

impl Backend for StaleBackend {
    fn get_text(&mut self) -> Result<String, ClipboardUnavailable> {
        let mut state = self.state.borrow_mut();
        state.reads += 1;
        if state.reject_reads {
            return Err(ClipboardUnavailable);
        }
        Ok(state.text.clone())
    }

    fn set_text(&mut self, text: &str) -> Result<(), ClipboardUnavailable> {
        let mut state = self.state.borrow_mut();
        if state.reject_writes {
            return Err(ClipboardUnavailable);
        }
        state.text.clear();
        state.text.push_str(text);
        Ok(())
    }
}

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

/// The error crosses the public surface, so it owes the two impls a
/// caller needs to put it in a `Box<dyn Error>` and print it.
#[test]
fn unavailable_reports_itself_as_an_error() {
    let boxed: Box<dyn std::error::Error> = Box::new(ClipboardUnavailable);
    assert_eq!(boxed.to_string(), "no clipboard backend could answer");
}

fn primary(text: &str) -> Rc<RefCell<PrimaryState>> {
    Rc::new(RefCell::new(PrimaryState {
        text: String::from(text),
        reject_writes: false,
        reject_reads: false,
        reads: 0,
    }))
}

fn over(primary: &Rc<RefCell<PrimaryState>>) -> Clipboard {
    Clipboard::new(
        Some(Box::new(StaleBackend {
            state: Rc::clone(primary),
        })),
        Box::<MemoryBackend>::default(),
    )
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

/// The last write reached only the fallback, so the fallback answers
/// until the primary shows a copy made after that write.
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

/// A primary that could not be read when the write failed takes its
/// first later answer as the reference instead.
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

/// Only a backend error falls back, and only to text the fallback
/// mirrored from the primary or wrote beside it.
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
