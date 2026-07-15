//! Per-window store for retained record payloads. Owned by `WindowRenderer`,
//! with a cheap `Rc` clone held by its `Ui` for record-time mesh / polyline /
//! formatting writes. Later CPU and GPU phases borrow that window's payloads
//! explicitly.
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
use crate::primitives::brush::FillAxis;
use crate::primitives::color::ColorU8;
use crate::primitives::fill_wire::{FillKind, LutRow};
use crate::primitives::interned_str::InternedStr;
use crate::primitives::mesh::Mesh;
use crate::primitives::span::Span;
use glam::Vec2;
use std::cell::{Ref, RefCell, RefMut};
use std::fmt::Write as _;
use std::rc::Rc;

/// Record-local handle into [`RecordPayloads::gradients`].
pub(crate) type GradientId = u32;

/// Pre-baked gradient payload stored with the recording that owns its lifetime.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct LoweredGradient {
    pub(crate) axis: FillAxis,
    pub(crate) row: LutRow,
    pub(crate) kind: FillKind,
}

/// Shared owner of one window's retained record payloads. `WindowRenderer`
/// constructs one and clones it into its `Ui`; frontend and backend phases
/// receive a borrow of the same payloads.
/// Phases run sequentially (record → encode → compose → upload) so the
/// underlying borrow is never contested; a double-borrow indicates a wiring
/// bug and panics.
///
/// User-facing operations (`clear`, `intern_str`, `intern_fmt`) borrow
/// internally. Pass-orchestration code (encode/compose/intrinsic) uses
/// [`Self::borrow`] / [`Self::borrow_mut`] once per pass and hands
/// `&RecordPayloads` down through it.
#[derive(Clone, Default, Debug)]
pub struct RecordStore {
    payloads: Rc<RefCell<RecordPayloads>>,
}

/// Payloads for one window's retained record. All bulk shape-geometry bytes
/// live here until the next record pass and are read by every later phase via
/// spans recorded on tree shape records and cmd-buffer payloads.
#[derive(Default, Debug)]
pub(crate) struct RecordPayloads {
    /// Incremented by every [`RecordStore::clear`]. Record-local text
    /// handles capture this value so a later record pass cannot reuse
    /// their span and cached hash against replacement bytes.
    record_pass_generation: u64,
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
    /// Frame-scoped gradient payloads. `ShapeBrush::Gradient(id)` (set
    /// by `shapes::lower::brush`) indexes into this vec. Cross-tree — storing
    /// it here means chrome lowering on one tree and
    /// shape lowering on another share one pool, and the encoder only
    /// needs the record payloads (not the originating tree) to resolve a
    /// gradient id.
    pub(crate) gradients: Vec<LoweredGradient>,
    /// `Ui::fmt` formatter scratch. Frame-local handles returned by
    /// [`RecordStore::intern_fmt`] point into this buffer; owned text
    /// keeps its bytes inline on `ShapeRecord::Text`. Cross-tree on
    /// purpose so handles survive `Ui::layer(...)` scopes. Cleared per
    /// record pass, capacity retained — steady-state `ui.fmt(...)`
    /// flows skip the `format!() → String` allocation entirely.
    pub(crate) fmt_scratch: String,
}

impl RecordStore {
    pub(crate) fn borrow(&self) -> Ref<'_, RecordPayloads> {
        self.payloads.borrow()
    }

    pub(crate) fn borrow_mut(&self) -> RefMut<'_, RecordPayloads> {
        self.payloads.borrow_mut()
    }

    /// Drop all record-pass storage and invalidate its text handles.
    /// PaintOnly skips this so the retained tree and payload generation
    /// remain valid together.
    pub(crate) fn clear(&self) {
        let mut payloads = self.payloads.borrow_mut();
        payloads.record_pass_generation = payloads
            .record_pass_generation
            .checked_add(1)
            .expect("RecordStore generation overflowed");
        payloads.meshes.clear();
        payloads.polyline_points.clear();
        payloads.polyline_colors.clear();
        payloads.gradients.clear();
        payloads.fmt_scratch.clear();
    }

    /// Copy `s` into the record-pass text storage and return a frame-local
    /// [`InternedStr`]. Backs [`crate::Ui::intern`] for the format-less
    /// case (plain `&str` borrow, no `format_args!`).
    #[must_use]
    pub(crate) fn intern_str(&self, s: &str) -> InternedStr {
        let mut payloads = self.payloads.borrow_mut();
        let start = payloads.fmt_scratch.len();
        payloads.fmt_scratch.push_str(s);
        let hash = hash_str(s);
        InternedStr::frame_local(
            Span::new(start as u32, s.len() as u32),
            hash,
            payloads.record_pass_generation,
        )
    }

    /// Format `args` directly into the record-pass text storage and return
    /// a frame-local [`InternedStr`] spanning the freshly-written bytes.
    /// Backs [`crate::Ui::fmt`].
    #[must_use]
    pub(crate) fn intern_fmt(&self, args: std::fmt::Arguments<'_>) -> InternedStr {
        let mut payloads = self.payloads.borrow_mut();
        let start = payloads.fmt_scratch.len();
        payloads.fmt_scratch.write_fmt(args).unwrap();
        let end = payloads.fmt_scratch.len();
        let bytes = &payloads.fmt_scratch.as_str()[start..end];
        let hash = hash_str(bytes);
        InternedStr::frame_local(
            Span::new(start as u32, (end - start) as u32),
            hash,
            payloads.record_pass_generation,
        )
    }

    /// Enforce that a frame-local text handle belongs to the active
    /// record pass before its cached hash enters the shape tree.
    #[inline]
    pub(crate) fn assert_text_generation(&self, generation: u64) {
        debug_assert_eq!(
            generation,
            self.payloads.borrow().record_pass_generation,
            "frame-local text reused after record store reset",
        );
    }
}
