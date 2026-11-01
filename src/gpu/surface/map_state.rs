//! [`MapState`]: where a staging buffer's read map stands.

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

const PENDING: u8 = 0;
const MAPPED: u8 = 1;
const FAILED: u8 = 2;

/// Where a staging buffer's read map stands. Its callback writes it inside a device poll, on
/// whichever thread polls, so a reader that blocks waits for it through
/// [`Gpu::wait_until`](crate::gpu::device::gpu::Gpu::wait_until).
#[derive(Debug)]
pub(crate) struct MapState(Arc<AtomicU8>);

impl MapState {
    /// Start mapping `buffer` for reading, once the submits that fill it are made.
    pub(crate) fn map_read(buffer: &wgpu::Buffer) -> Self {
        let state = Arc::new(AtomicU8::new(PENDING));
        let landed = Arc::clone(&state);
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let done = if result.is_ok() { MAPPED } else { FAILED };
                landed.store(done, Ordering::Release);
            });
        Self(state)
    }

    /// Whether the map callback has yet to run. A lost device runs it too, with a failure.
    pub(crate) fn is_pending(&self) -> bool {
        self.0.load(Ordering::Acquire) == PENDING
    }

    /// Whether the callback ran and the buffer is mapped.
    pub(crate) fn is_mapped(&self) -> bool {
        self.0.load(Ordering::Acquire) == MAPPED
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::gpu::surface::map_state::{MapState, PENDING};
    use std::sync::Arc;
    use std::sync::atomic::AtomicU8;

    impl MapState {
        /// A map whose callback never runs, as one the device is still copying for.
        pub(crate) fn never_landing() -> Self {
            Self(Arc::new(AtomicU8::new(PENDING)))
        }
    }
}
