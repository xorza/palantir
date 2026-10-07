//! Cloneable clipboard capability with an in-memory fallback.

use std::cell::RefCell;
use std::error;
use std::fmt;
use std::rc::Rc;

/// No clipboard backend could answer. Distinct from an empty clipboard, which is why [`Clipboard::text`] returns a `Result`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClipboardUnavailable;

impl fmt::Display for ClipboardUnavailable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("no clipboard backend could answer")
    }
}

impl error::Error for ClipboardUnavailable {}

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
    /// arboard reports an empty clipboard and one with no text as `ContentNotAvailable`; both mean nothing to paste.
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
    /// The last write reached only the fallback. `primary_then` is what the primary held then; any other value on a later read means another application copied since.
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

/// The host's clipboard, a cheap `Rc` clone a widget can hold.
///
/// Obtained only from [`Ui::clipboard`](crate::Ui::clipboard); a clipboard belongs to a host. Text only.
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

    /// The clipboard's text. An empty clipboard is `Ok("")`, so test the string, not the `Result`.
    pub fn text(&self) -> Result<String, ClipboardUnavailable> {
        self.state.borrow_mut().text()
    }

    /// Replace the clipboard's text. An action that deletes what it copied (cut) must check the result first.
    pub fn set_text(&self, text: &str) -> Result<(), ClipboardUnavailable> {
        self.state.borrow_mut().set_text(text)
    }
}

#[cfg(test)]
pub(crate) mod internals {
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
mod tests;
