//! Shared cross-frame handle for CPU gradient registration and flushing.

use crate::primitives::brush::{GradientStops, Interp};
use crate::primitives::fill_wire::LutRow;
use crate::renderer::gradient_atlas::{FlushedRows, GradientCpuAtlas};
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Clone, Debug, Default)]
pub(crate) struct GradientAtlas {
    inner: Rc<RefCell<GradientCpuAtlas>>,
}

impl GradientAtlas {
    #[inline]
    pub(crate) fn register_stops(&self, stops: &GradientStops, interp: Interp) -> LutRow {
        self.inner.borrow_mut().register_stops(stops, interp)
    }

    #[inline]
    pub(crate) fn flush_with<R>(&self, upload: impl FnOnce(FlushedRows<'_>) -> R) -> Option<R> {
        let mut atlas = self.inner.borrow_mut();
        atlas.flush().map(upload)
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use crate::renderer::gradient_atlas::handle::GradientAtlas;

    pub(crate) fn registration_count(atlas: &GradientAtlas) -> u64 {
        atlas.inner.borrow().clock
    }
}
