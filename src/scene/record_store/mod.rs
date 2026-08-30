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
//! This module is storage and the staging calls that fill it: the
//! authoring `Shape` → `ShapeRecord` / `ChromeRow` lowering that decides
//! *what* to stage lives in [`crate::scene::shapes::lower`].
//!
//! [`Forest`]: crate::scene::forest::Forest

pub(crate) mod record_payloads;
pub(crate) mod recorded_gradient;
pub(crate) mod recorded_gradients;
pub(crate) mod text_store;

use crate::primitives::color::{Color, ColorU8};
use crate::primitives::interned_str::InternedStr;
use crate::primitives::mesh::Mesh;
use crate::primitives::recorded_text::RecordedText;
use crate::primitives::span::Span;
use crate::primitives::text_input::TextInput;
use crate::scene::record_store::record_payloads::RecordPayloads;
use crate::scene::record_store::recorded_gradient::RecordedGradient;
use crate::scene::record_store::recorded_gradients::GradientId;
use glam::Vec2;

/// Owner of one window's retained record payloads, and the only way to
/// append to them. `Forest` owns one; frontend and backend phases read
/// the same payloads through [`Self::payloads`].
///
/// Every writer holds `&mut Forest`, so the write API is `&mut self` and
/// the exclusion is the borrow checker's — a shared cell here would ask
/// per lowered shape, at run time, what is already known at compile time.
#[derive(Default, Debug)]
pub(crate) struct RecordStore {
    payloads: RecordPayloads,
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
    /// This pass's payloads, as every later phase reads them.
    pub(crate) fn payloads(&self) -> &RecordPayloads {
        &self.payloads
    }

    /// Drop all record-pass storage.
    /// PaintOnly skips this so the retained tree and payload storage remain
    /// valid together.
    pub(crate) fn clear(&mut self) {
        self.payloads.clear();
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
        self.payloads.text.intern_str(s)
    }

    /// Format `args` directly into the record-pass text storage and return
    /// an arena-backed [`InternedStr`] spanning the freshly-written bytes.
    /// Backs [`crate::Ui::fmt`].
    #[must_use]
    pub(crate) fn intern_fmt(&mut self, args: std::fmt::Arguments<'_>) -> InternedStr {
        self.payloads.text.intern_fmt(args)
    }

    /// Take a handle back as this pass's own, or panic if it belongs to
    /// another — [`Self::intern`]'s already-interned arm, the one input
    /// that reaches a widget without being copied.
    #[must_use]
    fn reuse(&self, text: InternedStr) -> InternedStr {
        self.payloads.text.reuse(text)
    }

    /// Lower a handle this pass minted into the span and content hash a
    /// `ShapeRecord::Text` carries.
    pub(crate) fn record_text(&self, text: InternedStr) -> RecordedText {
        self.payloads.text.record(text)
    }

    /// Intern one gradient payload under its content `hash`, returning
    /// the id a `ShapeBrush::Gradient` carries.
    pub(super) fn intern_gradient(&mut self, hash: u64, gradient: RecordedGradient) -> GradientId {
        self.payloads.gradients.intern(hash, gradient)
    }

    /// Copy one mesh's vertices and indices in, returning the spans a
    /// `ShapeRecord::Mesh` carries.
    pub(super) fn stage_mesh(&mut self, mesh: &Mesh) -> MeshSpans {
        let meshes = &mut self.payloads.meshes;
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
        let payloads = &mut self.payloads;
        let staged_points = Span::new(payloads.polyline_points.len() as u32, points.len() as u32);
        payloads.polyline_points.extend_from_slice(points);
        let staged_colors = Span::new(payloads.polyline_colors.len() as u32, colors.len() as u32);
        payloads
            .polyline_colors
            .extend(colors.iter().map(|&c| ColorU8::from(c)));
        PolylineSpans {
            points: staged_points,
            colors: staged_colors,
        }
    }
}

#[cfg(test)]
mod tests;
