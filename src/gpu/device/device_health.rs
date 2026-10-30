//! Whether a device still draws, as its lost callback reports.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Set once, by the device's lost callback, for any reason; shared by every clone of a `Gpu`. A
/// `Destroyed` loss counts too: only an app or a test destroys a device, and either way it no
/// longer draws.
#[derive(Clone, Debug, Default)]
pub(crate) struct DeviceHealth(Arc<AtomicBool>);

impl DeviceHealth {
    /// The health of `device`. Installs its lost callback, and an uncaptured-error handler that
    /// panics as wgpu's default does while the device is healthy and drops the error once it is
    /// lost: the error of a call on an object made after the loss is no bug of the caller's.
    #[expect(
        clippy::disallowed_methods,
        reason = "the one place a device's lost callback and error handler are set"
    )]
    pub(crate) fn watch(device: &wgpu::Device) -> Self {
        let health = Self::default();
        let lost = Arc::clone(&health.0);
        device.set_device_lost_callback(move |reason, message| {
            tracing::error!(?reason, %message, "graphics device lost");
            lost.store(true, Ordering::Release);
        });
        let lost = Arc::clone(&health.0);
        device.on_uncaptured_error(Arc::new(move |error: wgpu::Error| {
            assert!(lost.load(Ordering::Acquire), "wgpu error: {error}");
            tracing::debug!(%error, "dropped: the device is lost");
        }));
        health
    }

    pub(crate) fn is_lost(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}
