//! Shared cross-frame handle for CPU gradient registration and flushing.

use crate::primitives::paint::brush::gradient::color_ramp::ColorRamp;
use crate::primitives::paint::lut_row::LutRow;
use crate::renderer::gradient_atlas::{
    CpuGradientAtlas, DEFAULT_MAX_ATLAS_ROWS, FlushedRows, MAX_ATLAS_ROWS,
};
use crate::renderer::texture_limit::TextureLimit;
use std::cell::RefCell;
use std::num::NonZeroU32;
use std::rc::Rc;

#[derive(Clone, Debug, Default)]
pub(crate) struct SharedGradientAtlas {
    cpu: Rc<RefCell<CpuGradientAtlas>>,
}

impl SharedGradientAtlas {
    /// Atlas whose growth ceiling is the device's `max_texture_dimension_2d` (one LUT row per texture row), clamped by the [`MAX_ATLAS_ROWS`] policy ceiling since growth never reverses. `None` (deviceless) uses [`DEFAULT_MAX_ATLAS_ROWS`].
    pub(crate) fn new(texture_limit: TextureLimit) -> Self {
        let max_rows = texture_limit
            .max_dimension()
            .map_or(DEFAULT_MAX_ATLAS_ROWS, NonZeroU32::get)
            .min(MAX_ATLAS_ROWS);
        Self {
            cpu: Rc::new(RefCell::new(CpuGradientAtlas::new(max_rows))),
        }
    }

    /// Rows the atlas holds, the height the backend's LUT texture must match. Starts at [`INITIAL_ATLAS_ROWS`](crate::renderer::gradient_atlas::INITIAL_ATLAS_ROWS) and only grows.
    pub(crate) fn rows(&self) -> u32 {
        self.cpu.borrow().capacity()
    }

    #[inline]
    pub(crate) fn register(&self, ramp: &ColorRamp) -> LutRow {
        self.cpu.borrow_mut().register(ramp)
    }

    /// Hand this frame's dirty rows to `upload`, if any.
    #[inline]
    pub(crate) fn flush_with(&self, upload: impl FnOnce(FlushedRows<'_>)) {
        let mut atlas = self.cpu.borrow_mut();
        if let Some(rows) = atlas.flush() {
            upload(rows);
        }
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::common::counters::CounterSet;
    use crate::renderer::gradient_atlas::shared_gradient_atlas::SharedGradientAtlas;

    impl SharedGradientAtlas {
        /// Resolved growth ceiling, for the clamp test.
        pub(crate) fn max_rows(&self) -> u32 {
            self.cpu.borrow().max_rows()
        }

        /// `register` calls so far, letting resolver tests prove their per-pass memo suppresses repeats. Accumulates for the atlas's life; read a delta.
        pub(crate) fn registrations(&self) -> u32 {
            self.cpu.borrow().counters.counts().registrations
        }
    }
}
