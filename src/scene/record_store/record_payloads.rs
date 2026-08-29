//! One window's retained record-pass payload storage.

use crate::primitives::color::ColorU8;
use crate::primitives::interned_text::InternedText;
use crate::primitives::mesh::Mesh;
use crate::scene::record_store::recorded_gradients::RecordedGradients;
use crate::scene::record_store::text_store::TextStore;
use glam::Vec2;

/// Payloads for one window's retained record. All bulk shape-geometry bytes
/// live here until the next record pass and are read by every later phase via
/// spans recorded on tree shape records and the encoder's paint payloads.
#[derive(Default, Debug)]
pub(crate) struct RecordPayloads {
    /// User-supplied mesh geometry (`Shape::Mesh`), written at record
    /// time only — compose reads the payloads, never appends.
    pub(crate) meshes: Mesh,
    /// Point storage for `ShapeRecord::Polyline`. Indexed by the
    /// record's `points` `Span`.
    pub(crate) polyline_points: Vec<Vec2>,
    /// Color storage for `ShapeRecord::Polyline`. Length per
    /// record is 1, `points.len()`, or `points.len() - 1` per
    /// `ColorMode`. Stored as `ColorU8` (4 B/elem, same precision
    /// the `CurveInstance` color lanes carry) — quantization happens
    /// once at lowering, not per-emitted-instance.
    pub(crate) polyline_colors: Vec<ColorU8>,
    /// Interned record-scoped gradient payloads. `ShapeBrush::Gradient(id)`
    /// (set by `shapes::lower::brush`) indexes into its records. Cross-tree —
    /// storing it here means chrome lowering on one tree and
    /// shape lowering on another share one pool, and the encoder only
    /// needs the record payloads (not the originating tree) to resolve a
    /// gradient id.
    pub(crate) gradients: RecordedGradients,
    pub(super) text: TextStore,
}

impl RecordPayloads {
    /// Drop every payload the last record pass appended, keeping the
    /// capacity a steady scene re-fills each frame.
    pub(crate) fn clear(&mut self) {
        let Self {
            meshes,
            polyline_points,
            polyline_colors,
            gradients,
            text,
        } = self;
        meshes.clear();
        polyline_points.clear();
        polyline_colors.clear();
        gradients.clear();
        text.clear();
    }

    pub(crate) fn interned_text(&self) -> InternedText<'_> {
        InternedText::new(self.text.bytes())
    }
}
