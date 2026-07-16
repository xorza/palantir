use crate::forest::element::Element;
use crate::forest::visibility::Visibility;
use crate::input::sense::Sense;
use crate::layout::types::align::{Align, HAlign, VAlign};
use crate::layout::types::clip_mode::ClipMode;
use crate::layout::types::grid_cell::GridCell;
use crate::layout::types::justify::Justify;
use crate::layout::types::layout_mode::{GridDefId, LayoutMode, ModePayload, ScrollSpec};
use crate::layout::types::sizing::Sizes;
use crate::primitives::approx;
use crate::primitives::size::Size;
use crate::primitives::spacing::Spacing;
use crate::primitives::transform::TranslateScale;
use crate::primitives::widget_id::WidgetId;
use glam::Vec2;
use half::f16;
use std::hash::Hash;

#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct Gaps([u16; 2]);

impl std::fmt::Debug for Gaps {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Gaps")
            .field("gap", &self.gap())
            .field("line_gap", &self.line_gap())
            .finish()
    }
}

impl Hash for Gaps {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write_u32(u32::from_ne_bytes(bytemuck::cast(self.0)));
    }
}

impl Gaps {
    pub(crate) const ZERO: Self = Self([0; 2]);

    #[inline]
    pub(crate) fn gap(self) -> f32 {
        f16::from_bits(self.0[0]).to_f32()
    }

    #[inline]
    pub(crate) fn line_gap(self) -> f32 {
        f16::from_bits(self.0[1]).to_f32()
    }

    #[inline]
    pub(crate) fn set_gap(&mut self, v: f32) {
        self.0[0] = f16::from_f32(v).to_bits();
    }

    #[inline]
    pub(crate) fn set_line_gap(&mut self, v: f32) {
        self.0[1] = f16::from_f32(v).to_bits();
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct BoundsExtras {
    pub(crate) position: Vec2,
    pub(crate) grid: GridCell,
    pub(crate) min_size: Size,
    pub(crate) max_size: Size,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PanelExtras {
    pub(crate) gaps: Gaps,
    pub(crate) justify: Justify,
    pub(crate) child_align: Align,
    pub(crate) transform: TranslateScale,
}

impl Hash for BoundsExtras {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        approx::hash_visual_vec2(self.position, h);
        self.grid.hash(h);
        approx::hash_visual_size(self.min_size, h);
        approx::hash_visual_size(self.max_size, h);
    }
}

impl Hash for PanelExtras {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        let gaps_u32 = u32::from_ne_bytes(bytemuck::cast(self.gaps.0));
        let packed = (gaps_u32 as u64)
            | ((self.child_align.raw() as u64) << 32)
            | ((self.justify as u64) << 40);
        h.write_u64(packed);
        if !self.transform.is_noop() {
            h.write_u8(1);
            approx::hash_visual_vec2(self.transform.translation, h);
            approx::hash_visual_f32(self.transform.scale - 1.0, h);
        } else {
            h.write_u8(0);
        }
    }
}

impl BoundsExtras {
    pub(crate) const DEFAULT: Self = Self {
        position: Vec2::ZERO,
        grid: GridCell {
            row: 0,
            col: 0,
            row_span: 1,
            col_span: 1,
        },
        min_size: Size::ZERO,
        max_size: Size::INF,
    };

    #[inline]
    pub(crate) fn is_default(&self) -> bool {
        approx::approx_zero(self.position.x)
            && approx::approx_zero(self.position.y)
            && self.grid == Self::DEFAULT.grid
            && self.min_size.approx_zero()
            && self.max_size == Self::DEFAULT.max_size
    }
}

impl PanelExtras {
    pub(crate) const DEFAULT: Self = Self {
        gaps: Gaps::ZERO,
        justify: Justify::Start,
        child_align: Align::new(HAlign::Auto, VAlign::Auto),
        transform: TranslateScale::IDENTITY,
    };

    #[inline]
    pub(crate) fn is_default(&self) -> bool {
        self.gaps == Self::DEFAULT.gaps
            && self.justify == Self::DEFAULT.justify
            && self.child_align == Self::DEFAULT.child_align
            && self.transform.is_noop()
    }
}

impl Default for BoundsExtras {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl Default for PanelExtras {
    fn default() -> Self {
        Self::DEFAULT
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct LayoutCore {
    pub(crate) size: Sizes,
    pub(crate) padding: Spacing,
    pub(crate) margin: Spacing,
    pub(crate) mode_payload: ModePayload,
    bits: u8,
    pub(crate) mode: LayoutMode,
}

impl LayoutCore {
    const ALIGN_MASK: u8 = 0b11_1111;
    const VIS_SHIFT: u8 = 6;
    const VIS_MASK: u8 = 0b11 << Self::VIS_SHIFT;

    pub(crate) fn from_element(element: &Element) -> Self {
        Self {
            size: element.size,
            padding: element.padding,
            margin: element.margin,
            mode_payload: element.mode_payload,
            bits: Self::pack_bits(element.align, element.visibility),
            mode: element.mode,
        }
    }

    #[inline]
    const fn pack_bits(align: Align, vis: Visibility) -> u8 {
        (align.raw() & Self::ALIGN_MASK) | (((vis as u8) << Self::VIS_SHIFT) & Self::VIS_MASK)
    }

    #[inline(always)]
    pub(crate) fn align(&self) -> Align {
        Align::from_raw(self.bits & Self::ALIGN_MASK)
    }

    #[inline(always)]
    pub(crate) fn visibility(&self) -> Visibility {
        let raw = (self.bits & Self::VIS_MASK) >> Self::VIS_SHIFT;
        unsafe { std::mem::transmute::<u8, Visibility>(raw) }
    }

    pub(crate) fn grid_def_id(self) -> GridDefId {
        self.mode_payload.grid_def_id(self.mode)
    }

    pub(crate) fn scroll_spec(self) -> ScrollSpec {
        self.mode_payload.scroll_spec(self.mode)
    }

    #[inline]
    pub(crate) fn hash_with_flags<H: std::hash::Hasher>(&self, flags: NodeFlags, h: &mut H) {
        h.write_u64(self.size.as_u64());
        h.write_u64(self.padding.as_u64());
        h.write_u64(self.margin.as_u64());
        let [flags_lo, flags_hi] = flags.bits.to_ne_bytes();
        let tail = u32::from_ne_bytes([self.bits, self.mode as u8, flags_lo, flags_hi]);
        h.write_u32(tail);
        if matches!(self.mode, LayoutMode::Scroll) {
            self.scroll_spec().hash(h);
        }
    }
}

const _: () = assert!(
    (Visibility::Collapsed as u8) <= (LayoutCore::VIS_MASK >> LayoutCore::VIS_SHIFT),
    "Visibility discriminant exceeds 2 bits",
);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct NodeFlags {
    bits: u16,
}

impl NodeFlags {
    const SENSE_MASK: u16 = 0b1_1111;
    const DISABLED: u16 = 1 << 5;
    const CLIP_SHIFT: u16 = 6;
    const CLIP_MASK: u16 = 0b11 << Self::CLIP_SHIFT;
    const FOCUSABLE: u16 = 1 << 8;

    #[inline]
    pub(crate) fn sense(self) -> Sense {
        Sense::from_bits_truncate((self.bits & Self::SENSE_MASK) as u8)
    }

    #[inline]
    pub(crate) fn is_disabled(self) -> bool {
        self.bits & Self::DISABLED != 0
    }

    #[inline]
    pub(crate) fn clip_mode(self) -> ClipMode {
        match (self.bits & Self::CLIP_MASK) >> Self::CLIP_SHIFT {
            0 => ClipMode::None,
            1 => ClipMode::Rect,
            2 => ClipMode::Rounded,
            _ => unreachable!(),
        }
    }

    #[inline]
    pub(crate) fn is_focusable(self) -> bool {
        self.bits & Self::FOCUSABLE != 0
    }

    #[inline]
    pub(crate) fn set_sense(&mut self, s: Sense) {
        self.bits = (self.bits & !Self::SENSE_MASK) | ((s.bits() as u16) & Self::SENSE_MASK);
    }

    #[inline]
    pub(crate) fn set_disabled(&mut self, v: bool) {
        self.bits = (self.bits & !Self::DISABLED) | (if v { Self::DISABLED } else { 0 });
    }

    #[inline]
    pub(crate) fn set_clip(&mut self, c: ClipMode) {
        self.bits = (self.bits & !Self::CLIP_MASK) | ((c as u16) << Self::CLIP_SHIFT);
    }

    #[inline]
    pub(crate) fn set_focusable(&mut self, v: bool) {
        self.bits = (self.bits & !Self::FOCUSABLE) | (if v { Self::FOCUSABLE } else { 0 });
    }
}

const _: () = assert!(
    (ClipMode::Rounded as u16) <= (NodeFlags::CLIP_MASK >> NodeFlags::CLIP_SHIFT),
    "ClipMode discriminant exceeds 2 bits",
);
const _: () = assert!(
    Sense::all().bits() as u16 <= NodeFlags::SENSE_MASK,
    "Sense uses more than 5 bits",
);

#[derive(Debug)]
pub(crate) struct ElementColumns {
    pub(crate) widget_id: WidgetId,
    pub(crate) layout: LayoutCore,
    pub(crate) attrs: NodeFlags,
    pub(crate) bounds: BoundsExtras,
    pub(crate) panel: PanelExtras,
}
