//! Explicit builder configuration tracking for [`Element`].

use crate::input::sense::Sense;
use crate::layout::types::align::Align;
use crate::layout::types::clip_mode::ClipMode;
use crate::layout::types::grid_cell::GridCell;
use crate::layout::types::justify::Justify;
use crate::layout::types::sizing::Sizes;
use crate::primitives::size::Size;
use crate::primitives::spacing::Spacing;
use crate::primitives::transform::TranslateScale;
use crate::scene::element::{Element, Salt};
use crate::scene::visibility::Visibility;
use bitflags::bitflags;
use glam::Vec2;

bitflags! {
    #[derive(Clone, Copy, Debug, Default)]
    pub(crate) struct ConfiguredFields: u32 {
        const SALT = 1 << 0;
        const SIZE = 1 << 1;
        const MIN_SIZE = 1 << 2;
        const MAX_SIZE = 1 << 3;
        const PADDING = 1 << 4;
        const MARGIN = 1 << 5;
        const POSITION = 1 << 6;
        const GRID = 1 << 7;
        const GAP = 1 << 8;
        const LINE_GAP = 1 << 9;
        const JUSTIFY = 1 << 10;
        const ALIGN = 1 << 11;
        const CHILD_ALIGN = 1 << 12;
        const SENSE = 1 << 13;
        const DISABLED = 1 << 14;
        const FOCUSABLE = 1 << 15;
        const VISIBILITY = 1 << 16;
        const CLIP = 1 << 17;
        const TRANSFORM = 1 << 18;
    }
}

/// Read-only view of values explicitly supplied through builder configuration.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ConfiguredElement<'a> {
    element: &'a Element,
}

// This deliberately mirrors the complete builder surface even though only
// fields with defaults currently have production readers.
#[allow(dead_code)]
impl<'a> ConfiguredElement<'a> {
    pub(crate) fn new(element: &'a Element) -> Self {
        Self { element }
    }

    fn get<T: Copy>(self, field: ConfiguredFields, value: T) -> Option<T> {
        self.element.configured.contains(field).then_some(value)
    }

    pub(crate) fn salt(self) -> Option<Salt> {
        self.get(ConfiguredFields::SALT, self.element.salt)
    }

    pub(crate) fn size(self) -> Option<Sizes> {
        self.get(ConfiguredFields::SIZE, self.element.size)
    }

    pub(crate) fn min_size(self) -> Option<Size> {
        self.get(ConfiguredFields::MIN_SIZE, self.element.min_size)
    }

    pub(crate) fn max_size(self) -> Option<Size> {
        self.get(ConfiguredFields::MAX_SIZE, self.element.max_size)
    }

    pub(crate) fn padding(self) -> Option<Spacing> {
        self.get(ConfiguredFields::PADDING, self.element.padding)
    }

    pub(crate) fn margin(self) -> Option<Spacing> {
        self.get(ConfiguredFields::MARGIN, self.element.margin)
    }

    pub(crate) fn position(self) -> Option<Vec2> {
        self.get(ConfiguredFields::POSITION, self.element.position)
    }

    pub(crate) fn grid(self) -> Option<GridCell> {
        self.get(ConfiguredFields::GRID, self.element.grid)
    }

    pub(crate) fn gap(self) -> Option<f32> {
        self.get(ConfiguredFields::GAP, self.element.gaps.gap())
    }

    pub(crate) fn line_gap(self) -> Option<f32> {
        self.get(ConfiguredFields::LINE_GAP, self.element.gaps.line_gap())
    }

    pub(crate) fn justify(self) -> Option<Justify> {
        self.get(ConfiguredFields::JUSTIFY, self.element.justify)
    }

    pub(crate) fn align(self) -> Option<Align> {
        self.get(ConfiguredFields::ALIGN, self.element.align)
    }

    pub(crate) fn child_align(self) -> Option<Align> {
        self.get(ConfiguredFields::CHILD_ALIGN, self.element.child_align)
    }

    pub(crate) fn sense(self) -> Option<Sense> {
        self.get(ConfiguredFields::SENSE, self.element.flags.sense())
    }

    pub(crate) fn disabled(self) -> Option<bool> {
        self.get(ConfiguredFields::DISABLED, self.element.flags.is_disabled())
    }

    pub(crate) fn focusable(self) -> Option<bool> {
        self.get(
            ConfiguredFields::FOCUSABLE,
            self.element.flags.is_focusable(),
        )
    }

    pub(crate) fn visibility(self) -> Option<Visibility> {
        self.get(ConfiguredFields::VISIBILITY, self.element.visibility)
    }

    pub(crate) fn clip(self) -> Option<ClipMode> {
        self.get(ConfiguredFields::CLIP, self.element.flags.clip_mode())
    }

    pub(crate) fn transform(self) -> Option<TranslateScale> {
        self.get(ConfiguredFields::TRANSFORM, self.element.transform)
    }
}
