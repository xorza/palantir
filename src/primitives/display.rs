/// Logical→physical pixel mapping. Read by the renderer at submit
/// time; written by the host on construction and on
/// `WindowEvent::ScaleFactorChanged`. Group exists so future
/// rasterization knobs (sRGB correction, MSAA, gamma) have a clear
/// home.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Display {
    /// Logical→physical conversion factor (e.g. `2.0` on a 2× retina
    /// display).
    pub scale_factor: f32,
    /// Whether the renderer snaps rect edges to integer physical
    /// pixels. Default `true` — sharper edges, no half-pixel blur.
    pub pixel_snap: bool,
}

impl Default for Display {
    fn default() -> Self {
        Self {
            scale_factor: 1.0,
            pixel_snap: true,
        }
    }
}
