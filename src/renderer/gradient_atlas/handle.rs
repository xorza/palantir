//! Shared cross-frame handle for CPU gradient registration and flushing.

use crate::primitives::brush::gradient::Interp;
use crate::primitives::brush::gradient::stops::GradientStops;
use crate::primitives::fill_wire::LutRow;
use crate::renderer::gradient_atlas::{CpuGradientAtlas, FlushedRows};
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Clone, Debug, Default)]
pub(crate) struct SharedGradientAtlas {
    cpu: Rc<RefCell<CpuGradientAtlas>>,
}

impl SharedGradientAtlas {
    #[inline]
    pub(crate) fn register_stops(&self, stops: &GradientStops, interp: Interp) -> LutRow {
        self.cpu.borrow_mut().register_stops(stops, interp)
    }

    #[inline]
    pub(crate) fn flush_with<R>(&self, upload: impl FnOnce(FlushedRows<'_>) -> R) -> Option<R> {
        let mut atlas = self.cpu.borrow_mut();
        atlas.flush().map(upload)
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use crate::renderer::gradient_atlas::handle::SharedGradientAtlas;

    pub(crate) fn registration_count(atlas: &SharedGradientAtlas) -> u64 {
        atlas.cpu.borrow().clock
    }
}
