//! [`FontSource`] — the bytes of a font to register.

use std::borrow::Cow;

/// The bytes [`Ui::load_font`](crate::Ui::load_font) registers: a font file or a collection.
/// Bytes only, as a library takes bytes and never a path; the caller reads the file.
#[derive(Clone, Debug)]
pub struct FontSource {
    /// `Cow` not `Arc<[u8]>`: fontdb wants `Arc<dyn AsRef<[u8]> + Send + Sync>`, which an unsized
    /// `Arc<[u8]>` can't coerce to. `include_bytes!` borrows without copying.
    pub(crate) bytes: Cow<'static, [u8]>,
}

impl From<&'static [u8]> for FontSource {
    fn from(bytes: &'static [u8]) -> Self {
        Self::from(Cow::Borrowed(bytes))
    }
}

/// What `include_bytes!` produces; an array reference doesn't coerce to a slice through a generic bound.
impl<const N: usize> From<&'static [u8; N]> for FontSource {
    fn from(bytes: &'static [u8; N]) -> Self {
        Self::from(Cow::Borrowed(bytes.as_slice()))
    }
}

impl From<Vec<u8>> for FontSource {
    fn from(bytes: Vec<u8>) -> Self {
        Self::from(Cow::Owned(bytes))
    }
}

impl From<Cow<'static, [u8]>> for FontSource {
    fn from(bytes: Cow<'static, [u8]>) -> Self {
        Self { bytes }
    }
}
