//! [`FontSource`] — where the bytes of a registered font come from.

use std::borrow::Cow;
use std::path::{Path, PathBuf};

/// The bytes [`Ui::load_font`](crate::Ui::load_font) registers, or the file to map them from.
///
/// A path becomes fontdb's `Source::File`, mapped lazily, so contents never pass through a `Vec`.
///
/// **A registered file must stay where it is** for the process lifetime.
#[derive(Clone, Debug)]
pub enum FontSource {
    /// `Cow` not `Arc<[u8]>`: fontdb wants `Arc<dyn AsRef<[u8]> + Send + Sync>`, which an unsized `Arc<[u8]>` can't coerce to. `include_bytes!` borrows without copying.
    Bytes(Cow<'static, [u8]>),
    /// A font file on disk, read when the source is registered.
    File(PathBuf),
}

impl From<&'static [u8]> for FontSource {
    fn from(bytes: &'static [u8]) -> Self {
        Self::Bytes(Cow::Borrowed(bytes))
    }
}

/// What `include_bytes!` produces; an array reference doesn't coerce to a slice through a generic bound.
impl<const N: usize> From<&'static [u8; N]> for FontSource {
    fn from(bytes: &'static [u8; N]) -> Self {
        Self::Bytes(Cow::Borrowed(bytes))
    }
}

impl From<Vec<u8>> for FontSource {
    fn from(bytes: Vec<u8>) -> Self {
        Self::Bytes(Cow::Owned(bytes))
    }
}

impl From<Cow<'static, [u8]>> for FontSource {
    fn from(bytes: Cow<'static, [u8]>) -> Self {
        Self::Bytes(bytes)
    }
}

impl From<PathBuf> for FontSource {
    fn from(path: PathBuf) -> Self {
        Self::File(path)
    }
}

impl From<&Path> for FontSource {
    fn from(path: &Path) -> Self {
        Self::File(path.to_path_buf())
    }
}

/// A string is a **path**, never a family name (see [`FontFamily::named`](crate::FontFamily::named)); a name fails as [`FontLoadError::Io`](crate::FontLoadError::Io).
impl From<&str> for FontSource {
    fn from(path: &str) -> Self {
        Self::File(PathBuf::from(path))
    }
}
