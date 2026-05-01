use crate::primitives::{Color, Corners, Stroke};
use crate::shape::Shape;
use crate::ui::Ui;

/// Paint data shared by container widgets (`Frame`, `Panel`, `Grid`):
/// fill colour, optional stroke, and corner radii. Default is transparent
/// fill / no stroke / zero radius — emitting nothing — so a container that
/// never sets any of these adds no shape to the tree (`Ui::add_shape`
/// filters no-op shapes).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Background {
    pub fill: Color,
    pub stroke: Option<Stroke>,
    pub radius: Corners,
}

impl Background {
    pub(crate) fn add_to(&self, ui: &mut Ui) {
        ui.add_shape(Shape::RoundedRect {
            radius: self.radius,
            fill: self.fill,
            stroke: self.stroke,
        });
    }
}

/// Mixin: any widget builder that holds a `Background` gets the chained
/// `.fill()` / `.stroke()` / `.radius()` setters by impl'ing one method.
/// Analogous to `Element` for layout fields.
pub trait Styled: Sized {
    fn background_mut(&mut self) -> &mut Background;

    fn fill(mut self, c: Color) -> Self {
        self.background_mut().fill = c;
        self
    }
    /// Accepts `Stroke`, `Option<Stroke>`, or `None`.
    fn stroke(mut self, s: impl Into<Option<Stroke>>) -> Self {
        self.background_mut().stroke = s.into();
        self
    }
    fn radius(mut self, r: impl Into<Corners>) -> Self {
        self.background_mut().radius = r.into();
        self
    }
}
