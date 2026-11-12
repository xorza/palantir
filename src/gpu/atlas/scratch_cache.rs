//! [`ScratchCache`]: the textures of their own a list hands an item too large for a page.

use crate::gpu::atlas::error::AtlasError;
use crate::gpu::resource::expiry_window::EXPIRY_FRAMES;
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
    /// An item holds it.
    Held,
    /// No item holds it since display frame `since`; it serves a request that fits until it
    /// expires.
    Idle { since: u64 },
    /// No texture: the slot waits for the next one made.
    Gone,
}

/// An item's texture of its own: its slot in the cache and the generation it was handed out at.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ScratchId {
    pub(crate) idx: u32,
    pub(crate) generation: u32,
}

/// Textures one item each, sized on the [`approx_side`] ladder, reused for a request they cover
/// within twice its area (Chromium `ResourcePool`'s `kReuseThreshold`), the most recently idled
/// first, and dropped an expiry window after they idle. The GPU half makes and drops a texture
/// where a slot's size or state changes.
#[derive(Debug, Default)]
pub(crate) struct ScratchCache {
    textures: Vec<ScratchTexture>,
}

impl ScratchCache {
    /// The most a reused texture's area may pass the rounded request's, Chromium's.
    const REUSE_AREA_FACTOR: u64 = 2;

    /// A texture for an item of `size`, an idle one where one serves, else a new one on the
    /// ladder, no side past `edge`, the device's.
    pub(crate) fn acquire(&mut self, size: UVec2, edge: u32) -> Result<ScratchId, AtlasError> {
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
        texture.state = ScratchState::Held;
        Ok(ScratchId {
            idx: u32::try_from(idx).expect("scratch textures are counted in u32"),
            generation: texture.generation,
        })
    }

    /// Give back the texture `id` holds at display frame `frame`; it idles until it serves again
    /// or expires.
    pub(crate) fn release(&mut self, id: ScratchId, frame: u64) {
        let texture = &mut self.textures[id.idx as usize];
        assert_eq!(
            texture.generation, id.generation,
            "a scratch id released twice"
        );
        assert_eq!(
            texture.state,
            ScratchState::Held,
            "a scratch id released twice"
        );
        texture.generation = texture
            .generation
            .checked_add(1)
            .expect("a scratch generation overflowed");
        texture.state = ScratchState::Idle { since: frame };
    }

    /// The size of the texture `id` holds, `None` once released.
    pub(crate) fn size(&self, id: ScratchId) -> Option<UVec2> {
        let texture = self.textures.get(id.idx as usize)?;
        (texture.generation == id.generation && texture.state == ScratchState::Held)
            .then_some(texture.size)
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

    /// Drop every idle texture now, whatever its age.
    pub(crate) fn purge(&mut self) {
        for texture in &mut self.textures {
            if matches!(texture.state, ScratchState::Idle { .. }) {
                texture.state = ScratchState::Gone;
            }
        }
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

#[cfg(test)]
mod tests {
    use crate::gpu::atlas::error::AtlasError;
    use crate::gpu::atlas::scratch_cache::{ScratchCache, approx_side};
    use crate::gpu::resource::expiry_window::EXPIRY_FRAMES;
    use glam::UVec2;

    /// The ladder: at least 16, powers of two through 1024, then the midpoint where it holds the
    /// side: 1025 and 1536 take 1536, 1537 and 2048 take 2048, and 1936 × 1088, a full-window glass
    /// with its apron, takes 2048 × 1536.
    #[test]
    fn a_side_rounds_up_the_ladder() {
        let rungs = [
            (1, 16),
            (16, 16),
            (17, 32),
            (1000, 1024),
            (1024, 1024),
            (1025, 1536),
            (1536, 1536),
            (1537, 2048),
            (2048, 2048),
            (3000, 3072),
            (3073, 4096),
        ];
        for (side, rung) in rungs {
            assert_eq!(approx_side(side), rung, "side {side}");
        }
        assert_eq!((approx_side(1936), approx_side(1088)), (2048, 1536));
    }

    /// A request rounds, then reuses an idle texture that covers it within twice the area, the
    /// most recently idled first: 1300 × 900 rounds to 1536 × 1024 and takes an idle
    /// 2048 × 1536, exactly twice its area, but not an idle 2048², past twice, which stays idle; a
    /// held texture serves no one. A released id reads as stale.
    #[test]
    fn a_request_reuses_what_covers_it_within_twice_its_area() {
        let mut cache = ScratchCache::default();
        let square = cache.acquire(UVec2::splat(2048), 16384).unwrap();
        let wide = cache.acquire(UVec2::new(1937, 1100), 16384).unwrap();
        assert_eq!(cache.size(wide), Some(UVec2::new(2048, 1536)));
        let held = cache.acquire(UVec2::new(2048, 1536), 16384).unwrap();
        cache.release(square, 1);
        cache.release(wide, 2);
        assert_eq!(cache.size(wide), None, "a released id is stale");
        let reused = cache.acquire(UVec2::new(1300, 900), 16384).unwrap();
        assert_eq!(reused.idx, wide.idx, "the idle 2048 × 1536, twice the area");
        assert_eq!(cache.size(reused), Some(UVec2::new(2048, 1536)));
        let fresh = cache.acquire(UVec2::new(1300, 900), 16384).unwrap();
        assert_ne!(
            fresh.idx, square.idx,
            "the idle 2048² passes twice the area"
        );
        assert_eq!(cache.size(fresh), Some(UVec2::new(1536, 1024)));
        assert_ne!(fresh.idx, held.idx);
        // 2048² + 2048 × 1536 twice + 1536 × 1024: the idle square counts until it expires.
        assert_eq!(cache.texels(), 2048 * 2048 + 2 * 2048 * 1536 + 1536 * 1024);
    }

    /// An idle texture goes an expiry window after it idled, `since + EXPIRY_FRAMES`, and not a
    /// frame before; its slot serves the next new texture. A purge drops every idle texture at
    /// once and none held.
    #[test]
    fn an_idle_texture_expires_after_a_window() {
        let mut cache = ScratchCache::default();
        let a = cache.acquire(UVec2::splat(100), 16384).unwrap();
        let b = cache.acquire(UVec2::splat(200), 16384).unwrap();
        cache.release(a, 10);
        cache.expire(10 + EXPIRY_FRAMES - 1);
        assert_eq!(cache.texels(), 128 * 128 + 256 * 256, "a frame short");
        cache.expire(10 + EXPIRY_FRAMES);
        assert_eq!(cache.texels(), 256 * 256);
        let c = cache.acquire(UVec2::splat(30), 16384).unwrap();
        assert_eq!(c.idx, a.idx, "the gone slot takes the new texture");
        cache.release(c, 20);
        cache.purge();
        assert_eq!(cache.texels(), 256 * 256, "the held one stays");
        assert_eq!(cache.size(b), Some(UVec2::splat(256)));
    }

    /// No side passes the device's edge: a request wider than it is refused, and one within it
    /// whose rung passes it takes the edge itself.
    #[test]
    fn no_side_passes_the_device_edge() {
        let mut cache = ScratchCache::default();
        assert_eq!(
            cache.acquire(UVec2::new(4097, 10), 4096),
            Err(AtlasError::Oversized {
                size: UVec2::new(4097, 10),
                edge: 4096
            })
        );
        let id = cache.acquire(UVec2::new(3100, 10), 4096).unwrap();
        assert_eq!(cache.size(id), Some(UVec2::new(4096, 16)));
    }
}
