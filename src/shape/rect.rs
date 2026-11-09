//! The rectangle builder. Lowers to `ShapeRecord::Quad(QuadShape::Rect)`.

use crate::effect::effect_registry::EffectRegistry;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::reduction::Reductions;
use crate::shape::lower::{self, LoweredBox};
use crate::shape::paint::lowered_material::MaterialRead;
use crate::shape::paint::quad_shape::QuadShape;
use crate::shape::record::ShapeRecord;
use crate::shape::record_store::RecordStore;
use crate::shape::sealed;

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum RectKind {
    /// Fill inside the rounded boundary.
    Rounded = 0,
    /// Fill outside the rounded boundary, leaving its interior transparent.
    Windowed = 1,
}

/// A box a [`Chrome`] paints: its fill, border, corners, shadow and material, over the owner's
/// arranged rect or [`Self::at`] a rect of its own.
#[derive(Clone, Debug)]
#[must_use]
pub struct RectShape {
    pub(crate) kind: RectKind,
    pub(crate) local_rect: Option<Rect>,
    pub(crate) chrome: Chrome,
    /// What `Ui::add_shape` finds the material reads.
    material: MaterialState,
}

/// What a [`RectShape`]'s material reads, which only the resolve before it lowers knows.
#[derive(Clone, Copy, Debug)]
enum MaterialState {
    Unresolved,
    /// `None` where the box paints no material: none set, or one that paints nothing for what is
    /// reduced.
    Resolved(Option<MaterialRead>),
}

impl RectShape {
    /// A box of `kind` painting `chrome`.
    ///
    /// # Panics
    ///
    /// Panics unless `chrome` passes its checks, as a widget's `chrome` does.
    #[track_caller]
    pub(super) const fn new(kind: RectKind, chrome: Chrome) -> Self {
        chrome.validate();
        Self {
            kind,
            local_rect: None,
            chrome,
            material: MaterialState::Unresolved,
        }
    }

    /// Paint into `rect` in owner-relative coords instead of the owner's arranged rect.
    ///
    /// # Panics
    ///
    /// Panics unless every component of `rect` is an [offset](crate::widget::domain::offset).
    #[track_caller]
    pub const fn at(mut self, rect: Rect) -> Self {
        rect.validate();
        self.local_rect = Some(rect);
        self
    }

    /// The material's read, as [`sealed::LowerShape::resolve`] found it.
    fn material_read(&self) -> Option<MaterialRead> {
        match self.material {
            MaterialState::Resolved(read) => read,
            MaterialState::Unresolved => unreachable!(
                "`Ui::add_shape` resolves a rect's material before it checks or lowers it"
            ),
        }
    }
}

impl sealed::LowerShape for RectShape {
    const CAN_BE_PANE: bool = true;

    #[track_caller]
    fn resolve(&mut self, effects: &EffectRegistry, reductions: Reductions) {
        self.material = MaterialState::Resolved(
            self.chrome
                .material
                .as_ref()
                .and_then(|material| MaterialRead::resolve(material, effects, reductions)),
        );
    }

    fn is_noop(&self) -> bool {
        self.local_rect.is_some_and(Rect::is_paint_empty)
            || (self.chrome.fill.is_noop()
                && self.chrome.border.is_noop()
                && self.chrome.shadow.is_noop()
                && self.material_read().is_none())
    }

    /// Geometry is already in storage form; the box lowers as a node's chrome does
    /// ([`lower::lower_box`]).
    fn lower(self, store: &mut RecordStore) -> ShapeRecord {
        let read = self.material_read();
        let Self {
            kind,
            local_rect,
            chrome,
            material: _,
        } = self;
        let LoweredBox {
            fill,
            border,
            corners,
            shadow,
            material,
        } = lower::lower_box(store, &chrome, read);
        ShapeRecord::Quad(QuadShape::Rect {
            kind,
            local_rect,
            corners,
            fill,
            border,
            shadow,
            material,
        })
    }
}
