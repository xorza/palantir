//! [`MapState`]: where a staging buffer's read map stands.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

/// The status in a state's low bits; the map's generation sits above them.
const STATUS: u32 = 0b11;
const IDLE: u32 = 0;
const PENDING: u32 = 1;
const MAPPED: u32 = 2;
const FAILED: u32 = 3;
/// The generations a state counts before it wraps.
const GENERATIONS: u32 = u32::MAX >> 2;

/// Where a staging buffer's read map stands, kept with the buffer and reused by each of its maps,
/// so a map allocates nothing. Its callback writes it inside a device poll, on whichever thread
/// polls, so a reader that blocks waits for it through
/// [`Gpu::wait_until`](crate::gpu::device::gpu::Gpu::wait_until).
///
/// Each map is a generation of its own: the callback of an earlier map, as one an unmap aborted
/// that lands after the next map began, finds another generation and changes nothing.
#[derive(Debug)]
pub(crate) struct MapState(Arc<AtomicU32>);

impl MapState {
    /// No map in flight.
    pub(crate) fn new() -> Self {
        Self(Arc::new(AtomicU32::new(IDLE)))
    }

    /// Start mapping `buffer`, the one this state is kept with, for reading, once the submits
    /// that fill it are made.
    pub(crate) fn map_read(&self, buffer: &wgpu::Buffer) {
        let generation = ((self.0.load(Ordering::Acquire) >> 2) + 1) & GENERATIONS;
        let tag = generation << 2;
        self.0.store(tag | PENDING, Ordering::Release);
        let state = Arc::clone(&self.0);
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let done = if result.is_ok() { MAPPED } else { FAILED };
                // A failed exchange is a later map's state, which this callback must not touch.
                state
                    .compare_exchange(
                        tag | PENDING,
                        tag | done,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    )
                    .ok();
            });
    }

    /// Back to no map in flight, once the buffer was read and unmapped, or its map failed.
    pub(crate) fn clear(&self) {
        let generation = self.0.load(Ordering::Acquire) & !STATUS;
        self.0.store(generation | IDLE, Ordering::Release);
    }

    fn status(&self) -> u32 {
        self.0.load(Ordering::Acquire) & STATUS
    }

    /// Whether no map is in flight or landed: none began since the state was made or cleared.
    pub(crate) fn is_idle(&self) -> bool {
        self.status() == IDLE
    }

    /// Whether the map callback has yet to run. A device its owner destroyed runs it with a
    /// failure; one the driver lost may never run it, as wgpu-core's `maintain` returns before the
    /// mappings once its fence read fails. A reader that waits for it also checks
    /// [`Gpu::is_lost`](crate::gpu::device::gpu::Gpu::is_lost), as `wait_until` does.
    pub(crate) fn is_pending(&self) -> bool {
        self.status() == PENDING
    }

    /// Whether the callback ran and the buffer is mapped.
    pub(crate) fn is_mapped(&self) -> bool {
        self.status() == MAPPED
    }

    /// Whether the callback ran and the map failed, which only a lost device does.
    pub(crate) fn has_failed(&self) -> bool {
        self.status() == FAILED
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::gpu::surface::map_state::{FAILED, MAPPED, MapState, PENDING};
    use std::sync::Arc;
    use std::sync::atomic::AtomicU32;

    impl MapState {
        /// A map whose callback never runs, as one the device is still copying for.
        pub(crate) fn never_landing() -> Self {
            Self(Arc::new(AtomicU32::new(PENDING)))
        }

        /// A map whose callback ran: mapped, or failed.
        pub(crate) fn landed(mapped: bool) -> Self {
            Self(Arc::new(AtomicU32::new(if mapped {
                MAPPED
            } else {
                FAILED
            })))
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::gpu::device::gpu_wait::GpuWait;
    use crate::gpu::surface::map_state::MapState;
    use crate::gpu::test_gpu::headless_test_gpu;

    /// One state serves every map of its buffer: idle, pending, mapped, idle again once cleared,
    /// and mapped again by the next map. A map an unmap aborted lands as no later map: its
    /// callback finds the next generation and leaves it be.
    #[test]
    fn one_state_serves_each_map_of_its_buffer() {
        let gpu = headless_test_gpu();
        let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("palantir.test.map_state"),
            size: 8,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let map = MapState::new();
        assert!(map.is_idle());
        for _ in 0..2 {
            map.map_read(&buffer);
            assert!(map.is_pending());
            assert_eq!(gpu.wait_until(&|| !map.is_pending()), GpuWait::Done);
            assert!(map.is_mapped() && !map.has_failed());
            buffer.unmap();
            map.clear();
            assert!(map.is_idle());
        }

        map.map_read(&buffer);
        buffer.unmap();
        map.map_read(&buffer);
        assert_eq!(gpu.wait_until(&|| !map.is_pending()), GpuWait::Done);
        assert!(
            map.is_mapped(),
            "the aborted map's callback left the next one be"
        );
        buffer.unmap();
    }
}
