//! Per-window store for retained record payloads. Owned by [`Forest`], which
//! pairs it with the trees whose shapes reference it. Later CPU and GPU phases
//! borrow that window's payloads through explicit frame inputs.
//! Cleared at record-pass start and retained across `PaintOnly` frames.
//!
//! One retained payload store rather than a three-step copy (user `Mesh` →
//! `Tree.shapes.payloads` → an intermediate command stream →
//! `RenderBuffer.meshes`). Shape records on
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
    ///
    /// `&mut self` because the only caller (`Forest::pre_record`) holds
    /// one: a reset that went through the cell would be asking at runtime
    /// what the borrow checker already knows here.
    pub(crate) fn clear(&mut self) {
        self.payloads.get_mut().clear();
    }

    /// Copy `s` into the record-pass text storage and return an arena-backed
    /// [`InternedStr`]. Backs [`crate::Ui::intern`] for the format-less
    /// case (plain `&str` borrow, no `format_args!`).
    #[must_use]
    pub(crate) fn intern_str(&self, s: &str) -> InternedStr {
        self.payloads.borrow_mut().text.intern_str(s)
    }

    /// Format `args` directly into the record-pass text storage and return
    /// an arena-backed [`InternedStr`] spanning the freshly-written bytes.
    /// Backs [`crate::Ui::fmt`].
    #[must_use]
    pub(crate) fn intern_fmt(&self, args: std::fmt::Arguments<'_>) -> InternedStr {
        self.payloads.borrow_mut().text.intern_fmt(args)
    }

    /// Normalize user-facing text into storage owned by this record pass.
    /// Handles from another arena are copied once so every recorded span
    /// resolves against `RecordPayloads::interned_text`.
    pub(crate) fn record_text(&self, text: InternedStr) -> RecordedText {
        self.payloads.borrow().text.record(text)
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

    /// Two properties, in priority order. **A hit is confirmed by
    /// equality**, so two distinct gradients that land on one hash never
    /// share an id — that one is correctness, and a shape painted with
    /// the wrong gradient is what it buys off. **Dedup is by hash**, so
    /// the repeat of a gradient whose key is uncontested returns the id
    /// it already has, while a colliding pair each mint a fresh record:
    /// wasted rows, never a wrong one.
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
        // The uncontested repeat: same hash, same content, same id, no
        // second record.
        assert_eq!(gradients.intern(7, first.clone()), first_id);
        assert_eq!(gradients.records.len(), 1);

        // The collision: same hash, different content. Equality
        // confirmation refuses to hand back `first_id`, which is the
        // property that matters, and mints a record of its own.
        let colliding_id = gradients.intern(7, colliding.clone());
        assert_ne!(first_id, colliding_id);
        assert_eq!(gradients.records.len(), 2);
        assert_eq!(gradients.records[colliding_id.0 as usize], colliding);

        // Dedup — and only dedup — is what the collision costs: each of
        // the pair now displaces the other's candidate, so both keep
        // minting records rather than one being wrongly reused.
        assert_ne!(gradients.intern(7, first), first_id);
        assert_ne!(gradients.intern(7, colliding), colliding_id);
        assert_eq!(gradients.records.len(), 4);

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
