//! Per-window store for retained record payloads. Owned by [`Forest`], which
//! pairs it with the trees whose shapes reference it. Later CPU and GPU phases
//! borrow that window's store through explicit frame inputs.
//! Cleared at record-pass start and retained across `PaintOnly` frames.
//!
//! One retained payload store rather than a three-step copy (user `Mesh` →
//! `Tree.shapes.payloads` → an intermediate command stream →
//! `RenderBuffer.meshes`). Shape records on
//! the tree, the paint payloads the encoder hands the composer, and `MeshDraw`
//! entries on the render buffer all carry spans into this storage directly.
//!
//! This module is storage and the staging calls that fill it: the
//! authoring `Shape` → `ShapeRecord` / `ChromeRow` lowering that decides
//! *what* to stage lives in [`crate::scene::shapes::lower`].
//!
//! [`Forest`]: crate::scene::forest::Forest

pub(crate) mod recorded_gradient;
pub(crate) mod recorded_gradients;
pub(crate) mod text_store;

use crate::primitives::color::{Color, ColorU8};
use crate::primitives::interned_str::InternedStr;
use crate::primitives::interned_text::InternedText;
use crate::primitives::mesh::Mesh;
use crate::primitives::recorded_text::RecordedText;
use crate::primitives::span::Span;
use crate::primitives::text_input::TextInput;
use crate::scene::record_store::recorded_gradient::RecordedGradient;
use crate::scene::record_store::recorded_gradients::{GradientId, RecordedGradients};
use crate::scene::record_store::text_store::TextStore;
use glam::Vec2;

/// The payload columns themselves, and the only API that appends to them.
///
/// Every writer holds `&mut Forest`, so the write API is `&mut self` and
/// the exclusion is the borrow checker's — a shared cell here would ask
/// per lowered shape, at run time, what is already known at compile time.
#[derive(Default, Debug)]
pub(crate) struct RecordStore {
    /// User-supplied mesh geometry (`Shape::Mesh`), written at record
    /// time only — compose reads it, never appends.
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
    /// needs this store (not the originating tree) to resolve a
    /// gradient id.
    pub(crate) gradients: RecordedGradients,
    text: TextStore,
}

/// Where [`RecordStore::stage_mesh`] put one mesh's geometry.
#[derive(Clone, Copy, Debug)]
pub(super) struct MeshSpans {
    pub(super) vertices: Span,
    pub(super) indices: Span,
}

/// Where [`RecordStore::stage_polyline`] put one polyline's geometry.
#[derive(Clone, Copy, Debug)]
pub(super) struct PolylineSpans {
    pub(super) points: Span,
    pub(super) colors: Span,
}

impl RecordStore {
    /// Drop every payload the last record pass appended, keeping the
    /// capacity a steady scene re-fills each frame.
    ///
    /// PaintOnly skips this so the retained tree and this storage remain
    /// valid together.
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

    /// Normalize borrowed, owned, or already-interned text into an
    /// [`InternedStr`] of this pass. Backs [`crate::Ui::intern`].
    ///
    /// Borrowed and owned inputs are copied into the record-pass text
    /// arena. An already-interned handle is the one arm that copies
    /// nothing, and so the one whose handle was not just minted here —
    /// it is screened through [`Self::reuse`] rather than passed
    /// through, because a stale one resolves to whatever text now sits
    /// at those offsets.
    #[must_use]
    pub(crate) fn intern<'a>(&mut self, text: impl Into<TextInput<'a>>) -> InternedStr {
        match text.into() {
            TextInput::Borrowed(text) => self.intern_str(text),
            TextInput::Owned(text) => self.intern_str(&text),
            TextInput::Interned(text) => self.reuse(text),
        }
    }

    /// Copy `s` into the record-pass text storage and return an arena-backed
    /// [`InternedStr`].
    #[must_use]
    fn intern_str(&mut self, s: &str) -> InternedStr {
        self.text.intern_str(s)
    }

    /// Format `args` directly into the record-pass text storage and return
    /// an arena-backed [`InternedStr`] spanning the freshly-written bytes.
    /// Backs [`crate::Ui::fmt`].
    #[must_use]
    pub(crate) fn intern_fmt(&mut self, args: std::fmt::Arguments<'_>) -> InternedStr {
        self.text.intern_fmt(args)
    }

    /// Take a handle back as this pass's own, or panic if it belongs to
    /// another — [`Self::intern`]'s already-interned arm, the one input
    /// that reaches a widget without being copied.
    #[must_use]
    fn reuse(&self, text: InternedStr) -> InternedStr {
        self.text.reuse(text)
    }

    /// Lower a handle this pass minted into the span and content hash a
    /// `ShapeRecord::Text` carries.
    pub(crate) fn record_text(&self, text: InternedStr) -> RecordedText {
        self.text.record(text)
    }

    /// Intern one gradient payload under its content `hash`, returning
    /// the id a `ShapeBrush::Gradient` carries.
    pub(super) fn intern_gradient(&mut self, hash: u64, gradient: RecordedGradient) -> GradientId {
        self.gradients.intern(hash, gradient)
    }

    /// Copy one mesh's vertices and indices in, returning the spans a
    /// `ShapeRecord::Mesh` carries.
    pub(super) fn stage_mesh(&mut self, mesh: &Mesh) -> MeshSpans {
        let meshes = &mut self.meshes;
        let vertices = Span::new(meshes.vertices.len() as u32, mesh.vertices.len() as u32);
        meshes.vertices.extend_from_slice(&mesh.vertices);
        let indices = Span::new(meshes.indices.len() as u32, mesh.indices.len() as u32);
        meshes.indices.extend_from_slice(&mesh.indices);
        MeshSpans { vertices, indices }
    }

    /// Copy one polyline's points and colours in, quantizing the colours
    /// once here rather than per emitted instance. Returns the spans a
    /// `ShapeRecord::Polyline` carries.
    pub(super) fn stage_polyline(&mut self, points: &[Vec2], colors: &[Color]) -> PolylineSpans {
        let staged_points = Span::new(self.polyline_points.len() as u32, points.len() as u32);
        self.polyline_points.extend_from_slice(points);
        let staged_colors = Span::new(self.polyline_colors.len() as u32, colors.len() as u32);
        self.polyline_colors
            .extend(colors.iter().map(|&c| ColorU8::from(c)));
        PolylineSpans {
            points: staged_points,
            colors: staged_colors,
        }
    }
}

#[cfg(test)]
mod tests;
