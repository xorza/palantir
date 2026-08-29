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

    /// Take a handle back as this pass's own, or panic if it belongs to
    /// another. Backs [`crate::Ui::intern`]'s already-interned arm, the
    /// one input that reaches a widget without being copied.
    #[must_use]
    pub(crate) fn reuse(&self, text: InternedStr) -> InternedStr {
        self.payloads.borrow().text.reuse(text)
    }

    /// Lower a handle this pass minted into the span and content hash a
    /// `ShapeRecord::Text` carries.
    pub(crate) fn record_text(&self, text: InternedStr) -> RecordedText {
        self.payloads.borrow().text.record(text)
    }
}

#[cfg(test)]
mod tests;
