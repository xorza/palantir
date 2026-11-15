//! [`MoveBudget`]: what a display frame's moves may take.

use crate::gpu::device::pass_traffic::PassTraffic;

/// What a display frame's moves may still take, across the lists: WebRender's compaction
/// `area_threshold` of texels, 512², and a count of moves, which on a device that moves by copy
/// commands each cost the driver's fixed overhead. The texels are spent, not fitted: the move that
/// passes them goes and ends the frame's moves, as WebRender's `try_compaction` counts a move
/// before it compares, so an item of more texels than a whole frame's still moves and its drain
/// ends. Refused: a move held to what is left, which kept such an item's page draining for good.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MoveBudget {
    pub(crate) texels: u64,
    pub(crate) moves: u32,
}

impl MoveBudget {
    /// A display frame's on a device that moves by copy commands: 16, as a copy costs 43 µs on
    /// V3D, so they hold a frame's moves near 0.7 ms there, where 64 took 2.4 ms
    /// (`.notes/research/2026-11-12-atlas-pool-measurements.md`, "Moves and pages").
    pub(crate) const COPIES: Self = Self {
        texels: 512 * 512,
        moves: 16,
    };

    /// A display frame's on a device that moves by one draw a destination page: the texels alone,
    /// as the rectangles ride in the draw's instances.
    pub(crate) const DRAWN: Self = Self {
        texels: 512 * 512,
        moves: u32::MAX,
    };

    /// A display frame's on a device whose passes move `traffic`: by copy commands where a pass
    /// moves its whole target, as a draw onto a page would load and store all of it, by draw
    /// elsewhere (`src/gpu/AGENTS.md`, Atlases).
    pub(crate) const fn of(traffic: PassTraffic) -> Self {
        match traffic {
            PassTraffic::WholeTarget => Self::COPIES,
            PassTraffic::Drawn => Self::DRAWN,
        }
    }

    /// Whether the frame's moves are spent: no move left, or the texels taken.
    pub(super) const fn is_spent(&self) -> bool {
        self.moves == 0 || self.texels == 0
    }

    /// Take one move of `texels`, from a budget not spent.
    pub(super) const fn spend(&mut self, texels: u64) {
        debug_assert!(!self.is_spent(), "a move from a spent budget");
        self.moves -= 1;
        self.texels = self.texels.saturating_sub(texels);
    }
}
