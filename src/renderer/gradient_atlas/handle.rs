//! Shared cross-frame handle for CPU gradient registration and flushing.

use crate::primitives::brush::{Interp, Stop};
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
    pub(crate) fn register_stops(&self, stops: &[Stop], interp: Interp) -> LutRow {
        self.inner.borrow_mut().register_stops(stops, interp)
    }

    #[inline]
    pub(crate) fn flush_with<R>(&self, upload: impl FnOnce(FlushedRows<'_>) -> R) -> Option<R> {
        let mut atlas = self.inner.borrow_mut();
        atlas.flush().map(upload)
    }
}
