//! The table of loaded icon sets, and the generational ids that stop a
//! released slot from aliasing the next set loaded into it.

use crate::icons::icon_set::IconSet;
use crate::icons::icon_table::IconTable;
use std::cell::RefCell;
use std::rc::{Rc, Weak};

/// Identity of a loaded icon set: which slot of [`IconRegistry`]'s table,
/// and which occupant of it. Half of an [`IconHandle`](crate::IconHandle),
/// and half of the atlas cache key, which is why it is four bytes rather
/// than a pointer.
///
/// The generation is what makes a slot reusable. Without it a released
/// slot's index would be handed to the next set loaded, and an
/// [`IconHandle`](crate::IconHandle) — `Copy`, owning nothing — minted
/// before the release would silently name the new set's icon. Sixteen bits
/// wrap after 65 536 load/release cycles *of the same slot*, and a handle
/// would have to survive all of them to alias; the consequence there is a
/// wrong icon, not unsoundness, so the width buys diagnosis rather than
/// safety.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct IconSetId {
    slot: u16,
    generation: u16,
}

impl IconSetId {
    pub(crate) const fn new(slot: u16, generation: u16) -> Self {
        Self { slot, generation }
    }

    /// Both halves in one word, for a content hash that wants a single
    /// write rather than a field-by-field `Hash`.
    pub(crate) const fn bits(self) -> u32 {
        self.slot as u32 | ((self.generation as u32) << 16)
    }
}

/// RAII core of an [`IconSet`]: the set's data, its id, and the shared
/// table to hand the slot back to.
///
/// Its [`Drop`] is the whole unload path — when the last `IconSet` clone
/// goes away the id is queued, and the next
/// [`IconRegistry::drain_released`] frees the slot and tells the backend
/// to forget what it cached against it.
pub(crate) struct IconSetToken {
    id: IconSetId,
    table: Rc<IconTable>,
    shared: Rc<RefCell<Inner>>,
}

/// Summarized for the reason [`IconSet`]'s own impl states: `shared` is the
/// whole registry, and `table` is every icon's bytes.
impl std::fmt::Debug for IconSetToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IconSetToken")
            .field("id", &self.id)
            .field("icons", &self.table.icons().len())
            .finish_non_exhaustive()
    }
}

impl IconSetToken {
    pub(crate) fn id(&self) -> IconSetId {
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
        // A release changes what a prewarm pass would cover just as much
        // as a load does — see [`IconRegistry::epoch`].
        inner.epoch += 1;
    }
}

/// One row of the table. Occupied while a set is loaded; `atlas` is `None`
/// from the drain that freed it until the next load takes it.
#[derive(Debug)]
struct Slot {
    /// Bumped when the slot is freed, so every id minted from it before
    /// then stops matching.
    generation: u16,
    /// Held for the backend, which resolves a set from an id rather than
    /// from the caller's `IconSet` — it has no access to one. Dropped at
    /// the drain, which is what frees the SVG blob.
    table: Option<Rc<IconTable>>,
    /// The live [`IconSetToken`] over this slot, so re-registering the
    /// same allocation shares it instead of minting a second owner — two
    /// owners of one slot would free it on the first drop.
    token: Weak<IconSetToken>,
}

#[derive(Debug, Default)]
struct Inner {
    slots: Vec<Slot>,
    /// Slot indices freed by [`IconRegistry::drain_released`], reused by
    /// the next load.
    free: Vec<u16>,
    /// Ids whose last [`IconSet`] clone dropped since the last drain.
    released: Vec<IconSetId>,
    epoch: u64,
}

/// The icon sets a host has loaded, shared between the `Ui` side that
/// loads them and the backend that rasterizes from them.
///
/// The icon counterpart of
/// [`ImageRegistry`](crate::renderer::image_registry::ImageRegistry), and
/// it runs the same lifecycle for the same reason: **the handle the caller
/// holds is what keeps the resource alive.** [`Self::register`] hands back
/// an [`IconSet`] owning a shared [`IconSetToken`]; dropping the last clone
/// queues the id, and [`Self::drain_released`] frees the slot and lets the
/// backend drop what it cached against it. There is no `unload` — the
/// `IconSet`'s lifetime *is* the set's lifetime.
///
/// The two registries differ in where the resource lives, and that follows
/// from what the id is. A `TextureId` is minted monotonically and never
/// reused, so an image can leave its GPU texture in the backend's map keyed
/// by id. An `IconSetId` is a table slot, because it has to fit beside an
/// icon index in a cache key — so it must be reusable, so the table has to
/// own the row and stamp it with a generation.
///
/// That difference reaches the release path too, which is why the two are
/// not the same shape. An image's release is one hash removal from a map
/// keyed by its own id, so the handle's `Drop` performs it there and then
/// and `ImageRegistry` queues nothing — there is no image drain. A set's
/// release is a whole *family* of keys — every parse, every packed
/// raster — that only a full walk of each store can find, so it is queued
/// instead: [`Self::drain_released`] hands the whole batch over and the
/// backend walks once.
///
/// Single-threaded `Rc<RefCell<…>>`; cheap to clone, with shared inner state.
#[derive(Clone, Debug, Default)]
pub(crate) struct IconRegistry {
    inner: Rc<RefCell<Inner>>,
}

impl IconRegistry {
    /// Load `table` and return the [`IconSet`] that keeps it resident.
    ///
    /// Registering an allocation a live `IconSet` already covers hands back
    /// a clone of that same set rather than a second entry — identity is
    /// the allocation, so an immediate-mode caller can load on every frame
    /// from an `Rc` it holds, at the cost of one refcount bump. Handing
    /// over a *freshly built* atlas every frame is a different thing and
    /// loads a set every frame; each is released as its `IconSet` drops,
    /// so the table cycles one slot rather than growing.
    ///
    /// Resident sets are scanned linearly for the `Rc` match, so "a host
    /// loads a handful" is a real constraint and not only a slot-width
    /// one — the panic below is three orders of magnitude past where the
    /// scan stops being free.
    ///
    /// # Panics
    ///
    /// Panics past 65 536 sets resident **at once**, which would overflow a
    /// slot index.
    pub(crate) fn register(&self, table: Rc<IconTable>) -> IconSet {
        let mut inner = self.inner.borrow_mut();
        let resident = inner.slots.iter().find_map(|slot| {
            let held = slot.table.as_ref()?;
            if !Rc::ptr_eq(held, &table) {
                return None;
            }
            // A slot whose token has already gone is released-but-not-yet
            // drained; it cannot be shared, so this falls through and takes
            // a fresh slot while the drain frees that one.
            slot.token.upgrade()
        });
        if let Some(token) = resident {
            return IconSet::from_token(token);
        }

        let slot = match inner.free.pop() {
            Some(slot) => slot,
            None => {
                let slot = u16::try_from(inner.slots.len())
                    .expect("more than 65536 icon sets resident at once");
                inner.slots.push(Slot {
                    generation: 0,
                    table: None,
                    token: Weak::new(),
                });
                slot
            }
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
    /// Panics on an id this registry never minted, or one whose set has
    /// been released — an [`IconHandle`](crate::IconHandle) outliving every
    /// `IconSet` it came from. Both are caller logic errors, and a silently
    /// skipped icon would hide them until someone noticed a hole in a
    /// toolbar.
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

    /// Free the slots of sets released since the last drain, then hand
    /// `forget` **all** of their ids at once so the backend can drop what
    /// it cached against them.
    ///
    /// One call, not one per id: everything the backend keys on a set —
    /// parsed SVGs, packed rasters — is a store it has to walk in full to
    /// find a doomed family in, so a per-id callback made unloading N
    /// sets cost N walks of each. Not called at all when nothing was
    /// released, which is every frame but the rare one.
    ///
    /// The registry borrow is held across `forget`, so the closure must not
    /// re-enter the registry — the backend's does not, and nothing else
    /// drains.
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
            // One token per slot, and only its drop queues an id, so the
            // stamp cannot have moved since.
            debug_assert_eq!(slot.generation, id.generation, "double release");
            slot.table = None;
            slot.generation = slot.generation.wrapping_add(1);
            free.push(id.slot);
        }
        // Drained after the callback rather than during, because
        // `forget` is handed the batch as a slice.
        forget(released);
        released.clear();
    }

    /// Counts every load and every release, so a consumer that caches
    /// per-set work can tell "the same sets as last time" from "a set was
    /// swapped for another that happens to reuse its slot".
    ///
    /// A count of resident sets could not: load, release, load leaves it
    /// unchanged while the content is entirely different.
    pub(crate) fn epoch(&self) -> u64 {
        self.inner.borrow().epoch
    }

    /// The set in table slot `slot` — `None` where the slot is free or
    /// past the table's end.
    ///
    /// Indexed rather than iterated, because its one caller rasterizes
    /// from each set through `&mut self` and so cannot hold a borrow of
    /// the registry across the walk. Collecting the table into a `Vec`
    /// was the other way to break that borrow; this is the one that
    /// allocates nothing.
    ///
    /// A slot past the end answering `None` is what lets a caller walk
    /// `0..slot_count()` without this promising the count holds still.
    pub(crate) fn resident(&self, slot: usize) -> Option<ResidentIconSet> {
        let inner = self.inner.borrow();
        let row = inner.slots.get(slot)?;
        Some(ResidentIconSet {
            id: IconSetId::new(slot as u16, row.generation),
            table: Rc::clone(row.table.as_ref()?),
        })
    }

    /// Slots the table holds — the exclusive bound for
    /// [`Self::resident`].
    pub(crate) fn slot_count(&self) -> usize {
        self.inner.borrow().slots.len()
    }
}

/// One loaded set as [`IconRegistry::resident`] hands it out: the id a
/// raster key carries, and the table the bytes come from.
#[derive(Clone, Debug)]
pub(crate) struct ResidentIconSet {
    pub(crate) id: IconSetId,
    pub(crate) table: Rc<IconTable>,
}

#[cfg(test)]
mod tests;
