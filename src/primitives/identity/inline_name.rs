//! [`InlineName`]: a short name held inline, so a `Copy` value names a thing with no table.

use std::fmt;
use std::str;

/// A name of 1 to [`Self::MAX_LEN`] bytes, held inline: what [`FontFamily`] and [`EffectName`]
/// each are. Zero past the name, so equality and hashing read the whole array.
///
/// [`FontFamily`]: crate::FontFamily
/// [`EffectName`]: crate::EffectName
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct InlineName {
    len: u8,
    bytes: [u8; Self::MAX_LEN],
}

impl InlineName {
    /// The longest name, in bytes.
    pub(crate) const MAX_LEN: usize = 63;

    /// `name` held inline; `None` for an empty name or one past [`Self::MAX_LEN`] bytes.
    pub(crate) const fn new(name: &str) -> Option<Self> {
        let name = name.as_bytes();
        if name.is_empty() || name.len() > Self::MAX_LEN {
            return None;
        }
        let mut bytes = [0; Self::MAX_LEN];
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

impl fmt::Debug for InlineName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.as_str(), f)
    }
}
