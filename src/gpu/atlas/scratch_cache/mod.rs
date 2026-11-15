//! [`ScratchCache`]: the textures of their own a list hands an item too large for a page.

use crate::common::expiry_window::EXPIRY_FRAMES;
use crate::gpu::atlas::Evictions;
use crate::gpu::atlas::atlas_format::AtlasFormat;
use crate::gpu::atlas::error::AtlasError;
use crate::gpu::atlas::tenant::ItemOwner;
use glam::UVec2;

/// The least side [`approx_side`] gives, Skia's `kMinApproxSize`.
const APPROX_MIN: u32 = 16;

/// The side up to which every rung of [`approx_side`] is a power of two, Skia's `kMagicTol`.
const APPROX_POW2_UP_TO: u32 = 1024;

/// `side` rounded up as Skia rounds an approximate-fit texture: to the next power of two up to
/// [`APPROX_POW2_UP_TO`], above it to the midpoint between two powers where that holds the side, and
/// to at least [`APPROX_MIN`] (`skgpu::GetApproxSize`, `src/gpu/SkBackingFit.cpp`). The ladder
/// bounds how many sizes a resizing item makes.
const fn approx_side(side: u32) -> u32 {
    let side = if side < APPROX_MIN { APPROX_MIN } else { side };
    let ceil = side.next_power_of_two();
    if ceil <= APPROX_POW2_UP_TO {
        return ceil;
    }
    let mid = ceil / 2 + ceil / 4;
    if side <= mid { mid } else { ceil }
}

/// A texture of the cache: its size, whether an item holds it, and the generation it was handed
/// out at.
#[derive(Clone, Copy, Debug)]
struct ScratchTexture {
    size: UVec2,
    state: ScratchState,
    generation: u32,
}

/// What a texture of the cache is doing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ScratchState {
    /// `owner`'s item holds it, last drawn in display frame `last_use`.
    Held { owner: ItemOwner, last_use: u64 },
    /// No item holds it since display frame `since`; it serves a request that fits until it
    /// expires.
    Idle { since: u64 },
    /// No texture: the slot waits for the next one made.
    Gone,
}

/// An item's texture of its own: its list, its slot in the list's cache and the generation it was
/// handed out at, as an [`AtlasHandle`](crate::gpu::atlas::atlas_handle::AtlasHandle) names an
/// item of a page.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ScratchId {
    pub(crate) format: AtlasFormat,
    pub(crate) idx: u32,
    pub(crate) generation: u32,
}

/// A scratch texture a purge took, as its tenant drains it: its id, stale now, and the tag the
/// tenant acquired it under, as [`Evicted`](crate::gpu::atlas::atlas_handle::Evicted) names an
/// item.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct EvictedScratch {
    pub(crate) id: ScratchId,
    pub(crate) tag: u32,
}

/// Textures one item each of one format, sized on the [`approx_side`] ladder, no side past the
/// device's edge, reused for a request they cover within twice its area (Chromium
/// `ResourcePool`'s `kReuseThreshold`), the most recently idled first, and dropped an expiry window
/// after they idle. The GPU half makes and drops a texture where a slot's size or state changes.
///
/// Its bound is its items': a held texture is an item a tenant draws, and an idle one goes after a
/// window, so the bytes are those of the large items a window draws, each under a side of
/// `edge`, on top of the list's soft budget. A full-window backdrop of 1936 × 1088 takes about
/// 24 MiB of `Rgba16F`; the app's frame, not the cache, sets how many it draws.
#[derive(Debug)]
pub(crate) struct ScratchCache {
    format: AtlasFormat,
    /// The device's largest texture edge.
    edge: u32,
    textures: Vec<ScratchTexture>,
}

impl ScratchCache {
    /// The most a reused texture's area may pass the rounded request's, Chromium's.
    const REUSE_AREA_FACTOR: u64 = 2;

    /// An empty cache of `format` on a device whose largest texture edge is `edge`.
    pub(crate) const fn new(format: AtlasFormat, edge: u32) -> Self {
        Self {
            format,
            edge,
            textures: Vec::new(),
        }
    }

    /// A texture for `owner`'s item of `size`, drawn at display frame `frame`: an idle one where
    /// one serves, else a new one on the ladder.
    pub(crate) fn acquire(
        &mut self,
        size: UVec2,
        owner: ItemOwner,
        frame: u64,
    ) -> Result<ScratchId, AtlasError> {
        let edge = self.edge;
        if size.x > edge || size.y > edge {
            return Err(AtlasError::Oversized { size, edge });
        }
        assert!(size.x > 0 && size.y > 0, "a scratch item holds a texel");
        let want = UVec2::new(approx_side(size.x), approx_side(size.y)).min(UVec2::splat(edge));
        let area = |size: UVec2| u64::from(size.x) * u64::from(size.y);
        let reuse = self
            .textures
            .iter()
            .enumerate()
            .filter_map(|(idx, texture)| match texture.state {
                ScratchState::Idle { since }
                    if texture.size.cmpge(want).all()
                        && area(texture.size) <= Self::REUSE_AREA_FACTOR * area(want) =>
                {
                    Some((since, idx))
                }
                _ => None,
            })
            .max();
        let idx = if let Some((_, idx)) = reuse {
            idx
        } else if let Some(idx) = self
            .textures
            .iter()
            .position(|texture| texture.state == ScratchState::Gone)
        {
            self.textures[idx].size = want;
            idx
        } else {
            self.textures.push(ScratchTexture {
                size: want,
                state: ScratchState::Gone,
                generation: 0,
            });
            self.textures.len() - 1
        };
        let texture = &mut self.textures[idx];
        texture.state = ScratchState::Held {
            owner,
            last_use: frame,
        };
        Ok(ScratchId {
            format: self.format,
            idx: u32::try_from(idx).expect("scratch textures are counted in u32"),
            generation: texture.generation,
        })
    }

    /// Give back the texture `id` holds at display frame `frame`; it idles until it serves again
    /// or expires.
    ///
    /// # Panics
    ///
    /// If `id` is another list's, or is stale: released already, or taken by a purge, whose take
    /// its tenant drops before it releases anything (`WgpuBackend::drop_evicted`), so a stale id is
    /// its tenant's slip.
    pub(crate) fn release(&mut self, id: ScratchId, frame: u64) {
        assert_eq!(id.format, self.format, "a scratch id of another list");
        let texture = &mut self.textures[id.idx as usize];
        assert_eq!(
            texture.generation, id.generation,
            "a scratch id released twice"
        );
        assert!(
            matches!(texture.state, ScratchState::Held { .. }),
            "a scratch id released twice"
        );
        texture.generation = texture
            .generation
            .checked_add(1)
            .expect("a scratch generation overflowed");
        texture.state = ScratchState::Idle { since: frame };
    }

    /// Drop the textures idle for an expiry window at display frame `frame`
    /// ([`EXPIRY_FRAMES`]).
    pub(crate) fn expire(&mut self, frame: u64) {
        for texture in &mut self.textures {
            if let ScratchState::Idle { since } = texture.state
                && since + EXPIRY_FRAMES <= frame
            {
                texture.state = ScratchState::Gone;
            }
        }
    }

    /// The texture `id` holds, live: its size, `None` once a purge took it or it was released.
    pub(crate) fn size(&self, id: ScratchId) -> Option<UVec2> {
        let texture = self.live(id)?;
        Some(texture.size)
    }

    /// Mark the texture `id` holds drawn at display frame `frame`, and give its size; `None` once a
    /// purge took it.
    pub(crate) fn stamp(&mut self, id: ScratchId, frame: u64) -> Option<UVec2> {
        let texture = self.live_mut(id)?;
        if let ScratchState::Held { last_use, .. } = &mut texture.state {
            debug_assert!(*last_use <= frame, "the display frames ran backwards");
            *last_use = frame;
        }
        Some(texture.size)
    }

    /// The slot of the texture `id` names, where its item still holds it.
    fn live_idx(&self, id: ScratchId) -> Option<usize> {
        debug_assert_eq!(id.format, self.format, "a scratch id of another list");
        let idx = id.idx as usize;
        self.textures
            .get(idx)
            .filter(|texture| {
                texture.generation == id.generation
                    && matches!(texture.state, ScratchState::Held { .. })
            })
            .map(|_| idx)
    }

    /// The texture of [`Self::live_idx`].
    fn live(&self, id: ScratchId) -> Option<&ScratchTexture> {
        self.live_idx(id).map(|idx| &self.textures[idx])
    }

    /// The texture of [`Self::live_idx`], to change.
    fn live_mut(&mut self, id: ScratchId) -> Option<&mut ScratchTexture> {
        let idx = self.live_idx(id)?;
        Some(&mut self.textures[idx])
    }

    /// Drop every idle texture now, whatever its age, and every held one display frame `frame` did
    /// not draw, queueing its id for its tenant in `evictions`, as a purge takes an item of a page:
    /// one full-window glass holds tens of MiB.
    pub(crate) fn purge(&mut self, frame: u64, evictions: &mut Evictions) {
        for (idx, texture) in self.textures.iter_mut().enumerate() {
            match texture.state {
                ScratchState::Idle { .. } => texture.state = ScratchState::Gone,
                ScratchState::Held { owner, last_use } if last_use < frame => {
                    evictions.push_scratch(
                        owner.tenant,
                        EvictedScratch {
                            id: ScratchId {
                                format: self.format,
                                idx: u32::try_from(idx)
                                    .expect("scratch textures are counted in u32"),
                                generation: texture.generation,
                            },
                            tag: owner.tag,
                        },
                    );
                    texture.generation = texture
                        .generation
                        .checked_add(1)
                        .expect("a scratch generation overflowed");
                    texture.state = ScratchState::Gone;
                }
                ScratchState::Held { .. } | ScratchState::Gone => {}
            }
        }
    }

    /// Each slot's texture size, by slot, `None` where it holds none: what the GPU half makes.
    pub(crate) fn sizes(&self) -> impl ExactSizeIterator<Item = Option<UVec2>> + '_ {
        self.textures
            .iter()
            .map(|texture| (texture.state != ScratchState::Gone).then_some(texture.size))
    }

    /// The texels the cache's textures hold, held and idle.
    pub(crate) fn texels(&self) -> u64 {
        self.textures
            .iter()
            .filter(|texture| texture.state != ScratchState::Gone)
            .map(|texture| u64::from(texture.size.x) * u64::from(texture.size.y))
            .sum()
    }
}

#[cfg(feature = "internals")]
pub(crate) mod internals {
    use crate::gpu::atlas::scratch_cache::{ScratchCache, ScratchState};

    impl ScratchCache {
        /// The texels of the textures an item holds, the live part of [`Self::texels`].
        pub(crate) fn held_texels(&self) -> u64 {
            self.textures
                .iter()
                .filter(|texture| matches!(texture.state, ScratchState::Held { .. }))
                .map(|texture| u64::from(texture.size.x) * u64::from(texture.size.y))
                .sum()
        }
    }
}

#[cfg(test)]
mod tests;
