//! [`DisplayFrames`]: the backend's frame clock across the windows it draws.

use crate::gpu::render_owner_id::RenderOwnerId;

/// The backend's display frame: it moves on when an owner submits a second time since it last
/// moved, so a round in which each live window submits once is one frame, whatever the number of
/// windows. What decays by frames ([`ExpiryWindow`](crate::common::expiry_window::ExpiryWindow))
/// keeps its two seconds with two windows open. Refused: a count of submits, which N windows
/// at one rate run N times as fast; a clock of time, which a test cannot step; one clock with the
/// shaper's (`FrameRuntime::tick_text_clock`), as two hosts may share a shaper and not a backend.
#[derive(Debug, Default)]
pub(crate) struct DisplayFrames {
    frame: u64,
    /// The owners that submitted in this frame; bounded by the live owners.
    submitted: Vec<RenderOwnerId>,
}

impl DisplayFrames {
    /// The frame `owner`'s next submit belongs to, the next where `owner` submitted in this one
    /// already, which its build reads before the submit moves the clock.
    pub(crate) fn next(&self, owner: RenderOwnerId) -> u64 {
        self.frame + u64::from(self.submitted.contains(&owner))
    }

    /// The frame `owner`'s submit belongs to, [`Self::next`], moving the clock on to it.
    pub(crate) fn advance(&mut self, owner: RenderOwnerId) -> u64 {
        let frame = self.next(owner);
        if frame != self.frame {
            self.frame = frame;
            self.submitted.clear();
        }
        self.submitted.push(owner);
        self.frame
    }

    /// Forget `owner`, whose window closed, so the frame no longer waits on it.
    pub(crate) fn retire(&mut self, owner: RenderOwnerId) {
        self.submitted.retain(|submitted| *submitted != owner);
    }
}

#[cfg(test)]
mod tests {
    use crate::gpu::render_owner_id::RenderOwners;
    use crate::gpu::wgpu_backend::display_frames::DisplayFrames;

    /// One window moves the clock each submit; two at one rate move it once a round, A then B in
    /// frame 2 and again in frame 3; a window that submits twice before the other moves it on.
    /// Each submit's frame is the one `next` named before it, which moves nothing.
    #[test]
    fn a_round_of_owners_is_one_frame() {
        let mut owners = RenderOwners::default();
        let (a, b) = (owners.reserve(), owners.reserve());
        let mut frames = DisplayFrames::default();
        let mut submit = |owner, want: u64| {
            assert_eq!(frames.next(owner), want, "named before the submit");
            assert_eq!(frames.next(owner), want, "naming moves nothing");
            assert_eq!(frames.advance(owner), want);
        };
        submit(a, 0);
        submit(a, 1);
        submit(a, 2);
        submit(b, 2);
        submit(a, 3);
        submit(b, 3);
        submit(b, 4);
        frames.retire(b);
        assert_eq!(frames.next(b), 4, "a retired owner waits on no round");
        assert_eq!(frames.advance(b), 4);
    }
}
