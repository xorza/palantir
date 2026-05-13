use crate::primitives::brush::Brush;
use crate::primitives::corners::Corners;
use crate::primitives::shadow::Shadow;
use crate::primitives::stroke::Stroke;
use palantir_anim_derive::Animatable;

/// Paint data shared by container widgets (`Frame`, `Panel`, `Grid`)
/// and per-state widget visuals: fill colour, optional stroke, and
/// corner radii. Default is transparent fill / no stroke / zero radius
/// — emitting nothing.
///
/// Pure data, no methods that need a `Ui` — paint emission goes
/// through `Tree::chrome_table` and the encoder, not through
/// shape-list registration.
///
/// `Animatable` derived: fill and stroke interpolate componentwise;
/// `radius` is `#[animate(snap)]` (corner-radius morphing across
/// states is rarely-wanted polish and would require `Corners:
/// Animatable`). "No stroke" is `Stroke::ZERO` (width 0, transparent)
/// — there is no `Option<Stroke>` here. The animation pipeline lerps
/// `Stroke` directly through `Stroke::ZERO`; paint-time `is_noop`
/// filtering catches both authored and animation-decayed no-ops.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Hash, serde::Serialize, serde::Deserialize, Animatable,
)]
pub struct Background {
    pub fill: Brush,
    pub stroke: Stroke,
    #[animate(snap)]
    pub radius: Corners,
    /// Single drop / inset shadow. `Shadow::NONE` (the `Default`) is
    /// the "no shadow" sentinel — matches the `Stroke::ZERO`
    /// convention so the field stays plain `Shadow` and animates
    /// componentwise (alpha lerps in/out for hover-elevation), with
    /// the paint-time `is_noop` filter catching authored or
    /// animation-decayed no-ops. Multi-shadow stacks: push
    /// `Shape::Shadow` records directly via `Ui::add_shape`.
    pub shadow: Shadow,
}

impl Background {
    /// True when this Background paints nothing visible — transparent
    /// fill + transparent/zero-width stroke + no-op shadow. The
    /// encoder skips emitting a `DrawRect` for no-op chrome so
    /// transparent `Surface::scissor()` defaults don't leak draw
    /// commands. The shadow check is required: the encoder's chrome
    /// branch paints shadow before the rect, so dropping chrome
    /// without considering shadow would silently kill a shadow-only
    /// background.
    pub fn is_noop(&self) -> bool {
        self.fill.is_noop() && self.stroke.is_noop() && self.shadow.is_noop()
    }
}
