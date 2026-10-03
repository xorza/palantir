//! How a lowered shape fills: a colour or an interned gradient.

use crate::primitives::color::rgba_f16::RgbaF16;
use crate::primitives::nan::NanCheck;
use crate::scene::record_store::recorded_gradients::GradientId;

/// No `Hash` derive, deliberately: a `Gradient`'s [`GradientId`] is a
/// per-frame index into the record store, so a derived hash would make
/// the same paint a different value on every frame.
/// [`Self::hash_parts`] is the one way to fold a fill into a key.
#[derive(Clone, Copy, Debug)]
pub(crate) enum ShapeBrush {
    Solid(RgbaF16),
    /// A gradient interned in the record store. The reference and its
    /// content hash travel as one value, the way a
    /// [`RecordedText`](crate::primitives::recorded_text::RecordedText)
    /// carries its span and hash: `id` is where the content is this
    /// frame, and `hash` is what the content is, computed once at
    /// lowering by [`brush`](crate::scene::shapes::lower::brush).
    Gradient {
        id: GradientId,
        hash: u64,
    },
}

/// What a curve's stroke colour multiplies: nothing, or a ramp interned
/// in the record store.
///
/// The interned pair is inline in the variant rather than a struct of
/// its own: a struct would carry its padding with it, and the tag could
/// not share the id's word. Inline, the value is 16 B, which is what
/// lets `ShapeRecord::Curve` fit its 88 B.
#[derive(Clone, Copy, Debug)]
pub(crate) enum CurveRamp {
    None,
    /// `id` is where the ramp is this frame and `hash` is what it is —
    /// the same split as [`ShapeBrush::Gradient`].
    Interned {
        id: GradientId,
        hash: u64,
    },
}

/// What a lowered fill contributes to a hash: a variant tag and the
/// payload the variant's identity is in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct BrushHash {
    pub(crate) tag: u8,
    pub(crate) payload: u64,
}

impl ShapeBrush {
    /// This fill's hash identity.
    ///
    /// A `Gradient`'s [`GradientId`] is a frame-local index into the
    /// record store, so it is never the payload: the content hash it
    /// carries beside the id is.
    ///
    /// One derivation, read by the shape-record hash and by the chrome
    /// hash, so a fill cannot be one thing to the damage diff and
    /// another to the measure cache.
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

/// A gradient's geometry never reaches this far: `lower::brush` screens
/// it at intern time, so a `Gradient` here is known-finite and only its
/// solid sibling needs testing.
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
    use crate::primitives::color::RgbaF32;
    use crate::primitives::color::rgba_f16::RgbaF16;
    use crate::scene::record_store::recorded_gradients::GradientId;
    use crate::scene::shapes::paint::shape_brush::ShapeBrush;

    /// A gradient's hash payload is the lowering-time content hash, not
    /// its frame-local id: two records naming different ids under the
    /// same stops hash alike, and the same id under different stops does
    /// not.
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
