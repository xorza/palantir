//! Thin newtype around `wgpu::Queue` that, with the `internals` feature
//! enabled, tallies every per-frame `write_texture` call into the global
//! counters in [`crate::renderer::backend::write_stats`].
//!
//! Without `internals`, the wrapper is a zero-cost passthrough — the
//! shadowed method inlines straight into `wgpu::Queue::write_texture` and
//! `Deref<Target = wgpu::Queue>` covers everything else.

#[cfg(feature = "internals")]
use crate::renderer::backend::write_stats;
use std::ops::Deref;

/// Newtype owning a `wgpu::Queue`. Construct via `Queue::new` from a
/// `wgpu::Queue` handed in by the host; pass `&Queue` to pipelines
/// instead of `&wgpu::Queue`.
#[derive(Debug)]
pub(crate) struct Queue(wgpu::Queue);

impl Queue {
    pub(crate) fn new(inner: wgpu::Queue) -> Self {
        Self(inner)
    }

    /// Counted shadow of [`wgpu::Queue::write_texture`]. Records the
    /// length of the source byte slice as the upload size.
    #[inline]
    pub(crate) fn write_texture(
        &self,
        texture: wgpu::TexelCopyTextureInfo<'_>,
        data: &[u8],
        data_layout: wgpu::TexelCopyBufferLayout,
        size: wgpu::Extent3d,
    ) {
        #[cfg(feature = "internals")]
        write_stats::record_texture(data.len() as u64);
        self.0.write_texture(texture, data, data_layout, size);
    }
}

/// Everything else (`submit`, `on_submitted_work_done`, etc.) goes
/// straight through. `&Queue` deref-coerces to `&wgpu::Queue` so
/// occasional places that need the raw handle (e.g. handing to a
/// third-party API) keep working without a `.0`.
impl Deref for Queue {
    type Target = wgpu::Queue;
    fn deref(&self) -> &wgpu::Queue {
        &self.0
    }
}
