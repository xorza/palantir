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
pub struct ClipboardError;

impl fmt::Display for ClipboardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("no clipboard backend could answer")
    }
}

impl error::Error for ClipboardError {}

/// The clipboard outside the process a [`Clipboard`] writes through first: the OS clipboard, or
/// in this crate's tests one they script.
#[derive(Debug)]
enum Primary {
    #[cfg(feature = "system-clipboard")]
    System(SystemBackend),
    /// A backend a test scripts: it holds text and refuses as told.
    #[cfg(test)]
    Scripted(internals::Scripted),
}

#[cfg_attr(
    not(feature = "system-clipboard"),
    expect(
        clippy::needless_pass_by_ref_mut,
        reason = "only the system clipboard's calls take `&mut`"
    )
)]
#[cfg_attr(
    not(any(feature = "system-clipboard", test)),
    expect(
        unused_variables,
        clippy::missing_const_for_fn,
        reason = "with no system clipboard and no test backend, no primary exists, so each \
                  match is empty"
    )
)]
impl Primary {
    fn text(&mut self) -> Result<String, ClipboardError> {
        match *self {
            #[cfg(feature = "system-clipboard")]
            Self::System(ref mut system) => system.text(),
            #[cfg(test)]
            Self::Scripted(ref scripted) => scripted.text(),
        }
    }

    fn image(&mut self) -> Result<Option<Image>, ClipboardError> {
        match *self {
            #[cfg(feature = "system-clipboard")]
            Self::System(ref mut system) => system.image(),
            #[cfg(test)]
            Self::Scripted(ref scripted) => scripted.image(),
        }
    }

    fn write(&mut self, content: Content<'_>) -> Result<(), ClipboardError> {
        match *self {
            #[cfg(feature = "system-clipboard")]
            Self::System(ref mut system) => match content {
                Content::Text(text) => system.set_text(text),
                Content::Image(image) => system.set_image(image),
            },
            #[cfg(test)]
            Self::Scripted(ref scripted) => match content {
                Content::Text(text) => scripted.set_text(text),
                Content::Image(image) => scripted.set_image(image),
            },
        }
    }
}

/// What a write puts on the clipboard: text or an image, as a system clipboard holds one or the
/// other.
#[derive(Clone, Copy, Debug)]
enum Content<'a> {
    Text(&'a str),
    Image(&'a Image),
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
    fn answer(read: Result<String, arboard::Error>) -> Result<String, ClipboardError> {
        match read {
            Ok(text) => Ok(text),
            Err(arboard::Error::ContentNotAvailable) => Ok(String::new()),
            Err(_) => Err(ClipboardError),
        }
    }
}

#[cfg(feature = "system-clipboard")]
impl SystemBackend {
    fn text(&mut self) -> Result<String, ClipboardError> {
        Self::answer(self.0.get_text())
    }

    fn set_text(&mut self, text: &str) -> Result<(), ClipboardError> {
        self.0.set_text(text.to_owned()).map_err(|_| ClipboardError)
    }

    /// arboard answers `ContentNotAvailable` for a clipboard with no image, which reads as none.
    fn image(&mut self) -> Result<Option<Image>, ClipboardError> {
        let data = match self.0.get_image() {
            Ok(data) => data,
            Err(arboard::Error::ContentNotAvailable) => return Ok(None),
            Err(_) => return Err(ClipboardError),
        };
        let (Ok(w), Ok(h)) = (u32::try_from(data.width), u32::try_from(data.height)) else {
            return Err(ClipboardError);
        };
        image_or_none(Image::from_srgba8(
            UVec2::new(w, h),
            data.bytes.into_owned(),
        ))
    }

    fn set_image(&mut self, image: &Image) -> Result<(), ClipboardError> {
        self.0
            .set_image(arboard::ImageData {
                width: image.size().x as usize,
                height: image.size().y as usize,
                bytes: Cow::Borrowed(&image.pixels),
            })
            .map_err(|_| ClipboardError)
    }
}

/// The in-process buffer: text or an image, as a system clipboard holds one or the other.
#[derive(Debug, Default)]
struct MemoryBackend {
    text: String,
    image: Option<Image>,
}

impl MemoryBackend {
    /// `content` written: text replaces the image, and an image the text, a text read answering
    /// empty after it; of an image, the buffer keeps what `keep` says.
    fn write(&mut self, content: Content<'_>, keep: Keep) {
        self.text.clear();
        match content {
            Content::Text(text) => {
                self.text.push_str(text);
                self.image = None;
            }
            Content::Image(image) => {
                self.image = match keep {
                    Keep::Copy => Some(image.clone()),
                    Keep::Nothing => None,
                };
            }
        }
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
fn image_or_none(image: Result<Image, ImageDataError>) -> Result<Option<Image>, ClipboardError> {
    match image {
        Ok(image) => Ok(Some(image)),
        Err(ImageDataError::ZeroSize { .. }) => Ok(None),
        Err(ImageDataError::LengthOverflow { .. } | ImageDataError::LengthMismatch { .. }) => {
            Err(ClipboardError)
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

/// A clipboard's backends and which one holds the clipboard: the primary, when there is one, and
/// the in-process buffer beside it, which every write reaches, so a copy is never lost.
#[derive(Debug)]
struct ClipboardState {
    primary: Option<Primary>,
    memory: MemoryBackend,
    authority: Authority,
    /// Whether the buffer holds what the primary last answered or was last written, so a read
    /// the primary refuses can answer from it.
    mirror_cur: bool,
    /// Whether a refused write was logged; the first is, so a refusing system clipboard shows in
    /// the log without one line a copy.
    refusal_logged: bool,
}

impl ClipboardState {
    const fn new(primary: Option<Primary>) -> Self {
        Self {
            primary,
            memory: MemoryBackend {
                text: String::new(),
                image: None,
            },
            authority: Authority::Primary,
            mirror_cur: false,
            refusal_logged: false,
        }
    }

    fn text(&mut self) -> Result<String, ClipboardError> {
        let Some(primary) = self.primary.as_mut() else {
            return Ok(self.memory.text.clone());
        };
        let read = primary.text();
        if self.authority.fallback_holds(read.as_deref().ok()) {
            return Ok(self.memory.text.clone());
        }
        match read {
            Ok(text) => {
                self.memory.write(Content::Text(&text), Keep::Copy);
                self.mirror_cur = true;
                Ok(text)
            }
            Err(_) if self.mirror_cur => Ok(self.memory.text.clone()),
            Err(error) => Err(error),
        }
    }

    /// The clipboard's image, from the backend that holds the clipboard. A read the primary
    /// refuses fails: the buffer keeps no copy of what the primary holds.
    fn image(&mut self) -> Result<Option<Image>, ClipboardError> {
        let Some(primary) = self.primary.as_mut() else {
            return Ok(self.memory.image.clone());
        };
        if matches!(self.authority, Authority::Fallback { .. }) {
            let text = primary.text();
            if self.authority.fallback_holds(text.as_deref().ok()) {
                return Ok(self.memory.image.clone());
            }
        }
        primary.image()
    }

    /// One write through both backends: the primary first, then the buffer always, so a refused
    /// primary leaves the copy readable in process. The buffer keeps an image only when no
    /// primary took it. A refused primary is the system's and no caller's to handle, so the
    /// first refusal is logged and none returned.
    fn write(&mut self, content: Content<'_>) {
        let Some(primary) = self.primary.as_mut() else {
            self.memory.write(content, Keep::Copy);
            return;
        };
        if primary.write(content).is_ok() {
            self.memory.write(content, Keep::Nothing);
            self.authority = Authority::Primary;
        } else {
            self.memory.write(content, Keep::Copy);
            self.authority = Authority::Fallback {
                primary_then: primary.text().ok(),
            };
            if !self.refusal_logged {
                self.refusal_logged = true;
                tracing::warn!("the system clipboard refused a copy; it stays in this process");
            }
        }
        self.mirror_cur = true;
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
    fn new(primary: Option<Primary>) -> Self {
        Self {
            state: Rc::new(RefCell::new(ClipboardState::new(primary))),
        }
    }

    /// In-process only, for a host with no system backend to reach for.
    pub(crate) fn memory() -> Self {
        Self::new(None)
    }

    #[cfg(feature = "system-clipboard")]
    pub(crate) fn system_or_memory() -> Self {
        let primary = arboard::Clipboard::new()
            .ok()
            .map(|clipboard| Primary::System(SystemBackend(clipboard)));
        Self::new(primary)
    }

    /// The clipboard's text, `None` when it holds none, as [`Self::image`] answers an empty
    /// clipboard.
    pub fn text(&self) -> Result<Option<String>, ClipboardError> {
        self.state
            .borrow_mut()
            .text()
            .map(|text| (!text.is_empty()).then_some(text))
    }

    /// Replace the clipboard's text. It always lands: a system clipboard that refuses it leaves
    /// it in this process, where a read answers it until another application copies, as egui's
    /// `copy_text` reports nothing. Refused: arboard's `Err` for a write the system refused that
    /// the process kept, which each caller would have to interpret.
    pub fn set_text(&self, text: &str) {
        self.state.borrow_mut().write(Content::Text(text));
    }

    /// The clipboard's image, `None` when it holds none, as after a copy of text.
    pub fn image(&self) -> Result<Option<Image>, ClipboardError> {
        self.state.borrow_mut().image()
    }

    /// Replace the clipboard's contents with `image`, which other applications paste as a
    /// picture; a text read answers empty after it. It always lands, as [`Self::set_text`] does.
    pub fn set_image(&self, image: &Image) {
        self.state.borrow_mut().write(Content::Image(image));
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::clipboard::{Clipboard, ClipboardError, Primary};
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
        pub(super) fn text(&self) -> Result<String, ClipboardError> {
            let mut state = self.0.borrow_mut();
            state.reads += 1;
            if state.reject_reads {
                return Err(ClipboardError);
            }
            Ok(state.text.clone())
        }

        pub(super) fn set_text(&self, text: &str) -> Result<(), ClipboardError> {
            let mut state = self.0.borrow_mut();
            if state.reject_writes {
                return Err(ClipboardError);
            }
            state.text.clear();
            state.text.push_str(text);
            state.image = None;
            Ok(())
        }

        pub(super) fn image(&self) -> Result<Option<Image>, ClipboardError> {
            let mut state = self.0.borrow_mut();
            state.reads += 1;
            if state.reject_reads {
                return Err(ClipboardError);
            }
            Ok(state.image.clone())
        }

        /// As a system clipboard holding an image: no text to read.
        pub(super) fn set_image(&self, image: &Image) -> Result<(), ClipboardError> {
            let mut state = self.0.borrow_mut();
            if state.reject_writes {
                return Err(ClipboardError);
            }
            state.text.clear();
            state.image = Some(image.clone());
            Ok(())
        }
    }

    impl Clipboard {
        /// Whether the in-process buffer keeps a copy of an image.
        pub(crate) fn buffer_has_image(&self) -> bool {
            self.state.borrow().memory.image.is_some()
        }

        /// `primary` over the in-process buffer, as a system clipboard sits.
        pub(crate) fn over_scripted(primary: Scripted) -> Self {
            Self::new(Some(Primary::Scripted(primary)))
        }
    }
}

#[cfg(test)]
mod tests;
