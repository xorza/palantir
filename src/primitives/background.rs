use crate::primitives::approx::approx_zero;
use crate::primitives::color::Color;
use crate::primitives::corners::Corners;
use crate::primitives::stroke::Stroke;

/// Paint data shared by container widgets (`Frame`, `Panel`, `Grid`)
/// and per-state widget visuals: fill colour, optional stroke, and
/// corner radii. Default is transparent fill / no stroke / zero radius
/// — emitting nothing.
///
/// Pure data, no methods that need a `Ui` — paint emission goes
/// through `Tree::chrome_table` and the encoder, not through
/// shape-list registration.
#[derive(Clone, Copy, Debug, Default, PartialEq, Hash, serde::Serialize, serde::Deserialize)]
pub struct Background {
    pub fill: Color,
    pub stroke: Option<Stroke>,
    pub radius: Corners,
}

impl Background {
    /// True when this Background paints nothing visible (transparent
    /// fill + no/transparent/zero-width stroke). The encoder skips
    /// emitting a `DrawRect` for no-op chrome so transparent
    /// `Surface::scissor()` defaults don't leak draw commands.
    pub fn is_noop(&self) -> bool {
        let no_fill = self.fill.approx_transparent();
        let no_stroke = match self.stroke {
            None => true,
            Some(s) => approx_zero(s.width) || s.color.approx_transparent(),
        };
        no_fill && no_stroke
    }
}
