use crate::icons::icon_atlas::IconAtlas;
use std::cell::RefCell;
use std::rc::Rc;

/// Identity of a loaded icon set — an index into [`IconRegistry`]'s table.
/// Half of an [`IconHandle`](crate::IconHandle), and half of the atlas cache
/// key, which is why it is a `u16` rather than a pointer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct IconSetId(pub(crate) u16);

/// The icon sets a host has loaded, shared between the `Ui` side that loads
/// them and the backend that rasterizes from them — the icon counterpart of
/// [`ImageRegistry`](crate::renderer::image_registry::ImageRegistry), and much
/// smaller, because a baked set is `'static` data with no GPU resource and no
/// lifecycle.
///
/// **Nothing is ever unloaded**, and that is what
/// [`Self::register`]'s identity check has to hold the line on: a caller
/// that hands over a freshly built [`IconAtlas`] every frame grows this
/// table every frame, and with it the backend's parsed-SVG cache and the
/// icon atlas's key space, none of which have any way to know the older
/// sets are dead. The panic at 65 536 sets is the backstop, not the
/// bound — memory goes long before it.
///
/// Single-threaded `Rc<RefCell<…>>`; cheap to clone, with shared inner state.
#[derive(Clone, Debug, Default)]
pub(crate) struct IconRegistry {
    sets: Rc<RefCell<Vec<Rc<IconAtlas>>>>,
}

impl IconRegistry {
    /// Register `atlas` and return its id. Registering the same `Rc` again
    /// hands back the same id rather than a second entry — identity is the
    /// allocation, so an immediate-mode caller can load on every frame from a
    /// handle it holds, at the cost of one refcount bump.
    ///
    /// # Panics
    ///
    /// Panics past 65 536 distinct sets, which would overflow
    /// [`IconSetId`]. A host loads a handful.
    pub(crate) fn register(&self, atlas: Rc<IconAtlas>) -> IconSetId {
        let mut sets = self.sets.borrow_mut();
        if let Some(i) = sets.iter().position(|s| Rc::ptr_eq(s, &atlas)) {
            return IconSetId(i as u16);
        }
        let id = u16::try_from(sets.len()).expect("more than 65536 icon sets registered");
        sets.push(atlas);
        IconSetId(id)
    }

    /// The set behind `id`, as a fresh handle — so the caller can read it
    /// without holding a borrow of the registry across the work.
    ///
    /// # Panics
    ///
    /// Panics on an id this registry never minted, which means a handle
    /// crossed between hosts.
    pub(crate) fn get(&self, id: IconSetId) -> Rc<IconAtlas> {
        let sets = self.sets.borrow();
        Rc::clone(
            sets.get(id.0 as usize)
                .unwrap_or_else(|| panic!("IconSetId({}) was not loaded by this host", id.0)),
        )
    }

    /// How many distinct sets are loaded.
    pub(crate) fn len(&self) -> usize {
        self.sets.borrow().len()
    }

    /// Every loaded set with the id it answers to. Collected rather than
    /// borrowed, so the caller can rasterize from each without holding the
    /// registry — and cheap for it, since each entry is one refcount bump and
    /// a host loads a handful of sets.
    pub(crate) fn sets(&self) -> Vec<(IconSetId, Rc<IconAtlas>)> {
        self.sets
            .borrow()
            .iter()
            .enumerate()
            .map(|(i, atlas)| (IconSetId(i as u16), Rc::clone(atlas)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use crate::icons::icon_atlas::{IconAtlas, IconDef, IconId};
    use crate::icons::icon_registry::{IconRegistry, IconSetId};
    use crate::primitives::span::Span;
    use glam::Vec2;
    use std::rc::Rc;

    const A_ICONS: &[IconDef] = &[IconDef {
        name: "a",
        view_box: Vec2::splat(24.0),
        svg: Span::new(0, 1),
        tintable: true,
        filtered: false,
    }];
    const B_ICONS: &[IconDef] = &[IconDef {
        name: "b",
        view_box: Vec2::splat(16.0),
        svg: Span::new(0, 1),
        tintable: false,
        filtered: true,
    }];
    fn a() -> Rc<IconAtlas> {
        Rc::new(IconAtlas::baked(A_ICONS, b"a"))
    }
    fn b() -> Rc<IconAtlas> {
        Rc::new(IconAtlas::baked(B_ICONS, b"b"))
    }

    #[test]
    fn reregistering_one_set_reuses_its_id_and_distinct_sets_do_not() {
        let reg = IconRegistry::default();
        assert_eq!(reg.len(), 0);
        let (set_a, set_b) = (a(), b());
        let ia = reg.register(Rc::clone(&set_a));
        let ib = reg.register(Rc::clone(&set_b));
        assert_eq!((ia, ib), (IconSetId(0), IconSetId(1)));
        // The same handle again: same id, no second entry. An immediate-mode
        // caller loading every frame must not grow the table.
        assert_eq!(reg.register(Rc::clone(&set_a)), ia);
        assert_eq!(reg.register(Rc::clone(&set_b)), ib);
        assert_eq!(reg.len(), 2);
        assert_eq!(reg.get(ia).icons()[0].name, "a");
        assert_eq!(reg.get(ib).icons()[0].name, "b");

        // A separate allocation over identical data is a separate set: the
        // registry keys on identity, not on contents.
        assert_eq!(reg.register(a()), IconSetId(2));
    }

    #[test]
    fn clones_share_one_table() {
        let reg = IconRegistry::default();
        let clone = reg.clone();
        let id = reg.register(a());
        assert_eq!(clone.len(), 1, "the backend's clone sees the Ui's load");
        assert_eq!(clone.get(id).icons()[0].view_box, Vec2::splat(24.0));
    }

    #[test]
    #[should_panic(expected = "was not loaded by this host")]
    fn unknown_set_id_panics() {
        let _ = IconRegistry::default().get(IconSetId(3));
    }

    /// The def accessor is what turns a stray id into a loud failure rather
    /// than a wrong icon.
    #[test]
    #[should_panic(expected = "is not in this set")]
    fn out_of_range_icon_id_panics() {
        let _ = a().def(IconId(1));
    }
}
