//! Cloneable clipboard capability with an in-memory fallback.

use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ClipboardUnavailable;

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
impl Backend for SystemBackend {
    fn get_text(&mut self) -> Result<String, ClipboardUnavailable> {
        self.0.get_text().map_err(|_| ClipboardUnavailable)
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Authority {
    Primary,
    Fallback,
}

#[derive(Debug)]
struct ClipboardState {
    primary: Option<Box<dyn Backend>>,
    fallback: Box<dyn Backend>,
    authority: Authority,
    fallback_current: bool,
}

impl ClipboardState {
    fn new(primary: Option<Box<dyn Backend>>, fallback: Box<dyn Backend>) -> Self {
        let authority = if primary.is_some() {
            Authority::Primary
        } else {
            Authority::Fallback
        };
        Self {
            primary,
            fallback,
            authority,
            fallback_current: false,
        }
    }

    fn get(&mut self) -> Result<String, ClipboardUnavailable> {
        if self.authority == Authority::Fallback {
            return self.fallback.get_text();
        }

        let primary = self
            .primary
            .as_mut()
            .expect("primary clipboard authority without a backend");
        match primary.get_text() {
            Ok(text) => {
                self.fallback_current = self.fallback.set_text(&text).is_ok();
                Ok(text)
            }
            Err(error) if self.fallback_current => self.fallback.get_text().or(Err(error)),
            Err(error) => Err(error),
        }
    }

    fn set(&mut self, text: &str) -> Result<(), ClipboardUnavailable> {
        let fallback_written = self.fallback.set_text(text).is_ok();
        let primary_written = self
            .primary
            .as_mut()
            .is_some_and(|primary| primary.set_text(text).is_ok());

        if primary_written {
            self.authority = Authority::Primary;
            self.fallback_current = fallback_written;
            Ok(())
        } else if fallback_written {
            self.authority = Authority::Fallback;
            self.fallback_current = true;
            Ok(())
        } else {
            Err(ClipboardUnavailable)
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Clipboard {
    state: Rc<RefCell<ClipboardState>>,
}

impl Default for Clipboard {
    fn default() -> Self {
        Self::new(None, Box::<MemoryBackend>::default())
    }
}

impl Clipboard {
    fn new(primary: Option<Box<dyn Backend>>, fallback: Box<dyn Backend>) -> Self {
        Self {
            state: Rc::new(RefCell::new(ClipboardState::new(primary, fallback))),
        }
    }

    #[cfg(feature = "system-clipboard")]
    pub(crate) fn system_or_memory() -> Self {
        let primary = arboard::Clipboard::new()
            .ok()
            .map(|clipboard| Box::new(SystemBackend(clipboard)) as Box<dyn Backend>);
        Self::new(primary, Box::<MemoryBackend>::default())
    }

    pub(crate) fn get(&self) -> String {
        self.state.borrow_mut().get().unwrap_or_default()
    }

    pub(crate) fn set(&self, text: &str) -> Result<(), ClipboardUnavailable> {
        self.state.borrow_mut().set(text)
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
        let first = Clipboard::default();
        let second = Clipboard::default();

        first.set("clipboard-test-roundtrip-✓").unwrap();

        assert_eq!(first.get(), "clipboard-test-roundtrip-✓");
        assert_eq!(second.get(), "");
    }

    #[test]
    fn clones_share_one_clipboard() {
        let first = Clipboard::default();
        let second = first.clone();

        first.set("shared").unwrap();

        assert_eq!(second.get(), "shared");
    }

    #[test]
    fn failed_primary_write_makes_fallback_authoritative() {
        let primary_state = Rc::new(RefCell::new(PrimaryState {
            text: String::from("stale"),
            reject_writes: true,
            reads: 0,
        }));
        let clipboard = Clipboard::new(
            Some(Box::new(StaleBackend {
                state: primary_state.clone(),
            })),
            Box::<MemoryBackend>::default(),
        );

        clipboard.set("fresh").unwrap();

        assert_eq!(clipboard.get(), "fresh");
        assert_eq!(primary_state.borrow().reads, 0);

        primary_state.borrow_mut().reject_writes = false;
        clipboard.set("replacement").unwrap();
        assert_eq!(primary_state.borrow().text, "replacement");

        primary_state.borrow_mut().text = String::from("external");
        assert_eq!(clipboard.get(), "external");
        assert_eq!(primary_state.borrow().reads, 1);
    }
}
