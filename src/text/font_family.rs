//! [`FontFamily`] and the process-wide table of interned family names.

use rustc_hash::FxHashMap;
use std::fmt;
use std::sync::{LazyLock, RwLock, RwLockReadGuard};

/// The process-wide [`FamilyTable`].
///
/// A static, and the crate's only mutable one. Nothing here is
/// per-shaper because a [`TextStyle`](crate::TextStyle) deserialized
/// from a theme file has no shaper in reach to resolve a name against —
/// and a family has to survive that round trip as the same two bytes the
/// hot path carries.
static NAMES: LazyLock<RwLock<FamilyTable>> =
    LazyLock::new(|| RwLock::new(FamilyTable::seeded(FAMILY_LIMIT)));

/// Every index a `u16` can name.
const FAMILY_LIMIT: usize = 1 << 16;

/// Append-only table mapping a [`FontFamily`] index to its name, and the
/// name back to its index.
///
/// Append-only, so an index handed out stays valid and `name` never has
/// to fail. Names are leaked on the way in, which is what lets the table
/// answer in `&'static str` and cosmic's `Attrs<'static>` hold one
/// without a copy per shape — and what lets the reverse index key on the
/// same leaked string rather than on a second copy.
#[derive(Debug)]
struct FamilyTable {
    names: Vec<&'static str>,
    index: FxHashMap<&'static str, u16>,
    /// How many names the table takes: [`FAMILY_LIMIT`] for the process
    /// table, fewer in the test that fills one.
    limit: usize,
}

impl FamilyTable {
    /// The two bundled families at their fixed indices, and room for
    /// `limit` names in all.
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

    /// The family called `name`, interning it when the table has not seen
    /// it, or `None` when it has not and holds [`Self::limit`] names
    /// already.
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

/// Which family to shape in, as an index into the interned name table.
///
/// Identity is the **name** — the unit every CSS engine, Zed and Slint
/// resolve on — and the index is what the hot path carries.
/// The shape-cache key and [`GlyphFont`](crate::widget::GlyphFont) both hold one,
/// so this stays `Copy` and two bytes wide.
///
/// Serializes as its name, so a theme file says `family: "Inter"` and
/// reads back as the same index this process interned.
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct FontFamily(u16);

impl FontFamily {
    /// The default proportional family: bundled Inter.
    pub const SANS: Self = Self(0);
    /// The bundled monospace family: JetBrains Mono.
    pub const MONO: Self = Self(1);

    const SANS_NAME: &'static str = "Inter";
    const MONO_NAME: &'static str = "JetBrains Mono";

    /// The family called `name`, interning it when this process has not
    /// seen it before.
    ///
    /// Cold: one lock, and a leak the first time a name appears. Naming
    /// a family no face answers to is not an error here — it resolves at
    /// shaping time, and [`Ui::font_available`](crate::Ui::font_available)
    /// is what asks in advance.
    ///
    /// # Panics
    ///
    /// Panics when 65 536 families are interned already and `name` is
    /// not one of them.
    pub fn named(name: &str) -> Self {
        Self::try_named(name).expect("more than 65536 font families interned")
    }

    /// [`Self::named`], or `None` when the table is full and `name` is not
    /// in it. Untrusted names — a theme file's — come through here, so a
    /// hostile file is a deserialization error rather than a panic.
    pub(crate) fn try_named(name: &str) -> Option<Self> {
        if let Some(found) = read_names().get(name) {
            return Some(found);
        }
        // `intern` searches again under the write lock rather than trusting
        // the read above: two threads can both miss it, and a name interned
        // twice would be two families.
        NAMES
            .write()
            .expect("the font name table is poisoned")
            .intern(name)
    }

    /// This family's name, as `Family::Name` wants it.
    pub fn name(self) -> &'static str {
        read_names()
            .names
            .get(usize::from(self.0))
            .copied()
            .expect("a font family index this process never interned")
    }

    /// The key's spelling of a family, and its inverse.
    ///
    /// Both `pub(crate)`: an index means nothing outside the process that
    /// interned it, so it is never part of the published surface — a
    /// family crosses that boundary as a name.
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

/// The name rather than the index, so a `{:?}` of a `TextStyle` reads
/// like the theme file it came from.
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

/// A visitor rather than `String::deserialize`, so a borrowed name off a
/// theme file interns without an allocation it would immediately drop.
struct NameVisitor;

impl serde::de::Visitor<'_> for NameVisitor {
    type Value = FontFamily;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a font family name")
    }

    fn visit_str<E: serde::de::Error>(self, name: &str) -> Result<Self::Value, E> {
        FontFamily::try_named(name)
            .ok_or_else(|| E::custom("more than 65536 font families interned"))
    }
}

#[cfg(test)]
mod tests {
    use crate::text::font_family::{FamilyTable, FontFamily};

    /// The two seeded families are the names `resolved_name` answers and the
    /// indices the key encodes — pinned together because the table's
    /// order is what makes `SANS`/`MONO` those indices.
    #[test]
    fn the_seeded_families_are_the_bundled_faces() {
        assert_eq!(FontFamily::SANS.raw(), 0);
        assert_eq!(FontFamily::MONO.raw(), 1);
        assert_eq!(FontFamily::SANS.name(), "Inter");
        assert_eq!(FontFamily::MONO.name(), "JetBrains Mono");
        assert_eq!(FontFamily::default(), FontFamily::SANS);
        assert_eq!(FontFamily::named("Inter"), FontFamily::SANS);
        assert_eq!(FontFamily::named("JetBrains Mono"), FontFamily::MONO);
    }

    /// Interning is idempotent, and a new name lands past the seeded two.
    #[test]
    fn a_new_name_interns_once() {
        let first = FontFamily::named("Palantir Test Family");
        let again = FontFamily::named("Palantir Test Family");
        assert_eq!(first, again);
        assert_eq!(first.name(), "Palantir Test Family");
        assert!(first.raw() >= 2, "a fresh name cannot take a seeded index");
    }

    /// A full table refuses a new name and keeps answering every name it
    /// holds with the index it gave out. Four names: the two seeded ones
    /// at 0 and 1, then two fresh ones at 2 and 3.
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

    /// A family round-trips through serde as its name, and an unknown
    /// name deserializes to the family interning it produces.
    #[test]
    fn serde_carries_the_name() {
        let encoded = ron::ser::to_string(&FontFamily::MONO).expect("serialize");
        assert_eq!(encoded, "\"JetBrains Mono\"");
        assert_eq!(
            ron::from_str::<FontFamily>(&encoded).expect("parse"),
            FontFamily::MONO
        );

        let unknown: FontFamily = ron::from_str("\"Segoe UI\"").expect("parse");
        assert_eq!(unknown, FontFamily::named("Segoe UI"));
        assert_eq!(unknown.name(), "Segoe UI");
    }
}
