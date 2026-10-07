//! How a lowered shape fills: a colour or an interned gradient.

use crate::primitives::math::nan::NanCheck;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::scene::record_store::recorded_gradients::GradientId;

/// No `Hash` derive: a `Gradient`'s [`GradientId`] is a per-frame index, so a derived hash would change every
/// frame; [`Self::hash_parts`] folds a fill into a key.
#[derive(Clone, Copy, Debug)]
pub(crate) enum ShapeBrush {
    Solid(RgbaF16),
    /// An interned gradient: `id` is where the content is this frame, `hash` what it is (computed at lowering).
    Gradient {
        id: GradientId,
        hash: u64,
    },
}

/// What a curve's stroke colour multiplies: nothing, or an interned ramp.
///
/// Inline pair, not a struct, so the value is 16 B and `ShapeRecord::Curve` fits its 88 B.
#[derive(Clone, Copy, Debug)]
pub(crate) enum CurveRamp {
    None,
    Interned { id: GradientId, hash: u64 },
}

/// What a lowered fill contributes to a hash: a variant tag and payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct BrushHash {
    pub(crate) tag: u8,
    pub(crate) payload: u64,
}

impl ShapeBrush {
    /// Hash identity: a `Gradient` contributes its content hash, never its frame-local id.
    #[inline]
    pub(crate) const fn hash_parts(self) -> BrushHash {
        match self {
            Self::Solid(color) => BrushHash {
                tag: 0,
                payload: color.as_u64(),
            },
            Self::Gradient { id: _, hash } => BrushHash {
                tag: 1,
                payload: hash,
            },
        }
    }
}

/// Gradient geometry is screened at intern time, so only the solid variant needs testing.
impl NanCheck for ShapeBrush {
    #[inline]
    fn has_nan(&self) -> bool {
        match self {
            Self::Solid(color) => color.has_nan(),
            Self::Gradient { .. } => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::primitives::paint::color::RgbaF32;
    use crate::primitives::paint::color::rgba_f16::RgbaF16;
    use crate::scene::record_store::recorded_gradients::GradientId;
    use crate::shape::paint::shape_brush::ShapeBrush;

    /// A gradient's hash payload is its lowering-time content hash, not its frame-local id: equal
    /// stops under different ids hash alike; the same id under different stops does not.
    #[test]
    fn a_gradient_hashes_by_its_stops_and_not_its_frame_local_id() {
        let gradient = |id, hash| {
            ShapeBrush::Gradient {
                id: GradientId(id),
                hash,
            }
            .hash_parts()
        };
        let a = gradient(1, 0xabcd);
        let b = gradient(7, 0xabcd);
        let c = gradient(1, 0x1234);
        assert_eq!(a, b, "the id is frame-local and excluded");
        assert_ne!(a, c, "the stops decide");

        let solid = ShapeBrush::Solid(RgbaF32::WHITE.into()).hash_parts();
        assert_ne!(solid.tag, a.tag, "a solid and a gradient never collide");
        assert_eq!(solid.payload, RgbaF16::from(RgbaF32::WHITE).as_u64());
    }
}
