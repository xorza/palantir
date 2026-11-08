//! [`EffectImageGroups`]: the bind groups effect draws bind their images through, one for each
//! set of images, kept by the image store that owns the textures they bind.

use rustc_hash::FxHashMap;

use crate::effect::MAX_IMAGE_SLOTS;
use crate::primitives::identity::texture_id::TextureId;

/// The images one effect draw binds, in slot order, `TextureId(0)` (the render path's "no
/// texture") in each slot past the last: the key its group is kept under.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct EffectImages([TextureId; MAX_IMAGE_SLOTS as usize]);

impl EffectImages {
    /// The draw that binds `ids`, at most [`MAX_IMAGE_SLOTS`], none of them `TextureId(0)`.
    pub(crate) fn new(ids: &[TextureId]) -> Self {
        assert!(
            ids.len() <= usize::from(MAX_IMAGE_SLOTS),
            "an effect binds at most {MAX_IMAGE_SLOTS} images, not {}",
            ids.len()
        );
        debug_assert!(
            !ids.contains(&TextureId(0)),
            "no registered image is `TextureId(0)`"
        );
        let mut slots = [TextureId(0); MAX_IMAGE_SLOTS as usize];
        slots[..ids.len()].copy_from_slice(ids);
        Self(slots)
    }

    /// The images bound, in slot order.
    pub(crate) fn ids(&self) -> &[TextureId] {
        let len = self
            .0
            .iter()
            .position(|&id| id == TextureId(0))
            .unwrap_or(self.0.len());
        &self.0[..len]
    }
}

/// The most groups kept past the sets one frame draws, each for a set of images some frame drew:
/// a new set past it takes the place of the set drawn longest ago, unless the frame draws every
/// set kept, as a page of 300 thumbnails each under an effect does, and then the cache holds them
/// all, as a set left out would paint nothing.
pub(crate) const MAX_GROUPS: usize = 256;

/// The group for each set of images effect draws bind, made the first frame a set draws and kept
/// until one of its images is freed, the device goes, or [`MAX_GROUPS`] pushes it out. Made ahead
/// and bound again, as WebGPU's guidance keeps a material's group (Toji, "WebGPU Bind Group best
/// practices"); the image store owns it, so a freed image drops each group that holds its texture
/// at once. Refused: a group per frame, which creates on every draw; a sweep of unused groups,
/// which lands its whole cost on one frame.
#[derive(Debug, Default)]
pub(crate) struct EffectImageGroups {
    groups: FxHashMap<EffectImages, KeptGroup>,
}

/// One kept group and the last display frame that drew it.
#[derive(Debug)]
struct KeptGroup {
    group: wgpu::BindGroup,
    drawn: u64,
}

impl EffectImageGroups {
    /// Note that `frame` draws `images`, making its group with `make` where none is kept; `make`
    /// returns `None` where an image is not on the device, as after a loss before its upload, and
    /// the draw then paints nothing.
    pub(crate) fn ensure(
        &mut self,
        images: EffectImages,
        frame: u64,
        make: impl FnOnce() -> Option<wgpu::BindGroup>,
    ) {
        if let Some(kept) = self.groups.get_mut(&images) {
            kept.drawn = frame;
            return;
        }
        let Some(group) = make() else {
            return;
        };
        if self.groups.len() >= MAX_GROUPS {
            // A scan of the kept sets, only on a new set past the bound.
            let (oldest, drawn) = self
                .groups
                .iter()
                .map(|(images, kept)| (*images, kept.drawn))
                .min_by_key(|&(_, drawn)| drawn)
                .expect("a full cache holds a group");
            if drawn < frame {
                self.groups.remove(&oldest);
            }
        }
        self.groups.insert(
            images,
            KeptGroup {
                group,
                drawn: frame,
            },
        );
    }

    /// The group `images` bind through, or `None` where an image is not on the device.
    pub(crate) fn get(&self, images: &EffectImages) -> Option<&wgpu::BindGroup> {
        self.groups.get(images).map(|kept| &kept.group)
    }

    /// Drop every group that binds `id`, an image just freed, so no group keeps its texture.
    pub(crate) fn forget(&mut self, id: TextureId) {
        self.groups.retain(|images, _| !images.ids().contains(&id));
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.groups.len()
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use crate::gpu::resource::effect_image_groups::{EffectImageGroups, EffectImages, MAX_GROUPS};
    use crate::gpu::test_gpu::headless_test_gpu;
    use crate::internals::panic_probe::assert_panics_with;
    use crate::primitives::identity::texture_id::TextureId;

    fn set(ids: &[u64]) -> EffectImages {
        EffectImages::new(&ids.iter().copied().map(TextureId).collect::<Vec<_>>())
    }

    /// A set reads back the ids it holds, in order, and no more; two sets of the same ids in
    /// another order differ; five ids are past the slots.
    #[test]
    fn a_set_holds_its_ids_in_slot_order() {
        assert_eq!(set(&[3, 1]).ids(), [TextureId(3), TextureId(1)]);
        assert_eq!(set(&[]).ids(), []);
        assert_eq!(set(&[1, 2, 3, 4]).ids().len(), 4);
        assert_ne!(set(&[3, 1]), set(&[1, 3]));
        assert_panics_with("at most 4 images, not 5", || set(&[1, 2, 3, 4, 5]));
    }

    /// A set's group is made once and kept: a second frame makes none; a set whose image is not
    /// on the device keeps none; forgetting an image drops exactly the sets that hold it. Past
    /// [`MAX_GROUPS`], a new set takes the place of the set drawn longest ago: with sets 1 to 256
    /// drawn at frames 1 to 256 and set 1 drawn again at 300, set 257 at 301 pushes out set 2. A
    /// frame that draws every kept set pushes none out: 256 sets and a 257th, all at frame 400,
    /// keep 257.
    #[test]
    fn a_group_is_made_once_and_kept_within_its_bound() {
        let gpu = headless_test_gpu();
        let layout = gpu
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: None,
                entries: &[],
            });
        let made = Cell::new(0);
        let make = || {
            made.set(made.get() + 1);
            Some(gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &layout,
                entries: &[],
            }))
        };
        let mut groups = EffectImageGroups::default();
        groups.ensure(set(&[1, 2]), 1, make);
        groups.ensure(set(&[1, 2]), 2, make);
        assert_eq!(made.get(), 1);
        groups.ensure(set(&[9]), 2, || None);
        assert!(groups.get(&set(&[9])).is_none());
        groups.ensure(set(&[2, 3]), 2, make);
        groups.ensure(set(&[4]), 2, make);
        groups.forget(TextureId(2));
        assert_eq!(groups.len(), 1);
        assert!(groups.get(&set(&[4])).is_some());

        let mut groups = EffectImageGroups::default();
        for id in 1..=MAX_GROUPS as u64 {
            groups.ensure(set(&[id]), id, make);
        }
        groups.ensure(set(&[1]), 300, make);
        groups.ensure(set(&[MAX_GROUPS as u64 + 1]), 301, make);
        assert_eq!(groups.len(), MAX_GROUPS);
        assert!(groups.get(&set(&[1])).is_some(), "drawn again at 300");
        assert!(groups.get(&set(&[2])).is_none(), "drawn longest ago");
        assert!(groups.get(&set(&[3])).is_some());
        assert!(groups.get(&set(&[MAX_GROUPS as u64 + 1])).is_some());

        let mut groups = EffectImageGroups::default();
        for id in 1..=MAX_GROUPS as u64 + 1 {
            groups.ensure(set(&[id]), 400, make);
        }
        assert_eq!(groups.len(), MAX_GROUPS + 1);
        assert!(groups.get(&set(&[1])).is_some(), "drawn this frame");
    }
}
