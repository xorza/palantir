//! A composer and its buffer, kept across frames as the frontend keeps them.

use crate::display::Display;
use crate::display::frame_stamp::FrameStamp;
use crate::internals::paint_capture::PaintCapture;
use crate::renderer::frontend::composer::Composer;
use crate::renderer::frontend::internals::TEST_MAX_TEXTURE_DIM;
use crate::renderer::render_buffer::RenderBuffer;
use crate::shape::record_store::RecordStore;
use std::num::NonZeroU32;
use std::time::Duration;

/// One composer, output buffer and record store reused by every [`Self::compose`], so
/// multi-frame tests and the bench exercise production's buffer reuse.
#[derive(Debug)]
pub(crate) struct ComposeRig {
    pub(crate) composer: Composer,
    pub(crate) out: RenderBuffer,
    pub(crate) store: RecordStore,
    pub(crate) display: Display,
}

impl ComposeRig {
    /// A rig at the deviceless test texture cap.
    pub(crate) fn new(display: Display) -> Self {
        Self::with_texture_cap(display, TEST_MAX_TEXTURE_DIM)
    }

    pub(crate) fn with_texture_cap(display: Display, max_texture_dim: NonZeroU32) -> Self {
        Self {
            composer: Composer::new(max_texture_dim),
            out: RenderBuffer::new(),
            store: RecordStore::default(),
            display,
        }
    }

    /// Compose `recorded` as one frame into [`Self::out`].
    pub(crate) fn compose(&mut self, recorded: &PaintCapture) {
        self.composer
            .begin(
                FrameStamp::new(self.display, Duration::ZERO),
                &self.store,
                &mut self.out,
            )
            .replay_from(recorded);
    }
}
