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
#[derive(Clone, Debug)]
pub(crate) struct FamilyKeys(Rc<RefCell<Table>>);

#[derive(Debug)]
struct Table {
    families: Vec<FontFamily>,
    keys: FxHashMap<FontFamily, FamilyKey>,
}

impl Default for FamilyKeys {
    fn default() -> Self {
        let keys = Self(Rc::new(RefCell::new(Table {
            families: Vec::new(),
            keys: FxHashMap::default(),
        })));
        for family in [FontFamily::SANS, FontFamily::MONO] {
            keys.key(family);
        }
        keys
    }
}

impl FamilyKeys {
    /// `family`'s key, minted on first sight.
    ///
    /// # Panics
    ///
    /// Panics on a 65 537th family, past what a `u16` key names.
    pub(crate) fn key(&self, family: FontFamily) -> FamilyKey {
        let mut table = self.0.borrow_mut();
        if let Some(&key) = table.keys.get(&family) {
            return key;
        }
        let key = FamilyKey(
            u16::try_from(table.families.len())
                .expect("one shaper keys at most 65 536 font families"),
        );
        table.families.push(family);
        table.keys.insert(family, key);
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

    /// The bundled pair holds the first two keys; a new family takes the next, once; a family
    /// reads back from its key; two tables mint apart.
    #[test]
    fn keys_are_minted_once_per_family_per_table() {
        let keys = FamilyKeys::default();
        assert_eq!(keys.key(FontFamily::SANS), FamilyKey(0));
        assert_eq!(keys.key(FontFamily::MONO), FamilyKey(1));
        let segoe = FontFamily::named("Segoe UI").unwrap();
        assert_eq!(keys.key(segoe), FamilyKey(2));
        assert_eq!(keys.key(segoe), FamilyKey(2));
        assert_eq!(keys.family(FamilyKey(2)), segoe);

        let other = FamilyKeys::default();
        let noto = FontFamily::named("Noto Sans").unwrap();
        assert_eq!(other.key(noto), FamilyKey(2), "each table counts its own");
        assert_eq!(keys.key(noto), FamilyKey(3));
    }
}
