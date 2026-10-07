//! [`FontFamily`] and the process-wide table of interned family names.

use rustc_hash::FxHashMap;
use serde::de;
use std::fmt;
use std::sync::{LazyLock, RwLock, RwLockReadGuard};

/// Static: a deserialized `TextStyle` has no shaper to resolve names against.
static NAMES: LazyLock<RwLock<FamilyTable>> =
    LazyLock::new(|| RwLock::new(FamilyTable::seeded(FAMILY_LIMIT)));

/// Every index a `u16` can name.
const FAMILY_LIMIT: usize = 1 << 16;

/// Append-only; names are leaked so lookups return `&'static str`.
#[derive(Debug)]
struct FamilyTable {
    names: Vec<&'static str>,
    index: FxHashMap<&'static str, u16>,
    /// Fewer than [`FAMILY_LIMIT`] in the test that fills one.
    limit: usize,
}

impl FamilyTable {
    fn seeded(limit: usize) -> Self {
        let mut table = Self {
            names: Vec::new(),
            index: FxHashMap::default(),
            limit,
        };
        for name in [FontFamily::SANS_NAME, FontFamily::MONO_NAME] {
            table.push(name);
        }
        table
    }

    fn get(&self, name: &str) -> Option<FontFamily> {
        self.index.get(name).map(|&index| FontFamily(index))
    }

    fn intern(&mut self, name: &str) -> Option<FontFamily> {
        if let Some(found) = self.get(name) {
            return Some(found);
        }
        if self.names.len() >= self.limit {
            return None;
        }
        Some(self.push(String::leak(name.to_owned())))
    }

    fn push(&mut self, name: &'static str) -> FontFamily {
        let index =
            u16::try_from(self.names.len()).expect("the table limit keeps every index inside u16");
        self.names.push(name);
        self.index.insert(name, index);
        FontFamily(index)
    }
}

/// Which family to shape in; the `Copy` index into the interned name table.
/// Serializes as its name.
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct FontFamily(u16);

impl FontFamily {
    /// Bundled Inter.
    pub const SANS: Self = Self(0);
    /// Bundled JetBrains Mono.
    pub const MONO: Self = Self(1);

    const SANS_NAME: &'static str = "Inter";
    const MONO_NAME: &'static str = "JetBrains Mono";

    /// The family called `name`, interning it on first sight.
    ///
    /// `None` when 65 536 families are already interned and `name` is new.
    pub fn named(name: &str) -> Option<Self> {
        if let Some(found) = read_names().get(name) {
            return Some(found);
        }
        // Re-check under the write lock: two threads can both miss the read.
        NAMES
            .write()
            .expect("the font name table is poisoned")
            .intern(name)
    }

    /// This family's name.
    pub fn name(self) -> &'static str {
        read_names()
            .names
            .get(usize::from(self.0))
            .copied()
            .expect("a font family index this process never interned")
    }

    /// `pub(crate)`: an index means nothing outside this process.
    pub(crate) const fn raw(self) -> u16 {
        self.0
    }

    pub(crate) const fn from_raw(raw: u16) -> Self {
        Self(raw)
    }
}

fn read_names() -> RwLockReadGuard<'static, FamilyTable> {
    NAMES.read().expect("the font name table is poisoned")
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

/// A visitor, so a borrowed name interns without allocating.
#[derive(Debug)]
struct NameVisitor;

impl de::Visitor<'_> for NameVisitor {
    type Value = FontFamily;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a font family name")
    }

    fn visit_str<E: de::Error>(self, name: &str) -> Result<Self::Value, E> {
        FontFamily::named(name).ok_or_else(|| E::custom("more than 65536 font families interned"))
    }
}

#[cfg(test)]
mod tests {
    use crate::text::font_family::{FamilyTable, FontFamily};
    use ron::ser;

    #[test]
    fn the_seeded_families_are_the_bundled_faces() {
        assert_eq!(FontFamily::SANS.raw(), 0);
        assert_eq!(FontFamily::MONO.raw(), 1);
        assert_eq!(FontFamily::SANS.name(), "Inter");
        assert_eq!(FontFamily::MONO.name(), "JetBrains Mono");
        assert_eq!(FontFamily::default(), FontFamily::SANS);
        assert_eq!(FontFamily::named("Inter").unwrap(), FontFamily::SANS);
        assert_eq!(
            FontFamily::named("JetBrains Mono").unwrap(),
            FontFamily::MONO
        );
    }

    #[test]
    fn a_new_name_interns_once() {
        let first = FontFamily::named("Palantir Test Family").unwrap();
        let again = FontFamily::named("Palantir Test Family").unwrap();
        assert_eq!(first, again);
        assert_eq!(first.name(), "Palantir Test Family");
        assert!(first.raw() >= 2, "a fresh name cannot take a seeded index");
    }

    #[test]
    fn a_full_table_refuses_a_new_name() {
        let mut table = FamilyTable::seeded(4);
        let fresh = ["Full Table A", "Full Table B"].map(|name| table.intern(name));
        assert_eq!(fresh, [Some(FontFamily(2)), Some(FontFamily(3))]);
        assert_eq!(table.intern("Full Table C"), None, "the table is full");
        for (name, index) in [
            ("Inter", 0),
            ("JetBrains Mono", 1),
            ("Full Table A", 2),
            ("Full Table B", 3),
        ] {
            assert_eq!(table.intern(name), Some(FontFamily(index)), "{name}");
        }
        assert_eq!(table.names.len(), 4, "a refusal leaves no trace");
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
    }
}
