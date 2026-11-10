//! One box shadow draw.

use crate::renderer::frontend::paint_sink::{PaintPayload, PaintSink};
use crate::renderer::frontend::payload::spin::Spin;
use crate::shape::geometry::box_geometry::PlacedBox;
use crate::shape::paint::lowered_shadow::LoweredShadow;

/// A drop or an inset `shadow` cast by `source`, a box in the logical frame: its corners round
/// the shadow, as CSS's `box-shadow` follows its box's radii. Its own payload, as it draws through
/// the blurred-box family, which no paint of a closed shape shares, and a triangle casts none.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DrawShadowPayload {
    pub(crate) source: PlacedBox,
    /// A paint animation's turn, the box's.
    pub(crate) spin: Option<Spin>,
    pub(crate) shadow: LoweredShadow,
}

impl DrawShadowPayload {
    /// `shadow` cast by `source`, unturned.
    pub(crate) const fn new(source: PlacedBox, shadow: LoweredShadow) -> Self {
        Self {
            source,
            spin: None,
            shadow,
        }
    }

    /// This draw turned by `spin`.
    pub(crate) const fn turned(mut self, spin: Option<Spin>) -> Self {
        self.spin = spin;
        self
    }
}

impl PaintPayload for DrawShadowPayload {
    #[inline]
    fn faded(self, by: f32) -> Self {
        if by == 1.0 {
            return self;
        }
        Self {
            shadow: LoweredShadow {
                color: self.shadow.color.faded(by),
                ..self.shadow
            },
            ..self
        }
    }

    /// Paints nothing where the shadow is invisible, or covers no pixel: a drop shadow covers its
    /// source grown by its halo, an inset one its source. A zero-σ drop shadow still paints a hard
    /// shifted rect.
    #[inline]
    fn is_noop(&self) -> bool {
        let covered = if self.shadow.inset() {
            self.source.rect
        } else {
            self.source.rect.inflated(self.shadow.geometry().halo())
        };
        covered.is_paint_empty() || self.shadow.is_noop()
    }

    fn paint(self, sink: &mut impl PaintSink) {
        sink.shadow(self);
    }
}

#[cfg(test)]
mod tests {
    use crate::primitives::geometry::corners::Corners;
    use crate::primitives::geometry::rect::Rect;
    use crate::primitives::paint::color::RgbaF32;
    use crate::primitives::paint::shadow::Shadow;
    use crate::renderer::frontend::paint_sink::PaintPayload;
    use crate::renderer::frontend::payload::draw_shadow_payload::DrawShadowPayload;
    use crate::shape::geometry::box_geometry::PlacedBox;
    use crate::shape::paint::lowered_shadow::LoweredShadow;
    use glam::Vec2;

    /// A shadow paints where it covers a pixel: a drop shadow of a source with no width still
    /// covers its halo, `4 · 2 = 8` px about it, while an inset one covers only its source. A
    /// transparent shadow paints nothing over any source. A fade scales its alpha, 1 to 0.5, and
    /// a fade of 1 changes nothing.
    #[test]
    fn a_shadow_paints_where_it_covers() {
        let sliver = PlacedBox {
            rect: Rect::new(10.0, 10.0, 0.0, 20.0),
            corners: Corners::ZERO,
        };
        let drop = Shadow::drop(RgbaF32::BLACK, Vec2::ZERO, 2.0);
        let cast = |shadow| DrawShadowPayload::new(sliver, LoweredShadow::lower(shadow));
        assert!(!cast(drop).is_noop());
        assert!(cast(drop.inset()).is_noop());
        assert!(cast(Shadow::drop(RgbaF32::TRANSPARENT, Vec2::ZERO, 2.0)).is_noop());

        let shadow = cast(drop);
        assert_eq!(shadow.faded(1.0), shadow);
        assert_eq!(RgbaF32::from(shadow.faded(0.5).shadow.color).a, 0.5);
        assert!(shadow.faded(0.0).is_noop());
    }
}
