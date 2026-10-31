//! Cloneable clipboard capability with an in-memory fallback.

#[cfg(feature = "system-clipboard")]
use std::borrow::Cow;
use std::cell::RefCell;
use std::error;
use std::fmt;
use std::rc::Rc;

use crate::primitives::paint::image::Image;
#[cfg(feature = "system-clipboard")]
use crate::primitives::paint::image::error::ImageDataError;
#[cfg(feature = "system-clipboard")]
use glam::UVec2;

/// No clipboard backend could answer. Distinct from an empty clipboard, which is why [`Clipboard::text`] returns a `Result`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClipboardUnavailable;

impl fmt::Display for ClipboardUnavailable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("no clipboard backend could answer")
    }
}

impl error::Error for ClipboardUnavailable {}

/// Where a clipboard reads and writes: the OS clipboard, or the in-process buffer.
#[derive(Debug)]
enum Backend {
    #[cfg(feature = "system-clipboard")]
    System(SystemBackend),
    Memory(MemoryBackend),
    /// A backend a test scripts: it holds text and refuses as told.
    #[cfg(test)]
    Scripted(internals::Scripted),
}

#[cfg_attr(
    not(any(feature = "system-clipboard", test)),
    expect(
        clippy::unnecessary_wraps,
        reason = "only a system backend fails, and this build has none"
    )
)]
impl Backend {
    fn text(&mut self) -> Result<String, ClipboardUnavailable> {
        match self {
            #[cfg(feature = "system-clipboard")]
            Self::System(system) => system.text(),
            Self::Memory(memory) => Ok(memory.text.clone()),
            #[cfg(test)]
            Self::Scripted(scripted) => scripted.text(),
        }
    }

    fn set_text(&mut self, text: &str) -> Result<(), ClipboardUnavailable> {
        match self {
            #[cfg(feature = "system-clipboard")]
            Self::System(system) => system.set_text(text),
            Self::Memory(memory) => {
                memory.set_text(text);
                Ok(())
            }
            #[cfg(test)]
            Self::Scripted(scripted) => scripted.set_text(text),
        }
    }

    fn image(&mut self) -> Result<Option<Image>, ClipboardUnavailable> {
        match self {
            #[cfg(feature = "system-clipboard")]
            Self::System(system) => system.image(),
            Self::Memory(memory) => Ok(memory.image.clone()),
            #[cfg(test)]
            Self::Scripted(scripted) => scripted.image(),
        }
    }

    fn set_image(&mut self, image: &Image, keep: Keep) -> Result<(), ClipboardUnavailable> {
        match self {
            #[cfg(feature = "system-clipboard")]
            Self::System(system) => system.set_image(image),
            Self::Memory(memory) => {
                memory.set_image(image, keep);
                Ok(())
            }
            #[cfg(test)]
            Self::Scripted(scripted) => scripted.set_image(image),
        }
    }
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
impl SystemBackend {
    fn text(&mut self) -> Result<String, ClipboardUnavailable> {
        Self::answer(self.0.get_text())
    }

    fn set_text(&mut self, text: &str) -> Result<(), ClipboardUnavailable> {
        self.0
            .set_text(text.to_owned())
            .map_err(|_| ClipboardUnavailable)
    }

    /// arboard answers `ContentNotAvailable` for a clipboard with no image, which reads as none.
    fn image(&mut self) -> Result<Option<Image>, ClipboardUnavailable> {
        let data = match self.0.get_image() {
            Ok(data) => data,
            Err(arboard::Error::ContentNotAvailable) => return Ok(None),
            Err(_) => return Err(ClipboardUnavailable),
        };
        let (Ok(w), Ok(h)) = (u32::try_from(data.width), u32::try_from(data.height)) else {
            return Err(ClipboardUnavailable);
        };
        image_or_none(Image::from_srgba8(
            UVec2::new(w, h),
            data.bytes.into_owned(),
        ))
    }

    fn set_image(&mut self, image: &Image) -> Result<(), ClipboardUnavailable> {
        self.0
            .set_image(arboard::ImageData {
                width: image.size().x as usize,
                height: image.size().y as usize,
                bytes: Cow::Borrowed(&image.pixels),
            })
            .map_err(|_| ClipboardUnavailable)
    }
}

/// The in-process buffer: text or an image, as a system clipboard holds one or the other.
#[derive(Debug, Default)]
struct MemoryBackend {
    text: String,
    image: Option<Image>,
}

impl MemoryBackend {
    fn set_text(&mut self, text: &str) {
        self.text.clear();
        self.text.push_str(text);
        self.image = None;
    }

    /// An image copied: a text read answers empty after it, and an image read answers what
    /// `keep` says.
    fn set_image(&mut self, image: &Image, keep: Keep) {
        self.text.clear();
        self.image = match keep {
            Keep::Copy => Some(image.clone()),
            Keep::Nothing => None,
        };
    }
}

/// What the in-process buffer keeps of an image write.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Keep {
    /// A copy: no primary took the image, so a read answers from the buffer.
    Copy,
    /// No copy: the primary took the image, and an image read goes there, so a copy would only
    /// double the image in memory.
    Nothing,
}

/// An image the system clipboard handed over, `None` for one with no texel; bytes that make no
/// image read as no answer.
#[cfg(feature = "system-clipboard")]
fn image_or_none(
    image: Result<Image, ImageDataError>,
) -> Result<Option<Image>, ClipboardUnavailable> {
    match image {
        Ok(image) => Ok(Some(image)),
        Err(ImageDataError::ZeroSize { .. }) => Ok(None),
        Err(ImageDataError::TooLarge { .. } | ImageDataError::LengthMismatch { .. }) => {
            Err(ClipboardUnavailable)
        }
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

impl Authority {
    /// Whether the fallback holds the clipboard: the last write reached only it, and the
    /// primary's text, `primary_text`, is what it held then. Another value means another
    /// application copied since, and the primary holds the clipboard again. `None`, a primary
    /// that gave no text, tells nothing, so the fallback keeps it.
    fn fallback_holds(&mut self, primary_text: Option<&str>) -> bool {
        let Self::Fallback { primary_then } = self else {
            return false;
        };
        let Some(text) = primary_text else {
            return true;
        };
        if primary_then.as_deref().is_some_and(|then| then != text) {
            *self = Self::Primary;
            return false;
        }
        primary_then.get_or_insert_with(|| text.to_owned());
        true
    }
}

#[derive(Debug)]
struct ClipboardState {
    primary: Option<Backend>,
    fallback: Backend,
    authority: Authority,
    fallback_cur: bool,
}

impl ClipboardState {
    const fn new(primary: Option<Backend>, fallback: Backend) -> Self {
        Self {
            primary,
            fallback,
            authority: Authority::Primary,
            fallback_cur: false,
        }
    }

    fn text(&mut self) -> Result<String, ClipboardUnavailable> {
        let Some(primary) = self.primary.as_mut() else {
            return self.fallback.text();
        };
        let read = primary.text();
        if self.authority.fallback_holds(read.as_deref().ok()) {
            return self.fallback.text();
        }

        match read {
            Ok(text) => {
                self.fallback_cur = self.fallback.set_text(&text).is_ok();
                Ok(text)
            }
            Err(error) if self.fallback_cur => self.fallback.text().or(Err(error)),
            Err(error) => Err(error),
        }
    }

    /// The clipboard's image, from the backend that holds the clipboard. A read the primary
    /// refuses fails: the fallback keeps no copy of what the primary holds.
    fn image(&mut self) -> Result<Option<Image>, ClipboardUnavailable> {
        let Some(primary) = self.primary.as_mut() else {
            return self.fallback.image();
        };
        if matches!(self.authority, Authority::Fallback { .. }) {
            let text = primary.text();
            if self.authority.fallback_holds(text.as_deref().ok()) {
                return self.fallback.image();
            }
        }
        primary.image()
    }

    /// One write through both backends: the primary first, then the fallback always, so a
    /// refused primary leaves the copy readable in process. `write_one` gets what the fallback
    /// keeps of an image: a copy unless the primary took it.
    fn write(
        &mut self,
        mut write_one: impl FnMut(&mut Backend, Keep) -> Result<(), ClipboardUnavailable>,
    ) -> Result<(), ClipboardUnavailable> {
        let Some(primary) = self.primary.as_mut() else {
            return write_one(&mut self.fallback, Keep::Copy);
        };
        let primary_took = write_one(primary, Keep::Copy).is_ok();
        let keep = if primary_took {
            Keep::Nothing
        } else {
            Keep::Copy
        };
        let fallback_written = write_one(&mut self.fallback, keep).is_ok();

        if primary_took {
            self.authority = Authority::Primary;
            self.fallback_cur = fallback_written;
            Ok(())
        } else if fallback_written {
            self.authority = Authority::Fallback {
                primary_then: primary.text().ok(),
            };
            self.fallback_cur = true;
            Ok(())
        } else {
            Err(ClipboardUnavailable)
        }
    }
}

/// The host's clipboard, a cheap `Rc` clone a widget can hold.
///
/// Obtained only from [`Ui::clipboard`](crate::Ui::clipboard); a clipboard belongs to a host. It
/// reads and writes text or an image.
#[derive(Clone, Debug)]
pub struct Clipboard {
    state: Rc<RefCell<ClipboardState>>,
}

impl Clipboard {
    fn new(primary: Option<Backend>, fallback: Backend) -> Self {
        Self {
            state: Rc::new(RefCell::new(ClipboardState::new(primary, fallback))),
        }
    }

    /// In-process only, for a host with no system backend to reach for.
    pub(crate) fn memory() -> Self {
        Self::new(None, Backend::Memory(MemoryBackend::default()))
    }

    #[cfg(feature = "system-clipboard")]
    pub(crate) fn system_or_memory() -> Self {
        let primary = arboard::Clipboard::new()
            .ok()
            .map(|clipboard| Backend::System(SystemBackend(clipboard)));
        Self::new(primary, Backend::Memory(MemoryBackend::default()))
    }

    /// The clipboard's text. An empty clipboard is `Ok("")`, so test the string, not the `Result`.
    pub fn text(&self) -> Result<String, ClipboardUnavailable> {
        self.state.borrow_mut().text()
    }

    /// Replace the clipboard's text. An action that deletes what it copied (cut) must check the result first.
    pub fn set_text(&self, text: &str) -> Result<(), ClipboardUnavailable> {
        self.state
            .borrow_mut()
            .write(|backend, _| backend.set_text(text))
    }

    /// The clipboard's image, `None` when it holds none, as after a copy of text.
    pub fn image(&self) -> Result<Option<Image>, ClipboardUnavailable> {
        self.state.borrow_mut().image()
    }

    /// Replace the clipboard's contents with `image`, which other applications paste as a
    /// picture. A text read answers empty after it.
    pub fn set_image(&self, image: &Image) -> Result<(), ClipboardUnavailable> {
        self.state
            .borrow_mut()
            .write(|backend, keep| backend.set_image(image, keep))
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::common::clipboard::{Backend, Clipboard, ClipboardUnavailable, MemoryBackend};
    use crate::primitives::paint::image::Image;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// What a [`Scripted`] backend holds and how it answers; a test changes it between calls.
    #[derive(Debug, Default)]
    pub(crate) struct ScriptedState {
        pub(crate) text: String,
        pub(crate) reject_writes: bool,
        pub(crate) reject_reads: bool,
        pub(crate) reads: usize,
        /// The image last written, cleared by a text write.
        pub(crate) image: Option<Image>,
    }

    /// A backend over a state the test keeps a handle to.
    #[derive(Clone, Debug)]
    pub(crate) struct Scripted(pub(crate) Rc<RefCell<ScriptedState>>);

    impl Scripted {
        pub(super) fn text(&self) -> Result<String, ClipboardUnavailable> {
            let mut state = self.0.borrow_mut();
            state.reads += 1;
            if state.reject_reads {
                return Err(ClipboardUnavailable);
            }
            Ok(state.text.clone())
        }

        pub(super) fn set_text(&self, text: &str) -> Result<(), ClipboardUnavailable> {
            let mut state = self.0.borrow_mut();
            if state.reject_writes {
                return Err(ClipboardUnavailable);
            }
            state.text.clear();
            state.text.push_str(text);
            state.image = None;
            Ok(())
        }

        pub(super) fn image(&self) -> Result<Option<Image>, ClipboardUnavailable> {
            let mut state = self.0.borrow_mut();
            state.reads += 1;
            if state.reject_reads {
                return Err(ClipboardUnavailable);
            }
            Ok(state.image.clone())
        }

        /// As a system clipboard holding an image: no text to read.
        pub(super) fn set_image(&self, image: &Image) -> Result<(), ClipboardUnavailable> {
            let mut state = self.0.borrow_mut();
            if state.reject_writes {
                return Err(ClipboardUnavailable);
            }
            state.text.clear();
            state.image = Some(image.clone());
            Ok(())
        }
    }

    impl Clipboard {
        /// Whether the in-process buffer keeps a copy of an image.
        pub(crate) fn buffer_has_image(&self) -> bool {
            match &self.state.borrow().fallback {
                Backend::Memory(memory) => memory.image.is_some(),
                Backend::Scripted(scripted) => scripted.0.borrow().image.is_some(),
                #[cfg(feature = "system-clipboard")]
                Backend::System(_) => unreachable!("the fallback is in process"),
            }
        }

        /// `primary` over the in-process buffer, as a system clipboard sits.
        pub(crate) fn over_scripted(primary: Scripted) -> Self {
            Self::new(
                Some(Backend::Scripted(primary)),
                Backend::Memory(MemoryBackend::default()),
            )
        }
    }

    /// A clipboard whose only backend refuses every read and write.
    pub(crate) fn rejecting() -> Clipboard {
        let state = ScriptedState {
            reject_writes: true,
            reject_reads: true,
            ..ScriptedState::default()
        };
        Clipboard::new(
            None,
            Backend::Scripted(Scripted(Rc::new(RefCell::new(state)))),
        )
    }
}

#[cfg(test)]
mod tests;
