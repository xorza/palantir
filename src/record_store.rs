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
use std::cell::{Ref, RefCell, RefMut};
use std::fmt::Write as _;
use std::rc::Rc;

/// Record-local handle into [`RecordPayloads::gradients`].
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

/// Shared owner of one window's retained record payloads. `Ui` owns one;
/// frontend and backend phases receive a borrow of the same payloads.
/// Phases run sequentially (record → encode → compose → upload) so the
/// underlying borrow is never contested; a double-borrow indicates a wiring
/// bug and panics.
///
/// User-facing operations (`clear`, `intern_str`, `intern_fmt`) borrow
/// internally. Pass-orchestration code (encode/compose/intrinsic) uses
/// [`Self::borrow`] / [`Self::borrow_mut`] once per pass and hands
/// `&RecordPayloads` down through it.
#[derive(Clone, Default, Debug)]
pub(crate) struct RecordStore {
    payloads: Rc<RefCell<RecordPayloads>>,
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
    /// Record-scoped gradient payloads. `ShapeBrush::Gradient(id)` (set
    /// by `shapes::lower::brush`) indexes into this vec. Cross-tree — storing
    /// it here means chrome lowering on one tree and
    /// shape lowering on another share one pool, and the encoder only
    /// needs the record payloads (not the originating tree) to resolve a
    /// gradient id.
    pub(crate) gradients: Vec<RecordedGradient>,
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
    pub(crate) fn record_gradient(&mut self, gradient: RecordedGradient) -> GradientId {
        let id = GradientId(
            u32::try_from(self.gradients.len()).expect("recorded gradient count exceeds u32"),
        );
        self.gradients.push(gradient);
        id
    }

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
    pub(crate) fn borrow(&self) -> Ref<'_, RecordPayloads> {
        self.payloads.borrow()
    }

    pub(crate) fn borrow_mut(&self) -> RefMut<'_, RecordPayloads> {
        self.payloads.borrow_mut()
    }

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
