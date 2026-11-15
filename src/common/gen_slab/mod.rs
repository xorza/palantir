//! [`GenSlab`]: values in reused slots, each named by a key that stops resolving once its value
//! is removed.

/// A slot of a [`GenSlab`] and the generation its value took it at, as slotmap's `KeyData`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct SlabKey {
    pub(crate) idx: u32,
    pub(crate) generation: u32,
}

/// Values in slots a removal frees for the next insert, with slotmap's names and its guarantee
/// (`SlotMap`): a key names its slot and the generation its value took it at, and a removal moves
/// the generation, so a removed key never resolves again, even once its slot holds another value.
/// A vacant slot holds no value, so nothing of a removed one lingers. The generations are a column
/// of their own, so a value packs alone, as a walk over the values reads no generation. Refused:
/// the `slotmap` dependency, whose key leaves no room for what a caller's handle adds to it, as an
/// atlas handle's format.
///
/// Its bound is its caller's: the slots are the most values held at once.
#[derive(Debug)]
pub(crate) struct GenSlab<T> {
    /// Each slot's value, `None` where it is vacant.
    values: Vec<Option<T>>,
    /// Each slot's generation, the one its current or next value takes it at.
    generations: Vec<u32>,
    /// Vacant slots, the next insert's last first.
    free: Vec<u32>,
}

impl<T> Default for GenSlab<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> GenSlab<T> {
    /// An empty slab.
    pub(crate) const fn new() -> Self {
        Self {
            values: Vec::new(),
            generations: Vec::new(),
            free: Vec::new(),
        }
    }

    /// File `value` in the last slot freed, else a new one, and name it.
    pub(crate) fn insert(&mut self, value: T) -> SlabKey {
        let idx = if let Some(idx) = self.free.pop() {
            let slot = &mut self.values[idx as usize];
            debug_assert!(slot.is_none(), "a free slot is vacant");
            *slot = Some(value);
            idx
        } else {
            self.values.push(Some(value));
            self.generations.push(0);
            u32::try_from(self.values.len() - 1).expect("a slab counts its slots in u32")
        };
        SlabKey {
            idx,
            generation: self.generations[idx as usize],
        }
    }

    /// File the value `make` builds from the key it is to have, as slotmap's
    /// `try_insert_with_key`, so the value can name its own key; nothing where `make` fails.
    pub(crate) fn try_insert_with_key<E>(
        &mut self,
        make: impl FnOnce(SlabKey) -> Result<T, E>,
    ) -> Result<SlabKey, E> {
        let key = match self.free.last() {
            Some(&idx) => SlabKey {
                idx,
                generation: self.generations[idx as usize],
            },
            None => SlabKey {
                idx: u32::try_from(self.values.len()).expect("a slab counts its slots in u32"),
                generation: 0,
            },
        };
        let value = make(key)?;
        let filed = self.insert(value);
        debug_assert_eq!(
            filed, key,
            "the key a value was built for is the one it takes"
        );
        Ok(key)
    }

    /// Take `key`'s value out, freeing its slot and moving its generation; `None` where the key no
    /// longer resolves.
    ///
    /// # Panics
    ///
    /// When a slot's generation passes `u32::MAX`.
    pub(crate) fn remove(&mut self, key: SlabKey) -> Option<T> {
        let idx = self.live_idx(key)?;
        let value = self.values[idx].take();
        self.generations[idx] = self.generations[idx]
            .checked_add(1)
            .expect("a slot generation overflowed");
        self.free.push(key.idx);
        value
    }

    /// The slot `key` names, where its value is still there: its generation alone tells, as a
    /// removal moves it and an insert keeps it.
    fn live_idx(&self, key: SlabKey) -> Option<usize> {
        let idx = key.idx as usize;
        (self.generations.get(idx) == Some(&key.generation)).then_some(idx)
    }

    /// `key`'s value, `None` where it was removed.
    pub(crate) fn get(&self, key: SlabKey) -> Option<&T> {
        self.values[self.live_idx(key)?].as_ref()
    }

    /// `key`'s value, to change, `None` where it was removed.
    pub(crate) fn get_mut(&mut self, key: SlabKey) -> Option<&mut T> {
        let idx = self.live_idx(key)?;
        self.values[idx].as_mut()
    }

    /// The slots, occupied and vacant, which a walk by slot index spans.
    pub(crate) const fn slot_count(&self) -> usize {
        self.values.len()
    }

    /// The key of slot `idx`'s value, `None` where the slot is vacant or past the last.
    pub(crate) fn key_at(&self, idx: usize) -> Option<SlabKey> {
        self.values.get(idx)?.as_ref()?;
        Some(SlabKey {
            idx: u32::try_from(idx).expect("a slab counts its slots in u32"),
            generation: self.generations[idx],
        })
    }

    /// Slot `idx`'s value, `None` where the slot is vacant or past the last.
    pub(crate) fn at(&self, idx: usize) -> Option<&T> {
        self.values.get(idx)?.as_ref()
    }

    /// Slot `idx`'s value, to change, `None` where the slot is vacant or past the last.
    pub(crate) fn at_mut(&mut self, idx: usize) -> Option<&mut T> {
        self.values.get_mut(idx)?.as_mut()
    }

    /// Each value with its key, by slot.
    pub(crate) fn iter(&self) -> Iter<'_, T> {
        Iter {
            values: self.values.iter().zip(&self.generations).enumerate(),
        }
    }

    /// Each value, by slot.
    pub(crate) fn values(&self) -> impl Iterator<Item = &T> {
        self.values.iter().flatten()
    }
}

impl<'a, T> IntoIterator for &'a GenSlab<T> {
    type Item = (SlabKey, &'a T);
    type IntoIter = Iter<'a, T>;

    fn into_iter(self) -> Iter<'a, T> {
        self.iter()
    }
}

/// [`GenSlab::iter`]'s iterator: each value with its key, by slot.
#[derive(Debug)]
pub(crate) struct Iter<'a, T> {
    values: std::iter::Enumerate<
        std::iter::Zip<std::slice::Iter<'a, Option<T>>, std::slice::Iter<'a, u32>>,
    >,
}

impl<'a, T> Iterator for Iter<'a, T> {
    type Item = (SlabKey, &'a T);

    fn next(&mut self) -> Option<Self::Item> {
        self.values.find_map(|(idx, (value, &generation))| {
            Some((
                SlabKey {
                    idx: u32::try_from(idx).expect("a slab counts its slots in u32"),
                    generation,
                },
                value.as_ref()?,
            ))
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, self.values.size_hint().1)
    }
}

#[cfg(test)]
mod tests;
