//! The window-surface texture usages Palantir's compositor requires, by Palantir's own names.

use crate::gpu::flag_names::wgpu_mirror;

flag_set! {
    /// The texture usages a window surface must offer Palantir's compositor, so a
    /// [`SurfaceError`](crate::SurfaceError) names the missing ones without a
    /// graphics-API type. `ALL` is the whole requirement.
    ///
    /// `Display` writes the graphics API's names, joined by `, `.
    pub struct SurfaceUsages: u8 {
        /// The compositor renders into the surface texture.
        const RENDER_ATTACHMENT = 1 << 0;
    }
}

wgpu_mirror!(SurfaceUsages => wgpu::TextureUsages {
    RENDER_ATTACHMENT => wgpu::TextureUsages::RENDER_ATTACHMENT,
});

#[cfg(test)]
mod tests;
