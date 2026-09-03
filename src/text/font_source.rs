//! [`FontSource`] — where the bytes of a registered font come from.

use std::borrow::Cow;
use std::path::{Path, PathBuf};

/// The bytes [`Ui::load_font`](crate::Ui::load_font) registers, or the
/// file to map them from.
///
/// Two variants because the two reach different halves of fontdb, not
/// because ownership differs. Bytes become `Source::Binary`; a path
/// becomes `Source::File`, which fontdb maps to parse the face table and
/// then re-maps on the rare later read of the face data, so the file's
/// contents never pass through a `Vec`.
///
/// **A registered file must stay where it is** for as long as the process
/// runs.
///
/// The `From` impls below are the call site: see
/// [`Ui::load_font`](crate::Ui::load_font) for what one reads like.
#[derive(Clone, Debug)]
pub enum FontSource {
    /// `Cow` rather than `Arc<[u8]>`: fontdb stores the bytes as
    /// `Arc<dyn AsRef<[u8]> + Send + Sync>` and `[u8]` is unsized, so an
    /// `Arc<[u8]>` cannot coerce to that trait object at all — it would
    /// need a newtype and a second `Arc` around the first. `include_bytes!`
    /// borrows with no copy, and a runtime read hands over its `Vec`.
    Bytes(Cow<'static, [u8]>),
    File(PathBuf),
}

impl From<&'static [u8]> for FontSource {
    fn from(bytes: &'static [u8]) -> Self {
        Self::Bytes(Cow::Borrowed(bytes))
    }
}

/// What `include_bytes!` produces, taken as it stands — an array
/// reference does not coerce to a slice through a generic bound, and
/// asking every call site for `&include_bytes!(..)[..]` buys nothing.
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

/// A string is a **path**, never a family name — naming a family is
/// [`FontFamily::named`](crate::FontFamily::named), and this call
/// registers bytes. A name passed here fails as
/// [`FontLoadError::Io`](crate::FontLoadError::Io) rather than resolving
/// to something.
impl From<&str> for FontSource {
    fn from(path: &str) -> Self {
        Self::File(PathBuf::from(path))
    }
}
