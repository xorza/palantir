//! [`DecayingTexture`]: a texture that grows to a frame's need and steps down to the peak of an
//! expiry window, the one form of `src/gpu/AGENTS.md`'s "Scratch decays".

use glam::UVec2;
use std::fmt::Debug;

/// The texels an edge grows by, so a surface that grows a few px a frame, as a resize drag does,
/// makes a new texture only every so often.
pub(crate) const GROWTH_GRANULE: u32 = 256;

/// What a decaying texture is sized by: its texels, or more where its shape needs more, as levels
/// add their mip count.
pub(crate) trait Extent: Copy + PartialEq + Debug {
    /// No texture.
    const NONE: Self;

    /// The extent that holds both.
    #[must_use]
    fn union(self, other: Self) -> Self;

    /// Whether a texture of this extent holds `want`.
    fn holds(self, want: Self) -> bool;

    /// The extent a texture holding `self` and asked to hold `want` is made at: rounded up so it
    /// grows only now and then, inside the device's `ceiling` edge.
    #[must_use]
    fn grown(self, want: Self, ceiling: u32) -> Self;
}

impl Extent for UVec2 {
    const NONE: Self = Self::ZERO;

    fn union(self, other: Self) -> Self {
        self.max(other)
    }

    fn holds(self, want: Self) -> bool {
        self.cmpge(want).all()
    }

    /// Each edge rounded up to [`GROWTH_GRANULE`], held to the ceiling.
    fn grown(self, want: Self, ceiling: u32) -> Self {
        debug_assert!(
            want.max_element() <= ceiling,
            "a texture of {want} passes the device's {ceiling} edge"
        );
        self.max(want.map(|edge| edge.next_multiple_of(GROWTH_GRANULE).min(ceiling)))
    }
}

/// A texture made by its owner at an [`Extent`]: grown to hold each frame's need, and at the end
/// of each expiry window made again at the window's peak, or dropped where no frame needed it.
/// The owner keeps the window, which several textures may share.
#[derive(Debug)]
pub(crate) struct DecayingTexture<T, E = UVec2> {
    held: Option<Held<T, E>>,
    /// The most a frame of the current window needed.
    peak: E,
}

/// A made texture and the extent it was made at.
#[derive(Debug)]
struct Held<T, E> {
    texture: T,
    extent: E,
}

impl<T, E: Extent> DecayingTexture<T, E> {
    /// No texture yet.
    pub(crate) const fn new() -> Self {
        Self {
            held: None,
            peak: E::NONE,
        }
    }

    /// The texture, `None` before the first need and after a window that needed none.
    pub(crate) fn get(&self) -> Option<&T> {
        self.held.as_ref().map(|held| &held.texture)
    }

    /// The extent it holds, [`Extent::NONE`] with no texture.
    pub(crate) fn extent(&self) -> E {
        self.held.as_ref().map_or(E::NONE, |held| held.extent)
    }

    /// Hold `want`, made again by `make` at a grown extent where it holds less, and note it toward
    /// the window's peak.
    pub(crate) fn reserve(&mut self, want: E, ceiling: u32, make: impl FnOnce(E) -> T) {
        self.peak = self.peak.union(want);
        let have = self.extent();
        if have.holds(want) {
            return;
        }
        self.make(have.grown(want, ceiling), make);
    }

    /// At the end of an expiry window: make it again by `make` at the window's peak where it holds
    /// another extent, or drop it where no frame needed it, and start the next window's peak.
    pub(crate) fn step_down(&mut self, ceiling: u32, make: impl FnOnce(E) -> T) {
        let peak = std::mem::replace(&mut self.peak, E::NONE);
        if peak == E::NONE {
            self.held = None;
            return;
        }
        let want = E::NONE.grown(peak, ceiling);
        if self.extent() != want {
            self.make(want, make);
        }
    }

    fn make(&mut self, extent: E, make: impl FnOnce(E) -> T) {
        self.held = Some(Held {
            texture: make(extent),
            extent,
        });
    }
}

#[cfg(test)]
mod tests {
    use crate::gpu::resource::decaying_texture::{DecayingTexture, Extent};
    use glam::UVec2;
    use std::cell::Cell;

    /// A reserve grows to granules inside the ceiling and only past what it holds; a step-down
    /// remakes at the window's peak and drops after a window that needed nothing.
    #[test]
    fn a_texture_grows_by_granules_and_steps_down_to_the_peak() {
        let made = Cell::new(0);
        let make = |extent: UVec2| {
            made.set(made.get() + 1);
            extent
        };
        let mut texture = DecayingTexture::<UVec2>::new();
        assert_eq!(texture.get(), None);
        texture.reserve(UVec2::new(300, 10), 600, make);
        assert_eq!(texture.extent(), UVec2::new(512, 256));
        texture.reserve(UVec2::new(500, 200), 600, make);
        assert_eq!(made.get(), 1, "held already");
        texture.reserve(UVec2::new(590, 10), 600, make);
        assert_eq!(
            texture.extent(),
            UVec2::new(600, 256),
            "held to the ceiling"
        );

        texture.step_down(600, make);
        assert_eq!(made.get(), 2);
        assert_eq!(
            texture.get(),
            Some(&UVec2::new(600, 256)),
            "the peak, 590 × 200"
        );
        texture.reserve(UVec2::new(10, 10), 600, make);
        texture.step_down(600, make);
        assert_eq!(texture.extent(), UVec2::new(256, 256), "stepped down");
        texture.step_down(600, make);
        assert_eq!(texture.get(), None, "a window that needed nothing");
        assert_eq!(texture.extent(), UVec2::NONE);
    }

    /// A texture grows each short edge to the next whole granule, 300 → 512 and 10 → 256, keeps an
    /// edge it already holds, 512 for 520 × 10 grows to 768 × 256, never shrinks, and stops at the
    /// ceiling, 16 300 → 16 384 rather than 16 384's next granule.
    #[test]
    fn a_texture_grows_by_whole_granules_up_to_the_ceiling() {
        let ceiling = 16_384;
        for (have, want, grows_to) in [
            (UVec2::ZERO, UVec2::new(300, 10), UVec2::new(512, 256)),
            (
                UVec2::new(512, 256),
                UVec2::new(520, 10),
                UVec2::new(768, 256),
            ),
            (UVec2::new(768, 768), UVec2::new(1, 1), UVec2::new(768, 768)),
            (
                UVec2::ZERO,
                UVec2::new(16_300, 256),
                UVec2::new(16_384, 256),
            ),
        ] {
            assert_eq!(have.grown(want, ceiling), grows_to, "{have} for {want}");
        }
    }
}
