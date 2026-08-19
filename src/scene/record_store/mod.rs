//! Per-window store for retained record payloads. Owned by [`Forest`], which
//! pairs it with the trees whose shapes reference it. Later CPU and GPU phases
//! borrow that window's payloads through explicit frame inputs.
//! Cleared at record-pass start and retained across `PaintOnly` frames.
//!
//! Replaces the previous three-step copy (user `Mesh` →
//! `Tree.shapes.payloads` → an intermediate command stream →
//! `RenderBuffer.meshes`) with a single retained payload store. Shape records on
//! the tree, the paint payloads the encoder hands the composer, and `MeshDraw`
//! entries on the render buffer all carry spans into this storage directly.
//!
//! This module is storage only: the authoring `Shape` → `ShapeRecord` /
//! `ChromeRow` lowering that appends here lives in
//! [`crate::scene::shapes::lower`].
//!
//! [`Forest`]: crate::scene::forest::Forest

pub(crate) mod record_payloads;
pub(crate) mod recorded_gradient;
pub(crate) mod recorded_gradients;
pub(crate) mod text_store;

use crate::primitives::interned_str::InternedStr;
use crate::primitives::recorded_text::RecordedText;
use crate::scene::record_store::record_payloads::RecordPayloads;
use std::cell::RefCell;

/// Owner of one window's retained record payloads. `Forest` owns one;
/// frontend and backend phases receive a borrow of the same payloads.
/// Phases run sequentially (record → encode → compose → upload) so the
/// underlying borrow is never contested; a double-borrow indicates a wiring
/// bug and panics.
///
/// User-facing operations (`clear`, `intern_str`, `intern_fmt`) borrow
/// internally. Pass-orchestration code (encode/compose/intrinsic) borrows
/// `payloads` once per pass and hands `&RecordPayloads` down through it.
#[derive(Default, Debug)]
pub(crate) struct RecordStore {
    pub(crate) payloads: RefCell<RecordPayloads>,
}

impl RecordStore {
    /// Drop all record-pass storage.
    /// PaintOnly skips this so the retained tree and payload storage remain
    /// valid together.
    pub(crate) fn clear(&self) {
        let mut payloads = self.payloads.borrow_mut();
        payloads.meshes.clear();
        payloads.polyline_points.clear();
        payloads.polyline_colors.clear();
        payloads.gradients.clear();
        payloads.text.clear();
    }

    /// Copy `s` into the record-pass text storage and return an arena-backed
    /// [`InternedStr`]. Backs [`crate::Ui::intern`] for the format-less
    /// case (plain `&str` borrow, no `format_args!`).
    #[must_use]
    pub(crate) fn intern_str(&self, s: &str) -> InternedStr {
        let payloads = self.payloads.borrow();
        payloads.text.intern_str(s)
    }

    /// Format `args` directly into the record-pass text storage and return
    /// an arena-backed [`InternedStr`] spanning the freshly-written bytes.
    /// Backs [`crate::Ui::fmt`].
    #[must_use]
    pub(crate) fn intern_fmt(&self, args: std::fmt::Arguments<'_>) -> InternedStr {
        let payloads = self.payloads.borrow();
        payloads.text.intern_fmt(args)
    }

    /// Normalize user-facing text into storage owned by this record pass.
    /// Handles from another arena are copied once so every recorded span
    /// resolves against `RecordPayloads::interned_text`.
    pub(crate) fn record_text(&self, text: InternedStr) -> RecordedText {
        let payloads = self.payloads.borrow();
        payloads.text.record(text)
    }
}

#[cfg(test)]
mod tests {
    use crate::primitives::brush::gradient::FillAxis;
    use crate::primitives::brush::gradient::stops::{GradientStops, Stop};
    use crate::primitives::brush::gradient::{Interp, Spread};
    use crate::primitives::color::ColorU8;
    use crate::primitives::fill_kind::FillKind;
    use crate::scene::record_store::RecordStore;
    use crate::scene::record_store::record_payloads::RecordPayloads;
    use crate::scene::record_store::recorded_gradient::RecordedGradient;
    use crate::scene::record_store::recorded_gradients::RecordedGradients;
    use glam::Vec2;
    use std::cell::RefCell;

    #[test]
    fn record_store_owns_inline_payloads_and_stores_are_isolated() {
        assert_eq!(
            std::mem::size_of::<RecordStore>(),
            std::mem::size_of::<RefCell<RecordPayloads>>(),
        );

        let first = RecordStore::default();
        let second = RecordStore::default();
        first
            .payloads
            .borrow_mut()
            .polyline_points
            .push(Vec2::new(3.0, 5.0));

        assert_eq!(
            first.payloads.borrow().polyline_points.as_slice(),
            &[Vec2::new(3.0, 5.0)],
        );
        assert!(second.payloads.borrow().polyline_points.is_empty());
    }

    #[test]
    fn gradient_interner_confirms_equality_across_hash_collisions_and_clears() {
        let stops = GradientStops::new([
            Stop::new(0.0, ColorU8::BLACK),
            Stop::new(1.0, ColorU8::WHITE),
        ]);
        let first = RecordedGradient {
            axis: FillAxis::from_lanes(1.0, 0.0, 0.0, 1.0),
            kind: FillKind::linear(Spread::Pad),
            stops,
            interp: Interp::Oklab,
        };
        let colliding = RecordedGradient {
            axis: FillAxis::from_lanes(0.0, 1.0, 0.0, 1.0),
            ..first.clone()
        };
        let mut gradients = RecordedGradients::default();
        let first_id = gradients.intern(7, first.clone());
        let colliding_id = gradients.intern(7, colliding.clone());
        let repeated_first_id = gradients.intern(7, first);
        let repeated_colliding_id = gradients.intern(7, colliding);

        assert_ne!(first_id, colliding_id);
        assert_eq!(repeated_first_id, first_id);
        assert_eq!(repeated_colliding_id, colliding_id);
        assert_eq!(gradients.records.len(), 2);

        gradients.clear();
        let after_clear = RecordedGradient {
            axis: FillAxis::ZERO,
            kind: FillKind::linear(Spread::Reflect),
            stops,
            interp: Interp::Linear,
        };
        let after_clear_id = gradients.intern(7, after_clear);
        assert_eq!(after_clear_id.0, 0);
        assert_eq!(gradients.records.len(), 1);
    }
}
