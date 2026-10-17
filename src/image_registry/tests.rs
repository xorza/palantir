use crate::image_registry::ImageRegistry;
use crate::image_registry::image_handle::ImageHandle;
use crate::image_registry::image_store::ImageStore;
use crate::primitives::identity::texture_id::TextureId;
use crate::primitives::paint::image::Image;
use glam::UVec2;
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Call {
    Write(TextureId),
    Free(TextureId),
}

/// A store that keeps the order of the calls it took.
#[derive(Debug, Default)]
struct CallLog {
    calls: RefCell<Vec<Call>>,
}

impl ImageStore for CallLog {
    fn write(&self, id: TextureId, _: &Image) {
        self.calls.borrow_mut().push(Call::Write(id));
    }

    fn free(&self, id: TextureId) {
        self.calls.borrow_mut().push(Call::Free(id));
    }
}

/// Every handle of a registry and its clones writes, updates and frees through the one store,
/// in the order the handles ask.
#[test]
fn a_registry_and_its_clones_reach_one_store() {
    let store = Rc::new(CallLog::default());
    let registry = ImageRegistry::new(Rc::clone(&store));

    let image = Image::blank(UVec2::ONE);
    let first = ImageHandle::new(TextureId(7), &image, registry.clone());
    let second = ImageHandle::new(TextureId(8), &image, registry);
    drop(first);
    second.update(&image);
    drop(second);

    assert_eq!(
        *store.calls.borrow(),
        [
            Call::Write(TextureId(7)),
            Call::Write(TextureId(8)),
            Call::Free(TextureId(7)),
            Call::Write(TextureId(8)),
            Call::Free(TextureId(8)),
        ],
    );
}
