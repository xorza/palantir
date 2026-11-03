//! [`FamilyKeys`]: the families one shaper has met, each under the `u16` its cache keys carry.

use crate::text::font_family::FontFamily;
use rustc_hash::FxHashMap;
use std::cell::RefCell;
use std::rc::Rc;

/// A family's index in one shaper's [`FamilyKeys`], the form a cache key and a recorded text
/// shape carry. Meaningless to any other shaper.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct FamilyKey(u16);

impl FamilyKey {
    pub(crate) const fn get(self) -> u16 {
        self.0
    }

    pub(crate) const fn from_raw(raw: u16) -> Self {
        Self(raw)
    }
}

/// The families one shaper keys: shared by the shaper and every record store that records text
/// for it, so a key minted on either side names one family. Append-only; seeded with
/// [`FontFamily::SANS`] and [`FontFamily::MONO`].
///
/// Bounded at the 65 536 families a `u16` key names, about 13 MiB with their index. Past them a
/// new family shapes as [`FontFamily::SANS`], and the table logs it once: an app that mints a
/// family per keystroke of a font picker reaches the bound, and the run it shapes shows in the
/// bundled face. Refused: a 32-bit key, which takes `TextShapeKey` from 24 to 32 bytes on every
/// cached run for a case no app should reach; a panic, on input a person types.
#[derive(Clone, Debug)]
pub(crate) struct FamilyKeys(Rc<RefCell<Table>>);

#[derive(Debug)]
struct Table {
    families: Vec<FontFamily>,
    keys: FxHashMap<FontFamily, FamilyKey>,
    /// The key [`FamilyKeys::key`] answered last. A frame's text runs mostly share one family,
    /// and comparing its 64 bytes costs a fraction of hashing them.
    last: FamilyKey,
    /// Whether a family past the bound was answered with [`FamilyKey::SANS`], logged once.
    full: bool,
}

impl FamilyKey {
    /// [`FontFamily::SANS`]'s key, the first a table seeds.
    const SANS: Self = Self(0);
}

impl Default for FamilyKeys {
    fn default() -> Self {
        let keys = Self(Rc::new(RefCell::new(Table {
            families: Vec::new(),
            keys: FxHashMap::default(),
            last: FamilyKey::SANS,
            full: false,
        })));
        for family in [FontFamily::SANS, FontFamily::MONO] {
            keys.key(family);
        }
        debug_assert_eq!(keys.key(FontFamily::SANS), FamilyKey::SANS);
        keys
    }
}

impl FamilyKeys {
    /// `family`'s key, minted on first sight; [`FontFamily::SANS`]'s past the bound (see
    /// [`FamilyKeys`]).
    pub(crate) fn key(&self, family: FontFamily) -> FamilyKey {
        let mut table = self.0.borrow_mut();
        if table.families.get(usize::from(table.last.0)) == Some(&family) {
            return table.last;
        }
        let key = if let Some(&key) = table.keys.get(&family) {
            key
        } else if let Ok(raw) = u16::try_from(table.families.len()) {
            let key = FamilyKey(raw);
            table.families.push(family);
            table.keys.insert(family, key);
            key
        } else {
            if !table.full {
                table.full = true;
                tracing::error!(
                    "a shaper keyed {} font families, all a key names; {family:?} and every new \
                     family after it shape as the bundled sans",
                    table.families.len(),
                );
            }
            return FamilyKey::SANS;
        };
        table.last = key;
        key
    }

    /// The family `key` names.
    ///
    /// # Panics
    ///
    /// Panics on a key another table minted.
    pub(crate) fn family(&self, key: FamilyKey) -> FontFamily {
        *self
            .0
            .borrow()
            .families
            .get(usize::from(key.0))
            .expect("a family key from another shaper")
    }
}

#[cfg(test)]
mod tests {
    use crate::text::family_keys::{FamilyKey, FamilyKeys};
    use crate::text::font_family::FontFamily;

    /// The bundled pair holds the first two keys; a new family takes the next, once, and keeps
    /// it after another family's lookup; a family reads back from its key; two tables mint apart.
    #[test]
    fn keys_are_minted_once_per_family_per_table() {
        let keys = FamilyKeys::default();
        assert_eq!(keys.key(FontFamily::SANS), FamilyKey(0));
        assert_eq!(keys.key(FontFamily::MONO), FamilyKey(1));
        let segoe = FontFamily::named("Segoe UI").unwrap();
        assert_eq!(keys.key(segoe), FamilyKey(2));
        assert_eq!(keys.key(segoe), FamilyKey(2));
        assert_eq!(
            keys.key(FontFamily::SANS),
            FamilyKey(0),
            "past the last key"
        );
        assert_eq!(keys.key(segoe), FamilyKey(2), "back to it");
        assert_eq!(keys.family(FamilyKey(2)), segoe);

        let other = FamilyKeys::default();
        let noto = FontFamily::named("Noto Sans").unwrap();
        assert_eq!(other.key(noto), FamilyKey(2), "each table counts its own");
        assert_eq!(keys.key(noto), FamilyKey(3));
    }

    /// The table holds 65 536 families, the seeded two among them; the next shapes as the bundled
    /// sans, and a family keyed before the bound keeps its key.
    #[test]
    fn a_family_past_the_bound_shapes_as_the_bundled_sans() {
        let keys = FamilyKeys::default();
        let family = |i: u32| FontFamily::named(&format!("family {i}")).unwrap();
        for i in 2..=u32::from(u16::MAX) {
            assert_eq!(keys.key(family(i)).get(), u16::try_from(i).unwrap());
        }
        let past = family(1 << 16);
        assert_eq!(keys.key(past), FamilyKey::SANS);
        assert_eq!(keys.family(keys.key(past)), FontFamily::SANS);
        assert_eq!(keys.key(family(7)), FamilyKey(7));
    }
}
