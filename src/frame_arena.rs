//! Per-frame bulk geometry arena. Owned by `WindowRenderer`, cloned (cheap, Rc)
//! into every subsystem that touches per-frame mesh / polyline / fmt
//! bytes (`Ui`, `Frontend`, `WgpuBackend`). Cleared at record-pass start.
//!
//! Replaces the previous three-step copy (user `Mesh` →
//! `Tree.shapes.payloads` → `RenderCmdBuffer.shape_payloads` →
//! `RenderBuffer.meshes.arena`) with a single arena. Shape records on
//! the tree, payloads on the cmd buffer, and `MeshDraw` entries on the
//! render buffer all carry spans into this arena directly.
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

/// Frame-local handle into [`FrameArenaInner::gradients`].
pub(crate) type GradientId = u32;

/// Pre-baked gradient payload stored in the arena that owns its lifetime.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct LoweredGradient {
    pub(crate) axis: FillAxis,
    pub(crate) row: LutRow,
    pub(crate) kind: FillKind,
}

/// Shared per-frame arena. `WindowRenderer` constructs one and clones it into
/// every subsystem (`Ui`, `Frontend`, `WgpuBackend`). Phases run
/// sequentially (record → encode → compose → upload) so the underlying
/// borrow is never contested; a double-borrow indicates a wiring bug
/// and panics.
///
/// User-facing operations (`clear`, `intern_str`, `intern_fmt`) take
/// `&self` and borrow internally — call sites never touch RefCell.
/// Pass-orchestration code (encode/compose/intrinsic) reaches the raw
/// storage via [`Self::inner`] / [`Self::inner_mut`] once per pass and
/// hands `&FrameArenaInner` down through the pass.
#[derive(Clone, Default, Debug)]
pub struct FrameArena(Rc<RefCell<FrameArenaInner>>);

/// One arena per frame. All bulk shape-geometry bytes live here for
/// the duration of a frame and are read by every later phase via
/// spans recorded on tree shape records and cmd-buffer payloads.
#[derive(Default, Debug)]
pub(crate) struct FrameArenaInner {
    /// Incremented by every [`FrameArena::clear`]. Frame-local text
    /// handles capture this value so a later record pass cannot reuse
    /// their span and cached hash against replacement bytes.
    record_pass_generation: u64,
    /// User-supplied mesh geometry (`Shape::Mesh`), written at record
    /// time only — compose reads the arena, never appends.
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
    /// by `shapes::lower::brush`) indexes into this vec. Cross-tree — keeping
    /// it on the frame arena means chrome lowering on one tree and
    /// shape lowering on another share one pool, and the encoder only
    /// needs the arena (not the originating tree) to resolve a
    /// gradient id.
    pub(crate) gradients: Vec<LoweredGradient>,
    /// `Ui::fmt` formatter scratch. Frame-local handles returned by
    /// [`FrameArena::intern_fmt`] point into this buffer; owned text
    /// keeps its bytes inline on `ShapeRecord::Text`. Cross-tree on
    /// purpose so handles survive `Ui::layer(...)` scopes. Cleared per
    /// record pass, capacity retained — steady-state `ui.fmt(...)`
    /// flows skip the `format!() → String` allocation entirely.
    pub(crate) fmt_scratch: String,
}

impl FrameArena {
    /// Borrow the raw inner storage for the duration of a pass. Used
    /// by encode/compose/intrinsic — the orchestrator opens one borrow
    /// at pass entry and threads `&FrameArenaInner` down so per-node
    /// code touches fields directly. Authoring code (widgets, tests)
    /// should prefer the `shapes::lower` / `intern_fmt` entry points.
    pub(crate) fn inner(&self) -> Ref<'_, FrameArenaInner> {
        self.0.borrow()
    }

    /// Mutable counterpart to [`Self::inner`] — record-time writers
    /// (shape lowering, mesh staging) and the per-frame `clear`.
    pub(crate) fn inner_mut(&self) -> RefMut<'_, FrameArenaInner> {
        self.0.borrow_mut()
    }

    /// Drop all record-pass storage and invalidate its text handles.
    /// PaintOnly skips this so the retained tree and arena generation
    /// remain valid together.
    pub(crate) fn clear(&self) {
        let mut a = self.0.borrow_mut();
        a.record_pass_generation = a
            .record_pass_generation
            .checked_add(1)
            .expect("FrameArena record-pass generation overflowed");
        a.meshes.clear();
        a.polyline_points.clear();
        a.polyline_colors.clear();
        a.gradients.clear();
        a.fmt_scratch.clear();
    }

    /// Copy `s` into the record-pass text arena and return a frame-local
    /// [`InternedStr`]. Backs [`crate::Ui::intern`] for the format-less
    /// case (plain `&str` borrow, no `format_args!`).
    #[must_use]
    pub(crate) fn intern_str(&self, s: &str) -> InternedStr {
        let mut a = self.0.borrow_mut();
        let start = a.fmt_scratch.len();
        a.fmt_scratch.push_str(s);
        let hash = hash_str(s);
        InternedStr::frame_local(
            Span::new(start as u32, s.len() as u32),
            hash,
            a.record_pass_generation,
        )
    }

    /// Format `args` directly into the record-pass text arena and return
    /// a frame-local [`InternedStr`] spanning the freshly-written bytes.
    /// Backs [`crate::Ui::fmt`].
    #[must_use]
    pub(crate) fn intern_fmt(&self, args: std::fmt::Arguments<'_>) -> InternedStr {
        let mut a = self.0.borrow_mut();
        let start = a.fmt_scratch.len();
        a.fmt_scratch.write_fmt(args).unwrap();
        let end = a.fmt_scratch.len();
        let bytes = &a.fmt_scratch.as_str()[start..end];
        let hash = hash_str(bytes);
        InternedStr::frame_local(
            Span::new(start as u32, (end - start) as u32),
            hash,
            a.record_pass_generation,
        )
    }

    /// Enforce that a frame-local text handle belongs to the active
    /// record pass before its cached hash enters the shape tree.
    #[inline]
    pub(crate) fn assert_text_generation(&self, generation: u64) {
        debug_assert_eq!(
            generation,
            self.0.borrow().record_pass_generation,
            "frame-local text reused after arena reset",
        );
    }
}
