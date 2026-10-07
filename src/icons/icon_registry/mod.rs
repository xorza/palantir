//! The table of loaded icon sets, and the generational ids that stop a released slot from aliasing the next set loaded into it.

use crate::icons::icon_set::IconSet;
use crate::icons::icon_table::IconTable;
use std::cell::RefCell;
use std::fmt;
use std::rc::{Rc, Weak};

/// Identity of a loaded icon set: a slot of [`IconRegistry`]'s table plus the occupant's generation. Half of an [`IconHandle`](crate::IconHandle) and of the atlas cache key, hence four bytes.
///
/// The generation makes a slot reusable: without it a `Copy` handle minted before a release would silently name the next set. Sixteen bits wrap after 65 536 cycles of one slot, giving a wrong icon, not unsoundness.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct IconSetId {
    slot: u16,
    generation: u16,
}

impl IconSetId {
    pub(crate) const fn new(slot: u16, generation: u16) -> Self {
        Self { slot, generation }
    }

    pub(crate) const fn bits(self) -> u32 {
        self.slot as u32 | ((self.generation as u32) << 16)
    }
}

/// RAII core of an [`IconSet`]: its [`Drop`] is the whole unload path. When the last clone goes the id is queued, and the next [`IconRegistry::drain_released`] frees the slot and tells the backend to forget it.
pub(crate) struct IconSetToken {
    id: IconSetId,
    table: Rc<IconTable>,
    shared: Rc<RefCell<Inner>>,
}

/// Summarized: `shared` is the whole registry and `table` every icon's bytes.
impl fmt::Debug for IconSetToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IconSetToken")
            .field("id", &self.id)
            .field("icons", &self.table.icons().len())
            .finish_non_exhaustive()
    }
}

impl IconSetToken {
    pub(crate) const fn id(&self) -> IconSetId {
        self.id
    }

    pub(crate) fn table(&self) -> &IconTable {
        &self.table
    }
}

impl Drop for IconSetToken {
    fn drop(&mut self) {
        let mut inner = self.shared.borrow_mut();
        inner.released.push(self.id);
        inner.epoch += 1;
    }
}

/// One row of the table. `atlas` is `None` from the drain that freed it until the next load.
#[derive(Debug)]
struct Slot {
    generation: u16,
    /// Held for the backend, which resolves a set from an id; dropped at the drain.
    table: Option<Rc<IconTable>>,
    /// The live [`IconSetToken`] over this slot; two owners would free it on the first drop.
    token: Weak<IconSetToken>,
}

#[derive(Debug, Default)]
struct Inner {
    slots: Vec<Slot>,
    free: Vec<u16>,
    /// Ids whose last [`IconSet`] clone dropped since the last drain.
    released: Vec<IconSetId>,
    epoch: u64,
}

/// The icon sets a host has loaded, shared between the `Ui` and the backend that rasterizes from them.
///
/// The counterpart of [`ImageRegistry`](crate::renderer::image_registry::ImageRegistry): **the caller's handle keeps the resource alive.** [`Self::register`] returns an [`IconSet`]; dropping the last clone queues the id and [`Self::drain_released`] frees the slot. There is no `unload`.
///
/// Unlike a never-reused `TextureId`, an `IconSetId` is a reusable table slot (it must fit beside an icon index in a cache key), so the table owns the row and stamps a generation. A set's release is also a family of keys found only by walking each backend store, so it is queued for one batched walk rather than done in `Drop`.
///
/// Single-threaded `Rc<RefCell<…>>`; cheap to clone.
#[derive(Clone, Debug, Default)]
pub(crate) struct IconRegistry {
    inner: Rc<RefCell<Inner>>,
}

impl IconRegistry {
    /// Load `table` and return the [`IconSet`] that keeps it resident.
    ///
    /// Registering an allocation a live `IconSet` covers returns a clone of it, so loading every frame from a held `Rc` costs one refcount bump. Resident sets are scanned linearly for that match, so load a handful.
    ///
    /// # Panics
    ///
    /// Panics past 65 536 sets resident at once.
    pub(crate) fn register(&self, table: Rc<IconTable>) -> IconSet {
        let mut inner = self.inner.borrow_mut();
        let resident = inner.slots.iter().find_map(|slot| {
            let held = slot.table.as_ref()?;
            if !Rc::ptr_eq(held, &table) {
                return None;
            }
            // A slot whose token has gone is released but not yet drained; it can't be shared, so a fresh slot is taken while the drain frees that one.
            slot.token.upgrade()
        });
        if let Some(token) = resident {
            return IconSet::from_token(token);
        }

        let slot = if let Some(slot) = inner.free.pop() {
            slot
        } else {
            let slot = u16::try_from(inner.slots.len())
                .expect("more than 65536 icon sets resident at once");
            inner.slots.push(Slot {
                generation: 0,
                table: None,
                token: Weak::new(),
            });
            slot
        };
        let id = IconSetId::new(slot, inner.slots[slot as usize].generation);
        let token = Rc::new(IconSetToken {
            id,
            table: Rc::clone(&table),
            shared: Rc::clone(&self.inner),
        });
        let row = &mut inner.slots[slot as usize];
        row.table = Some(table);
        row.token = Rc::downgrade(&token);
        inner.epoch += 1;
        IconSet::from_token(token)
    }

    /// The set behind `id`.
    ///
    /// # Panics
    ///
    /// Panics on an id never minted or whose set was released: caller logic errors a silent skip would hide.
    pub(crate) fn get(&self, id: IconSetId) -> Rc<IconTable> {
        let inner = self.inner.borrow();
        let table = inner
            .slots
            .get(id.slot as usize)
            .filter(|slot| slot.generation == id.generation)
            .and_then(|slot| slot.table.as_ref());
        Rc::clone(table.unwrap_or_else(|| {
            panic!(
                "icon set {}.{} is not loaded — an IconHandle outlived every \
                 IconSet holding its set, or crossed between hosts",
                id.slot, id.generation,
            )
        }))
    }

    /// Free released sets' slots, then hand `forget` all their ids at once (the backend walks each store in full). Not called when nothing was released. `forget` must not re-enter the registry, whose borrow is held.
    pub(crate) fn drain_released(&self, forget: impl FnOnce(&[IconSetId])) {
        let mut inner = self.inner.borrow_mut();
        let Inner {
            slots,
            free,
            released,
            ..
        } = &mut *inner;
        if released.is_empty() {
            return;
        }
        for &id in released.iter() {
            let slot = &mut slots[id.slot as usize];
            debug_assert_eq!(slot.generation, id.generation, "double release");
            slot.table = None;
            slot.generation = slot.generation.wrapping_add(1);
            free.push(id.slot);
        }
        forget(released);
        released.clear();
    }

    /// Counts every load and release, so a consumer caching per-set work can tell a swapped set from the same ones; a resident count can't (load, release, load leaves it unchanged).
    pub(crate) fn epoch(&self) -> u64 {
        self.inner.borrow().epoch
    }

    /// The set in table slot `slot`, or `None` where the slot is empty, released, or past the end. Indexed rather than iterated so a caller rasterizing through `&mut self` holds no registry borrow, without collecting into a `Vec`.
    pub(crate) fn resident(&self, slot: usize) -> Option<ResidentIconSet> {
        let inner = self.inner.borrow();
        let row = inner.slots.get(slot)?;
        if row.token.strong_count() == 0 {
            return None;
        }
        Some(ResidentIconSet {
            id: IconSetId::new(slot as u16, row.generation),
            table: Rc::clone(row.table.as_ref()?),
        })
    }

    pub(crate) fn slot_count(&self) -> usize {
        self.inner.borrow().slots.len()
    }
}

/// One loaded set as [`IconRegistry::resident`] hands it out: the id a raster key carries, and the table the bytes come from.
#[derive(Clone, Debug)]
pub(crate) struct ResidentIconSet {
    pub(crate) id: IconSetId,
    pub(crate) table: Rc<IconTable>,
}

#[cfg(test)]
mod tests;
