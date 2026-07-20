//! Per-window store for retained record payloads. Owned by `Ui`, which handles
//! record-time mesh / polyline / formatting writes. Later CPU and GPU phases
//! borrow that window's payloads through the same `Ui`.
//! Cleared at record-pass start and retained across `PaintOnly` frames.
//!
//! Replaces the previous three-step copy (user `Mesh` →
//! `Tree.shapes.payloads` → `RenderCmdBuffer.shape_payloads` →
//! `RenderBuffer.meshes`) with a single retained payload store. Shape records on
//! the tree, payloads on the cmd buffer, and `MeshDraw` entries on the
//! render buffer all carry spans into this storage directly.
//!
//! This file is storage only: the authoring `Shape` → `ShapeRecord` /
//! `ChromeRow` lowering that appends here lives in
//! [`crate::forest::shapes::lower`].

use crate::common::hash::hash_str;
use crate::primitives::brush::{FillAxis, GradientStops, Interp};
use crate::primitives::color::ColorU8;
use crate::primitives::fill_wire::FillKind;
use crate::primitives::interned_str::{InternedStr, RecordedText, TextArena};
use crate::primitives::mesh::Mesh;
use crate::primitives::span::Span;
use glam::Vec2;
use rustc_hash::FxHashMap;
use std::cell::{Ref, RefCell};
use std::fmt::Write as _;
use std::rc::Rc;

const GRADIENT_CHAIN_END: u32 = u32::MAX;

/// Record-local handle into [`RecordedGradients::records`].
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct GradientId(pub(crate) u32);

/// Retained gradient content. The physical atlas row is resolved while
/// encoding because the shared atlas may evict rows between window frames.
#[derive(Clone, Debug)]
pub(crate) struct RecordedGradient {
    pub(crate) axis: FillAxis,
    pub(crate) kind: FillKind,
    pub(crate) stops: GradientStops,
    pub(crate) interp: Interp,
}

impl PartialEq for RecordedGradient {
    fn eq(&self, other: &Self) -> bool {
        // Raw equality is the hot path; unpacking also collapses canonical ±0.
        (self.axis == other.axis || self.axis.lanes() == other.axis.lanes())
            && self.kind == other.kind
            && self.stops == other.stops
            && self.interp == other.interp
    }
}

/// Record-local gradient content and interning metadata under one reset boundary.
#[derive(Default, Debug)]
pub(crate) struct RecordedGradients {
    pub(crate) records: Vec<RecordedGradient>,
    heads: FxHashMap<u64, GradientId>,
    next: Vec<u32>,
}

impl RecordedGradients {
    pub(crate) fn intern(&mut self, content_hash: u64, gradient: RecordedGradient) -> GradientId {
        let head = self
            .heads
            .get(&content_hash)
            .copied()
            .map_or(GRADIENT_CHAIN_END, |id| id.0);
        let mut current = head;
        while current != GRADIENT_CHAIN_END {
            let idx = current as usize;
            // Equality confirmation keeps true hash collisions correct.
            if self.records[idx] == gradient {
                return GradientId(current);
            }
            current = self.next[idx];
        }

        assert!(
            self.records.len() < GRADIENT_CHAIN_END as usize,
            "recorded gradient count exceeds the u32 handle range",
        );
        let id = GradientId(self.records.len() as u32);
        self.records.push(gradient);
        self.next.push(head);
        self.heads.insert(content_hash, id);
        id
    }

    fn clear(&mut self) {
        self.records.clear();
        self.heads.clear();
        self.next.clear();
    }
}

/// Owner of one window's retained record payloads. `Ui` owns one;
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

/// Payloads for one window's retained record. All bulk shape-geometry bytes
/// live here until the next record pass and are read by every later phase via
/// spans recorded on tree shape records and cmd-buffer payloads.
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
    text: TextStore,
}

/// Cross-tree text storage for one window. Handles can cross layer scopes and
/// retain their source bytes; the active/spare pair keeps that safe while
/// recycling allocations once escaped handles drop.
#[derive(Debug, Default)]
struct TextStore {
    active: Rc<TextArena>,
    spare: Option<Rc<TextArena>>,
}

impl RecordPayloads {
    pub(crate) fn text_bytes(&self) -> Ref<'_, str> {
        self.text.bytes()
    }
}

impl TextStore {
    fn bytes(&self) -> Ref<'_, str> {
        Ref::map(self.active.bytes.borrow(), String::as_str)
    }

    fn clear(&mut self) {
        if Rc::strong_count(&self.active) == 1 {
            self.active.bytes.borrow_mut().clear();
            return;
        }

        let previous = std::mem::take(&mut self.active);
        self.active = match self.spare.take() {
            Some(arena) if Rc::strong_count(&arena) == 1 => arena,
            Some(_) | None => Rc::default(),
        };
        self.active.bytes.borrow_mut().clear();
        self.spare = Some(previous);
    }

    fn intern_str(&self, text: &str) -> InternedStr {
        let mut bytes = self.active.bytes.borrow_mut();
        let start = bytes.len();
        bytes.push_str(text);
        InternedStr::arena_backed(
            Span::new(start as u32, text.len() as u32),
            self.active.clone(),
        )
    }

    fn intern_fmt(&self, args: std::fmt::Arguments<'_>) -> InternedStr {
        let mut bytes = self.active.bytes.borrow_mut();
        let start = bytes.len();
        bytes.write_fmt(args).unwrap();
        let end = bytes.len();
        InternedStr::arena_backed(
            Span::new(start as u32, (end - start) as u32),
            self.active.clone(),
        )
    }

    fn record(&self, text: InternedStr) -> RecordedText {
        let source = text.arena.bytes.borrow();
        let source = &source[text.span.range()];
        let hash = hash_str(source);
        let span = if Rc::ptr_eq(&text.arena, &self.active) {
            text.span
        } else {
            let mut target = self.active.bytes.borrow_mut();
            let start = target.len();
            target.push_str(source);
            Span::new(start as u32, source.len() as u32)
        };
        RecordedText::new(span, hash)
    }
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
    /// resolves against `RecordPayloads::text_bytes`.
    pub(crate) fn record_text(&self, text: InternedStr) -> RecordedText {
        let payloads = self.payloads.borrow();
        payloads.text.record(text)
    }
}

#[cfg(test)]
mod tests {
    use crate::primitives::brush::{FillAxis, GradientStops, Interp, Spread, Stop};
    use crate::primitives::color::ColorU8;
    use crate::primitives::fill_wire::FillKind;
    use crate::record_store::{RecordPayloads, RecordStore, RecordedGradient, RecordedGradients};
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
