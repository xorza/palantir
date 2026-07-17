//! Process-wide clipboard backed by the OS clipboard with an in-memory
//! fallback for headless environments and transient backend failures.

use std::sync::{Mutex, OnceLock};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ClipboardUnavailable;

trait Backend {
    fn get_text(&mut self) -> Result<String, ClipboardUnavailable>;
    fn set_text(&mut self, text: &str) -> Result<(), ClipboardUnavailable>;
}

impl Backend for arboard::Clipboard {
    fn get_text(&mut self) -> Result<String, ClipboardUnavailable> {
        arboard::Clipboard::get_text(self).map_err(|_| ClipboardUnavailable)
    }

    fn set_text(&mut self, text: &str) -> Result<(), ClipboardUnavailable> {
        arboard::Clipboard::set_text(self, text.to_owned()).map_err(|_| ClipboardUnavailable)
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
struct Clipboard<P, F> {
    primary: Option<P>,
    fallback: F,
    authority: Authority,
    fallback_current: bool,
}

impl<P: Backend, F: Backend> Clipboard<P, F> {
    fn new(primary: Option<P>, fallback: F) -> Self {
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
            // A failed OS write may still be followed by a successful stale read.
            self.authority = Authority::Fallback;
            self.fallback_current = true;
            Ok(())
        } else {
            Err(ClipboardUnavailable)
        }
    }
}

#[cfg(not(test))]
type ProcessClipboard = Clipboard<arboard::Clipboard, MemoryBackend>;

#[cfg(test)]
type ProcessClipboard = Clipboard<arboard::Clipboard, TestBackend>;

#[cfg(test)]
#[derive(Debug, Default)]
struct TestBackend {
    text: String,
    reject_writes: bool,
}

#[cfg(test)]
impl Backend for TestBackend {
    fn get_text(&mut self) -> Result<String, ClipboardUnavailable> {
        Ok(self.text.clone())
    }

    fn set_text(&mut self, text: &str) -> Result<(), ClipboardUnavailable> {
        if self.reject_writes {
            return Err(ClipboardUnavailable);
        }
        self.text.clear();
        self.text.push_str(text);
        Ok(())
    }
}

fn instance() -> &'static Mutex<ProcessClipboard> {
    static INSTANCE: OnceLock<Mutex<ProcessClipboard>> = OnceLock::new();
    INSTANCE.get_or_init(|| {
        #[cfg(not(test))]
        let clipboard = Clipboard::new(arboard::Clipboard::new().ok(), MemoryBackend::default());
        #[cfg(test)]
        let clipboard = Clipboard::new(None, TestBackend::default());
        Mutex::new(clipboard)
    })
}

/// Current clipboard contents, or an empty string when neither backend can read.
pub(crate) fn get() -> String {
    instance()
        .lock()
        .expect("clipboard mutex poisoned")
        .get()
        .unwrap_or_default()
}

/// Writes to the OS clipboard when available, otherwise to the authoritative
/// in-memory fallback. Fails only when neither destination accepts the text.
pub(crate) fn set(text: &str) -> Result<(), ClipboardUnavailable> {
    instance()
        .lock()
        .expect("clipboard mutex poisoned")
        .set(text)
}

#[cfg(test)]
pub(crate) mod test_support {
    use crate::common::clipboard::instance;
    use std::sync::{Mutex, OnceLock};

    #[derive(Debug)]
    pub(crate) struct RejectWritesGuard {
        previous: bool,
    }

    impl Drop for RejectWritesGuard {
        fn drop(&mut self) {
            instance()
                .lock()
                .expect("clipboard mutex poisoned")
                .fallback
                .reject_writes = self.previous;
        }
    }

    /// Clipboard tests share process-global state.
    pub(crate) fn test_serialize_guard() -> std::sync::MutexGuard<'static, ()> {
        static GUARD: OnceLock<Mutex<()>> = OnceLock::new();
        GUARD
            .get_or_init(|| Mutex::new(()))
            .lock()
            .expect("clipboard test guard poisoned")
    }

    pub(crate) fn reject_writes() -> RejectWritesGuard {
        let mut clipboard = instance().lock().expect("clipboard mutex poisoned");
        let previous = clipboard.fallback.reject_writes;
        clipboard.fallback.reject_writes = true;
        RejectWritesGuard { previous }
    }
}

#[cfg(test)]
mod tests {
    use crate::common::clipboard::test_support::test_serialize_guard;
    use crate::common::clipboard::*;

    #[derive(Debug)]
    struct StaleBackend {
        text: String,
        reject_writes: bool,
        reads: usize,
    }

    impl Backend for StaleBackend {
        fn get_text(&mut self) -> Result<String, ClipboardUnavailable> {
            self.reads += 1;
            Ok(self.text.clone())
        }

        fn set_text(&mut self, text: &str) -> Result<(), ClipboardUnavailable> {
            if self.reject_writes {
                return Err(ClipboardUnavailable);
            }
            self.text.clear();
            self.text.push_str(text);
            Ok(())
        }
    }

    #[test]
    fn set_get_roundtrip() {
        let _guard = test_serialize_guard();
        set("clipboard-test-roundtrip-✓").unwrap();
        assert_eq!(get(), "clipboard-test-roundtrip-✓");
        set("").unwrap();
        assert_eq!(get(), "");
    }

    #[test]
    fn failed_primary_write_makes_fallback_authoritative() {
        let primary = StaleBackend {
            text: String::from("stale"),
            reject_writes: true,
            reads: 0,
        };
        let mut clipboard = Clipboard::new(Some(primary), MemoryBackend::default());

        clipboard.set("fresh").unwrap();

        assert_eq!(clipboard.get().unwrap(), "fresh");
        assert_eq!(clipboard.authority, Authority::Fallback);
        assert_eq!(clipboard.primary.as_ref().unwrap().reads, 0);

        clipboard.primary.as_mut().unwrap().reject_writes = false;
        clipboard.set("replacement").unwrap();
        assert_eq!(clipboard.authority, Authority::Primary);
        assert_eq!(clipboard.primary.as_ref().unwrap().text, "replacement");

        clipboard.primary.as_mut().unwrap().text = String::from("external");
        assert_eq!(clipboard.get().unwrap(), "external");
        assert_eq!(clipboard.primary.as_ref().unwrap().reads, 1);
    }
}
