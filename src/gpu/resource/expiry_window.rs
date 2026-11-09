//! [`ExpiryWindow`]: the span over which a piece of per-frame GPU scratch keeps its peak.

/// The display frames an expiry window spans: at its end per-frame scratch steps down to the most
/// a frame of the window needed, and what no frame needed goes. Two seconds of a 60 Hz animation,
/// so a backdrop that closes and opens again within one gesture keeps its textures; a window that
/// paints no frame keeps them until it paints again, as no frame runs to shrink them. Expire after
/// idle, as Chromium's `cc::ResourcePool` and Flutter's `RasterCache` drop what recent frames did
/// not use. Counted in display frames ([`GpuCtx::frame`](crate::gpu::frame::gpu_ctx::GpuCtx)),
/// so a second window does not halve it.
pub(crate) const EXPIRY_FRAMES: u64 = 120;

/// One resource's current expiry window over the backend's display frames.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ExpiryWindow {
    /// The display frame the window began at; `None` before the resource's first frame.
    started: Option<u64>,
}

impl ExpiryWindow {
    /// A window that begins at the first frame [`Self::ends`] sees.
    pub(crate) const NEW: Self = Self { started: None };

    /// Whether `frame` is the last of the window's [`EXPIRY_FRAMES`]; the next window then begins
    /// past it. The first call begins the first window.
    pub(crate) fn ends(&mut self, frame: u64) -> bool {
        let started = *self.started.get_or_insert(frame);
        if frame.saturating_sub(started) + 1 < EXPIRY_FRAMES {
            return false;
        }
        self.started = Some(frame + 1);
        true
    }
}

impl Default for ExpiryWindow {
    fn default() -> Self {
        Self::NEW
    }
}

#[cfg(test)]
mod tests {
    use crate::gpu::resource::expiry_window::{EXPIRY_FRAMES, ExpiryWindow};

    /// A window of 120 frames from the first it saw, 10, ends at frame 129, and the next ends 120
    /// frames later, at 249; frames it skips count, as a window that paints none keeps aging.
    #[test]
    fn a_window_ends_every_expiry_frames_from_its_first() {
        let mut window = ExpiryWindow::default();
        assert!(!window.ends(10));
        assert!(!window.ends(10 + EXPIRY_FRAMES - 2));
        assert!(window.ends(10 + EXPIRY_FRAMES - 1));
        assert!(!window.ends(10 + EXPIRY_FRAMES));
        assert!(window.ends(10 + 2 * EXPIRY_FRAMES - 1));
        assert!(
            window.ends(10 + 5 * EXPIRY_FRAMES),
            "a frame past its end ends it"
        );
    }
}
