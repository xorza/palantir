//! The window-surface texture usages Palantir's compositor requires, by Palantir's own names.

use std::fmt;
use std::fmt::{Display, Formatter};

use crate::gpu::flag_names;

flag_set! {
    /// The texture usages a window surface must offer Palantir's compositor, so a
    /// [`SurfaceError`](crate::SurfaceError) names the missing ones without a
    /// graphics-API type. `ALL` is the whole requirement.
    ///
    /// `Display` writes the graphics API's names, joined by `, `.
    pub struct SurfaceUsages {
        /// The compositor renders into the surface texture.
        const RENDER_ATTACHMENT = 1 << 0;
    }
}

impl SurfaceUsages {
    /// Each flag beside the wgpu usage it stands for; every declared flag appears once.
    const WGPU: [(Self, wgpu::TextureUsages); 1] = [(
        Self::RENDER_ATTACHMENT,
        wgpu::TextureUsages::RENDER_ATTACHMENT,
    )];

    pub(crate) const fn to_wgpu(self) -> wgpu::TextureUsages {
        let mut usages = wgpu::TextureUsages::empty();
        let mut i = 0;
        while i < Self::WGPU.len() {
            let (flag, usage) = Self::WGPU[i];
            if self.contains(flag) {
                usages = usages.union(usage);
            }
            i += 1;
        }
        usages
    }

    /// The flags whose usages `usages` holds; usages Palantir does not require drop.
    pub(crate) const fn from_wgpu_truncate(usages: wgpu::TextureUsages) -> Self {
        let mut set = Self::NONE;
        let mut i = 0;
        while i < Self::WGPU.len() {
            let (flag, usage) = Self::WGPU[i];
            if usages.contains(usage) {
                set.insert(flag);
            }
            i += 1;
        }
        set
    }
}

impl Display for SurfaceUsages {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        flag_names::write(f, self.to_wgpu().iter_names())
    }
}

#[cfg(test)]
mod tests {
    use crate::gpu::surface::surface_usages::SurfaceUsages;

    #[test]
    fn maps_to_wgpu_both_ways_and_displays_wgpu_names() {
        assert_eq!(
            SurfaceUsages::ALL.to_wgpu(),
            wgpu::TextureUsages::RENDER_ATTACHMENT,
            "every declared flag maps"
        );
        assert_eq!(SurfaceUsages::NONE.to_wgpu(), wgpu::TextureUsages::empty());
        assert_eq!(
            SurfaceUsages::from_wgpu_truncate(
                wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_DST
            ),
            SurfaceUsages::RENDER_ATTACHMENT,
            "a usage Palantir does not require drops"
        );
        assert_eq!(
            SurfaceUsages::from_wgpu_truncate(wgpu::TextureUsages::COPY_DST),
            SurfaceUsages::NONE
        );

        assert_eq!(
            SurfaceUsages::RENDER_ATTACHMENT.to_string(),
            "RENDER_ATTACHMENT"
        );
        assert_eq!(SurfaceUsages::NONE.to_string(), "");
    }
}
