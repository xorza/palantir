//! A composer and the buffer it writes, kept across frames as the
//! frontend keeps them.

use crate::display::Display;
use crate::renderer::frontend::capture::PaintCapture;
use crate::renderer::frontend::composer::Composer;
use crate::renderer::frontend::test_support::TEST_MAX_TEXTURE_DIM;
use crate::renderer::render_buffer::RenderBuffer;
use crate::scene::record_store::RecordStore;
use std::num::NonZeroU32;
use std::time::Duration;

/// One composer, one output buffer and one record store, reused by every
/// [`Self::compose`] — so a multi-frame test exercises the same buffer
/// reuse production does, and a one-frame test skips the setup.
#[derive(Debug)]
pub(super) struct ComposeRig {
    pub(super) composer: Composer,
    pub(super) out: RenderBuffer,
    pub(super) store: RecordStore,
    pub(super) display: Display,
}

impl ComposeRig {
    /// A rig at the deviceless test texture cap.
    pub(super) fn new(display: Display) -> Self {
        Self::with_texture_cap(display, TEST_MAX_TEXTURE_DIM)
    }

    pub(super) fn with_texture_cap(display: Display, max_texture_dim: NonZeroU32) -> Self {
        Self {
            composer: Composer::new(max_texture_dim),
            out: RenderBuffer::new(),
            store: RecordStore::default(),
            display,
        }
    }

    /// Compose `recorded` as one frame into [`Self::out`].
    pub(super) fn compose(&mut self, recorded: &PaintCapture) {
        self.composer
            .begin(self.display, Duration::ZERO, &self.store, &mut self.out)
            .replay_from(recorded);
    }
}
