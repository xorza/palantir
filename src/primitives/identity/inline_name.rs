//! [`InlineName`]: a short name held inline, so a `Copy` value names a thing with no table.

use std::fmt;
use std::str;

/// A name of 1 to `N` bytes, held inline: what [`FontFamily`] and [`EffectName`] each are, each
/// at the length its names need, as every copy of a value that holds one moves all `N` bytes.
/// Zero past the name, so equality and hashing read the whole array.
///
/// [`FontFamily`]: crate::FontFamily
/// [`EffectName`]: crate::EffectName
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct InlineName<const N: usize> {
    len: u8,
    bytes: [u8; N],
}

impl<const N: usize> InlineName<N> {
    /// The longest name, in bytes.
    pub(crate) const MAX_LEN: usize = N;

    /// `name` held inline; `None` for an empty name or one past [`Self::MAX_LEN`] bytes.
    pub(crate) const fn new(name: &str) -> Option<Self> {
        const {
            assert!(N <= u8::MAX as usize, "a name's length fits its one byte");
        }
        let name = name.as_bytes();
        if name.is_empty() || name.len() > Self::MAX_LEN {
            return None;
        }
        let mut bytes = [0; N];
        let mut i = 0;
        while i < name.len() {
            bytes[i] = name[i];
            i += 1;
        }
        Some(Self {
            len: name.len() as u8,
            bytes,
        })
    }

    pub(crate) fn as_str(&self) -> &str {
        str::from_utf8(&self.bytes[..usize::from(self.len)])
            .expect("a name is built from a whole `str`")
    }
}

impl<const N: usize> fmt::Debug for InlineName<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.as_str(), f)
    }
}
