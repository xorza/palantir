//! Shared cross-frame handle for CPU gradient registration and flushing.

use crate::primitives::brush::gradient::Interp;
use crate::primitives::brush::gradient::stops::GradientStops;
use crate::primitives::fill_wire::LutRow;
use crate::renderer::gradient_atlas::{CpuGradientAtlas, DEFAULT_MAX_ATLAS_ROWS, FlushedRows};
use std::cell::RefCell;
use std::num::NonZeroU32;
use std::rc::Rc;

#[derive(Clone, Debug, Default)]
pub(crate) struct SharedGradientAtlas {
    cpu: Rc<RefCell<CpuGradientAtlas>>,
}

impl SharedGradientAtlas {
    /// Atlas whose growth ceiling is the device's
    /// `max_texture_dimension_2d` — one LUT row is one texture row, so
    /// that limit is the real bound. `None` (deviceless tests, benches)
    /// falls back to [`DEFAULT_MAX_ATLAS_ROWS`].
    pub(crate) fn new(max_texture_dimension_2d: Option<NonZeroU32>) -> Self {
        let max_rows = max_texture_dimension_2d.map_or(DEFAULT_MAX_ATLAS_ROWS, NonZeroU32::get);
        Self {
            cpu: Rc::new(RefCell::new(CpuGradientAtlas::new(max_rows))),
        }
    }

    /// Rows the atlas currently holds — the height the backend's LUT
    /// texture must match. Starts at
    /// [`INITIAL_ATLAS_ROWS`](crate::renderer::gradient_atlas::INITIAL_ATLAS_ROWS)
    /// and only
    /// ever grows.
    pub(crate) fn rows(&self) -> u32 {
        self.cpu.borrow().capacity()
    }

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
pub(crate) mod internals {
    use crate::renderer::gradient_atlas::handle::SharedGradientAtlas;

    pub(crate) fn registration_count(atlas: &SharedGradientAtlas) -> u64 {
        atlas.cpu.borrow().clock
    }
}
