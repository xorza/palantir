//! Process-wide clipboard. Primary backend is the OS clipboard via
//! [`arboard`] (same crate egui / helix / lapce use); a per-process
//! in-memory buffer takes over when the OS clipboard is unavailable
//! (headless CI, Wayland without a clipboard manager, sandboxed test
//! environments) or transiently fails.
//!
//! State lives in a `OnceLock<Mutex<…>>` so the API is `&Ui`-free —
//! widget code calls [`get`] / [`set`] without threading state. The
//! mutex serializes the (rare) OS calls; non-clipboard widgets don't
//! pay anything.
//!
//! `cfg(test)` skips the OS path entirely so unit tests don't race
//! against (or pollute) the developer's real clipboard.

use std::sync::{Mutex, OnceLock};

struct Inner {
    /// `None` when arboard failed to initialise (no display server,
    /// etc.) — calls fall through to the in-memory `cache`. Also
    /// `None` in `cfg(test)` to keep tests off the user's real
    /// clipboard.
    #[cfg(not(test))]
    os: Option<arboard::Clipboard>,
    /// Authoritative copy when `os` is `None`; also written through
    /// on every `set` so a transient OS failure doesn't lose the
    /// most recent value.
    cache: String,
}

fn instance() -> &'static Mutex<Inner> {
    static I: OnceLock<Mutex<Inner>> = OnceLock::new();
    I.get_or_init(|| {
        Mutex::new(Inner {
            #[cfg(not(test))]
            os: arboard::Clipboard::new().ok(),
            cache: String::new(),
        })
    })
}

/// Current clipboard contents. Reads the OS clipboard when
/// available, otherwise the in-memory cache. Allocates one `String`
/// per call (OS clipboard text isn't borrowable across the mutex).
pub fn get() -> String {
    #[allow(unused_mut)]
    let mut g = instance().lock().expect("clipboard mutex poisoned");
    #[cfg(not(test))]
    if let Some(c) = g.os.as_mut()
        && let Ok(text) = c.get_text()
    {
        return text;
    }
    g.cache.clone()
}

/// Overwrite the clipboard with `s`. Writes to the OS clipboard and
/// mirrors into the cache so a later `get` round-trip is stable
/// across an OS-clipboard hiccup.
pub fn set(s: &str) {
    #[allow(unused_mut)]
    let mut g = instance().lock().expect("clipboard mutex poisoned");
    #[cfg(not(test))]
    if let Some(c) = g.os.as_mut() {
        let _ = c.set_text(s.to_owned());
    }
    g.cache.clear();
    g.cache.push_str(s);
}

/// `true` when the clipboard currently holds no text. Cheap probe
/// for menu-item `.enabled(...)` flags — avoids the `get()`
/// allocation when the caller only needs a yes/no.
pub fn is_empty() -> bool {
    #[allow(unused_mut)]
    let mut g = instance().lock().expect("clipboard mutex poisoned");
    #[cfg(not(test))]
    if let Some(c) = g.os.as_mut()
        && let Ok(text) = c.get_text()
    {
        return text.is_empty();
    }
    g.cache.is_empty()
}

/// Test-only serialization guard. The clipboard backend is a
/// process-global `Mutex<Inner>`, so parallel tests calling
/// `get`/`set` race on the cached value between each other's
/// assertions. Tests that depend on clipboard state acquire this
/// outer mutex first.
#[cfg(test)]
pub(crate) fn test_serialize_guard() -> std::sync::MutexGuard<'static, ()> {
    static G: OnceLock<Mutex<()>> = OnceLock::new();
    G.get_or_init(|| Mutex::new(()))
        .lock()
        .expect("clipboard test guard poisoned")
}

#[cfg(test)]
mod tests {
    // `cfg(test)` forces the in-memory backend so these assertions
    // don't touch the developer's real OS clipboard.
    use super::*;

    #[test]
    fn set_get_is_empty_roundtrip() {
        let _g = test_serialize_guard();
        set("clipboard-test-roundtrip-✓");
        assert_eq!(get(), "clipboard-test-roundtrip-✓");
        assert!(!is_empty());
        set("");
        assert!(is_empty());
        assert_eq!(get(), "");
    }
}
