//! Cloneable clipboard capability with an in-memory fallback.

use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

/// No clipboard backend could answer.
///
/// Distinct from an empty clipboard, and the distinction is the reason
/// [`Clipboard::text`] returns a `Result` rather than a `String`: a paste
/// that cannot tell the two apart replaces the selection it was asked to
/// fill with nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClipboardUnavailable;

impl fmt::Display for ClipboardUnavailable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("no clipboard backend could answer")
    }
}

impl std::error::Error for ClipboardUnavailable {}

trait Backend: fmt::Debug {
    fn get_text(&mut self) -> Result<String, ClipboardUnavailable>;
    fn set_text(&mut self, text: &str) -> Result<(), ClipboardUnavailable>;
}

#[cfg(feature = "system-clipboard")]
struct SystemBackend(arboard::Clipboard);

#[cfg(feature = "system-clipboard")]
impl fmt::Debug for SystemBackend {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SystemBackend")
            .finish_non_exhaustive()
    }
}

#[cfg(feature = "system-clipboard")]
impl SystemBackend {
    /// arboard reports an empty clipboard, and one holding no text, as
    /// `ContentNotAvailable`. Both are a clipboard with nothing to paste.
    fn answer(read: Result<String, arboard::Error>) -> Result<String, ClipboardUnavailable> {
        match read {
            Ok(text) => Ok(text),
            Err(arboard::Error::ContentNotAvailable) => Ok(String::new()),
            Err(_) => Err(ClipboardUnavailable),
        }
    }
}

#[cfg(feature = "system-clipboard")]
impl Backend for SystemBackend {
    fn get_text(&mut self) -> Result<String, ClipboardUnavailable> {
        Self::answer(self.0.get_text())
    }

    fn set_text(&mut self, text: &str) -> Result<(), ClipboardUnavailable> {
        self.0
            .set_text(text.to_owned())
            .map_err(|_| ClipboardUnavailable)
    }
}

#[derive(Debug, Default)]
struct MemoryBackend {
    text: String,
}

impl Backend for MemoryBackend {
    fn get_text(&mut self) -> Result<String, ClipboardUnavailable> {
        Ok(self.text.clone())
    }

    fn set_text(&mut self, text: &str) -> Result<(), ClipboardUnavailable> {
        self.text.clear();
        self.text.push_str(text);
        Ok(())
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Authority {
    Primary,
    /// The last write reached only the fallback. `primary_then` is what the
    /// primary held at that moment: a read that finds anything else there
    /// means another application copied since, which makes the primary the
    /// newer clipboard again.
    Fallback {
        primary_then: Option<String>,
    },
}

#[derive(Debug)]
struct ClipboardState {
    primary: Option<Box<dyn Backend>>,
    fallback: Box<dyn Backend>,
    authority: Authority,
    fallback_current: bool,
}

impl ClipboardState {
    const fn new(primary: Option<Box<dyn Backend>>, fallback: Box<dyn Backend>) -> Self {
        Self {
            primary,
            fallback,
            authority: Authority::Primary,
            fallback_current: false,
        }
    }

    fn text(&mut self) -> Result<String, ClipboardUnavailable> {
        let Some(primary) = self.primary.as_mut() else {
            return self.fallback.get_text();
        };
        let read = primary.get_text();

        if let Authority::Fallback { primary_then } = &mut self.authority {
            match &read {
                Ok(text) if primary_then.as_ref().is_some_and(|then| then != text) => {
                    self.authority = Authority::Primary;
                }
                Ok(text) => {
                    primary_then.get_or_insert_with(|| text.clone());
                    return self.fallback.get_text();
                }
                Err(_) => return self.fallback.get_text(),
            }
        }

        match read {
            Ok(text) => {
                self.fallback_current = self.fallback.set_text(&text).is_ok();
                Ok(text)
            }
            Err(error) if self.fallback_current => self.fallback.get_text().or(Err(error)),
            Err(error) => Err(error),
        }
    }

    fn set_text(&mut self, text: &str) -> Result<(), ClipboardUnavailable> {
        let fallback_written = self.fallback.set_text(text).is_ok();
        let Some(primary) = self.primary.as_mut() else {
            return if fallback_written {
                Ok(())
            } else {
                Err(ClipboardUnavailable)
            };
        };

        if primary.set_text(text).is_ok() {
            self.authority = Authority::Primary;
            self.fallback_current = fallback_written;
            Ok(())
        } else if fallback_written {
            self.authority = Authority::Fallback {
                primary_then: primary.get_text().ok(),
            };
            self.fallback_current = true;
            Ok(())
        } else {
            Err(ClipboardUnavailable)
        }
    }
}

/// The host's clipboard, as a handle a widget can hold.
///
/// Obtained from [`Ui::clipboard`](crate::Ui::clipboard), which is the
/// only way in — a clipboard belongs to a host, and one built beside that
/// host would answer for nobody. The clone it hands back is an `Rc` bump,
/// so a widget takes one before a keyboard walk and passes it to whatever
/// handles the paste, instead of carrying the whole `Ui` there.
///
/// Text only, today.
#[derive(Clone, Debug)]
pub struct Clipboard {
    state: Rc<RefCell<ClipboardState>>,
}

impl Clipboard {
    fn new(primary: Option<Box<dyn Backend>>, fallback: Box<dyn Backend>) -> Self {
        Self {
            state: Rc::new(RefCell::new(ClipboardState::new(primary, fallback))),
        }
    }

    /// In-process only, for a host with no system backend to reach for.
    pub(crate) fn memory() -> Self {
        Self::new(None, Box::<MemoryBackend>::default())
    }

    #[cfg(feature = "system-clipboard")]
    pub(crate) fn system_or_memory() -> Self {
        let primary = arboard::Clipboard::new()
            .ok()
            .map(|clipboard| Box::new(SystemBackend(clipboard)) as Box<dyn Backend>);
        Self::new(primary, Box::<MemoryBackend>::default())
    }

    /// The clipboard's text. An empty clipboard answers `Ok("")`, so a
    /// caller that only wants to know whether there is anything to paste
    /// tests the string rather than the `Result`.
    pub fn text(&self) -> Result<String, ClipboardUnavailable> {
        self.state.borrow_mut().text()
    }

    /// Replace the clipboard's text.
    ///
    /// A widget that acts on the write — a cut, which deletes what it just
    /// copied — checks the result first. Losing the copy and the selection
    /// together is the one outcome the user cannot undo from the clipboard.
    pub fn set_text(&self, text: &str) -> Result<(), ClipboardUnavailable> {
        self.state.borrow_mut().set_text(text)
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use crate::common::clipboard::{Backend, Clipboard, ClipboardUnavailable};

    #[derive(Debug)]
    struct RejectingBackend;

    impl Backend for RejectingBackend {
        fn get_text(&mut self) -> Result<String, ClipboardUnavailable> {
            Err(ClipboardUnavailable)
        }

        fn set_text(&mut self, _text: &str) -> Result<(), ClipboardUnavailable> {
            Err(ClipboardUnavailable)
        }
    }

    pub(crate) fn rejecting() -> Clipboard {
        Clipboard::new(None, Box::new(RejectingBackend))
    }
}

#[cfg(test)]
mod tests {
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
}
