//! [`FontFamily`]: a family named inline, so it needs no table to mean anything.

use serde::de;
use std::fmt;
use std::str;

/// Which family to shape in, by name. `Copy`: the name sits inline, so a family needs no table
/// to name it, and a deserialized style resolves wherever it is shaped. Each shaper keys the
/// families it meets on its own. Serializes as its name.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct FontFamily {
    len: u8,
    /// The name's bytes, zero past `len`, so equality and hashing read the whole array.
    bytes: [u8; Self::MAX_NAME_LEN],
}

impl FontFamily {
    /// The longest name a family holds, in bytes.
    pub const MAX_NAME_LEN: usize = 63;

    /// Bundled Inter.
    pub const SANS: Self = Self::named("Inter").expect("the bundled name fits");
    /// Bundled JetBrains Mono.
    pub const MONO: Self = Self::named("JetBrains Mono").expect("the bundled name fits");

    /// The family called `name`. `None` for an empty name or one longer than
    /// [`Self::MAX_NAME_LEN`] bytes.
    pub const fn named(name: &str) -> Option<Self> {
        let name = name.as_bytes();
        if name.is_empty() || name.len() > Self::MAX_NAME_LEN {
            return None;
        }
        let mut bytes = [0; Self::MAX_NAME_LEN];
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

    /// This family's name.
    pub fn name(&self) -> &str {
        str::from_utf8(&self.bytes[..usize::from(self.len)])
            .expect("a family is built from a whole `str`")
    }
}

impl Default for FontFamily {
    fn default() -> Self {
        Self::SANS
    }
}

impl fmt::Debug for FontFamily {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("FontFamily").field(&self.name()).finish()
    }
}

impl serde::Serialize for FontFamily {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.name())
    }
}

impl<'de> serde::Deserialize<'de> for FontFamily {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_str(NameVisitor)
    }
}

/// A visitor, so a borrowed name reads without allocating.
#[derive(Debug)]
struct NameVisitor;

impl de::Visitor<'_> for NameVisitor {
    type Value = FontFamily;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a font family name of 1 to 63 bytes")
    }

    fn visit_str<E: de::Error>(self, name: &str) -> Result<Self::Value, E> {
        FontFamily::named(name).ok_or_else(|| {
            E::custom(format_args!(
                "a font family name must be 1 to {} bytes, got {}",
                FontFamily::MAX_NAME_LEN,
                name.len()
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::text::font_family::FontFamily;
    use ron::ser;

    #[test]
    fn the_bundled_families_name_the_bundled_faces() {
        assert_eq!(FontFamily::SANS.name(), "Inter");
        assert_eq!(FontFamily::MONO.name(), "JetBrains Mono");
        assert_eq!(FontFamily::default(), FontFamily::SANS);
        assert_eq!(FontFamily::named("Inter"), Some(FontFamily::SANS));
        assert_eq!(FontFamily::named("JetBrains Mono"), Some(FontFamily::MONO));
    }

    /// A name is 1 to 63 bytes: 63 fit, 64 do not, and an empty name names no family.
    #[test]
    fn a_name_fits_or_is_refused() {
        let longest = "f".repeat(FontFamily::MAX_NAME_LEN);
        assert_eq!(FontFamily::named(&longest).unwrap().name(), longest);
        assert_eq!(FontFamily::named(&format!("{longest}f")), None);
        assert_eq!(FontFamily::named(""), None);
        assert_ne!(FontFamily::named("Inter Display"), Some(FontFamily::SANS));
    }

    #[test]
    fn serde_carries_the_name() {
        let encoded = ser::to_string(&FontFamily::MONO).expect("serialize");
        assert_eq!(encoded, "\"JetBrains Mono\"");
        assert_eq!(
            ron::from_str::<FontFamily>(&encoded).expect("parse"),
            FontFamily::MONO
        );

        let unknown: FontFamily = ron::from_str("\"Segoe UI\"").expect("parse");
        assert_eq!(unknown, FontFamily::named("Segoe UI").unwrap());
        assert_eq!(unknown.name(), "Segoe UI");

        let long = format!("\"{}\"", "f".repeat(64));
        assert!(ron::from_str::<FontFamily>(&long).is_err());
    }
}
