//! The window-surface texture usages Palantir's compositor requires, by Palantir's own names.

use crate::gpu::flag_names::wgpu_mirror;

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

wgpu_mirror!(SurfaceUsages => wgpu::TextureUsages {
    RENDER_ATTACHMENT => wgpu::TextureUsages::RENDER_ATTACHMENT,
});

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
