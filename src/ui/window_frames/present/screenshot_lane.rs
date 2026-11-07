//! [`ScreenshotLane`]: a window's screenshots, from the ask to the image.

use crate::display::Display;
use crate::gpu::surface::capture::Capture;
use crate::gpu::wgpu_backend::WgpuBackend;
use crate::gpu::wgpu_backend::render_stream::RenderStream;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::urect::URect;
use crate::window::screenshot::{Screenshot, ScreenshotRequest};
use glam::UVec2;
use std::collections::VecDeque;

/// The physical pixels of a logical `region` of `display`: edges rounded out, so every pixel
/// the region touches is in, and held inside the surface.
#[expect(
    clippy::cast_sign_loss,
    reason = "each coordinate is floored at zero first, and `as` saturates the far side"
)]
fn physical_region(region: Rect, display: Display) -> URect {
    let scale = display.scale_factor();
    let max = region.max();
    let bound = display.physical;
    let corner = |x: f32, y: f32| {
        UVec2::new(
            (x.max(0.0) as u32).min(bound.x),
            (y.max(0.0) as u32).min(bound.y),
        )
    };
    let min = corner(
        (region.min.x * scale).floor(),
        (region.min.y * scale).floor(),
    );
    let max = corner((max.x * scale).ceil(), (max.y * scale).ceil());
    URect::from_min_max(min, max.max(min))
}

/// A [`Capture`] with the request it answers, its region clamped to the window it was taken
/// from.
#[derive(Debug)]
pub(super) struct PendingScreenshot {
    pub(super) capture: Capture,
    pub(super) request: ScreenshotRequest,
    pub(super) scale: f32,
}

/// Where each screenshot of a window stands: asked and not yet painted, or on its way back from
/// the device.
#[derive(Debug, Default)]
pub(crate) struct ScreenshotLane {
    /// The images a recorded frame asked for, until a frame paints them: ones the host did not
    /// present (an outdated swapchain) carry to the next, and are resolved against the display of
    /// the frame that paints them, so a resize in between cannot leave one outside.
    pub(super) requests: Vec<ScreenshotRequest>,
    /// Screenshots on their way back from the device, oldest first; one lands per frame.
    pub(super) pending: VecDeque<PendingScreenshot>,
}

impl ScreenshotLane {
    /// Whether the frame about to paint takes a screenshot.
    pub(super) const fn is_capturing(&self) -> bool {
        !self.requests.is_empty()
    }

    /// Whether a screenshot is on its way back from the device, which a later frame lands.
    pub(super) fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    /// Whether the device's copy of every screenshot owed has landed, or failed, so the next frame
    /// hands the oldest over.
    pub(super) fn all_landed(&self) -> bool {
        self.pending
            .iter()
            .all(|pending| !pending.capture.is_pending())
    }

    /// Whether the window owes a frame for a screenshot, as nothing else wakes the host when one
    /// lands.
    pub(super) fn owes_frame(&self) -> bool {
        self.is_capturing() || self.has_pending()
    }

    /// What the window kept on an earlier device is gone with it: each screenshot on its way back
    /// from that device goes back to the requests, ahead of the asks queued since.
    pub(super) fn requeue_pending(&mut self) {
        self.requests
            .splice(0..0, self.pending.drain(..).map(|pending| pending.request));
    }

    /// The oldest screenshot the device has finished, or none. A copy the device failed, as only a
    /// lost one does, goes back to the requests, and the next frame that paints takes it again; a
    /// copy a lost device never finishes goes back when a new device attaches.
    pub(super) fn take_landed(&mut self, backend: &WgpuBackend) -> Option<Screenshot> {
        if !self.pending.is_empty() {
            backend.poll();
        }
        // Each failed ask is older than the asks queued since, so it goes ahead of them, in order.
        let mut requeued = 0;
        while self
            .pending
            .front()
            .is_some_and(|pending| !pending.capture.is_pending())
        {
            let PendingScreenshot {
                capture,
                request,
                scale,
            } = self.pending.pop_front().expect("the front one landed");
            if let Some(image) = capture.take() {
                return Some(Screenshot::new(image, scale, request.region, request.id));
            }
            self.requests.insert(requeued, request);
            requeued += 1;
        }
        None
    }

    /// Copy each image asked for out of the frame `stream` just painted on `display`.
    pub(super) fn capture_all(
        &mut self,
        backend: &mut WgpuBackend,
        stream: &RenderStream,
        display: Display,
    ) {
        for request in self.requests.drain(..) {
            let region = request.region.clamp_to(display.logical_rect());
            // An ask made between frames, before the window shrank past its region, has
            // no pixel left to copy: no image lands for it, as for an empty ask.
            if region.is_paint_empty() {
                continue;
            }
            self.pending.push_back(PendingScreenshot {
                capture: backend.capture(stream, physical_region(region, display)),
                request: ScreenshotRequest {
                    region,
                    id: request.id,
                },
                scale: display.scale_factor(),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::gpu::surface::capture::Capture;
    use crate::gpu::test_gpu::headless_test_gpu;
    use crate::primitives::geometry::rect::Rect;
    use crate::ui::window_frames::present::screenshot_lane::{PendingScreenshot, ScreenshotLane};
    use crate::window::screenshot::{ScreenshotId, ScreenshotRequest};

    fn request(id: ScreenshotId) -> ScreenshotRequest {
        ScreenshotRequest {
            region: Rect::new(0.0, 0.0, 1.0, 1.0),
            id,
        }
    }

    /// An ask owes a frame until a frame paints it, and a copy until it lands: each alone does. A
    /// loss sends each copy back ahead of the asks queued since, oldest first: copies 1 and 2 of
    /// asks 1, 2 and 3 leave the asks in that order, with nothing left on the device.
    #[test]
    fn a_screenshot_owes_a_frame_and_a_loss_requeues_it_first() {
        let gpu = headless_test_gpu();
        let pending = |id| PendingScreenshot {
            capture: Capture::never_landing(&gpu),
            request: request(id),
            scale: 1.0,
        };
        let first = ScreenshotId::FIRST;
        let second = first.next();
        let third = second.next();

        let mut lane = ScreenshotLane::default();
        assert!(!lane.owes_frame());
        assert!(lane.all_landed(), "no copy is owed");

        lane.requests.push(request(third));
        assert!(lane.is_capturing());
        assert!(!lane.has_pending());
        assert!(lane.owes_frame());
        assert!(lane.all_landed());

        lane.pending.push_back(pending(first));
        lane.pending.push_back(pending(second));
        assert!(lane.has_pending());
        assert!(!lane.all_landed());
        lane.requests.clear();
        assert!(!lane.is_capturing());
        assert!(lane.owes_frame());

        lane.requests.push(request(third));
        lane.requeue_pending();
        assert_eq!(
            lane.requests,
            [request(first), request(second), request(third)]
        );
        assert!(!lane.has_pending());
    }
}
