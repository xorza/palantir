use crate::input::key_class::KeyFilter;
use crate::input::sense::Sense;
use crate::layout::types::align::{Align, HAlign, VAlign};
use crate::layout::types::clip_mode::ClipMode;
use crate::layout::types::grid_cell::GridCell;
use crate::layout::types::justify::Justify;
use crate::layout::types::layout_mode::{LayoutMode, PackedLayoutMeta};
use crate::layout::types::limits::valid_packed_gap;
use crate::layout::types::sizing::Sizes;
use crate::primitives::approx::{self, FloatHash};
use crate::primitives::size::Size;
use crate::primitives::spacing::Spacing;
use crate::primitives::transform::TranslateScale;
use crate::primitives::widget_id::WidgetId;
use crate::scene::node::Node;
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
        state.write_u32(self.resolved());
    }
}

impl Gaps {
    pub(crate) const ZERO: Self = Self([0; 2]);

    /// The "caller never set this" bit pattern: an f16 quiet NaN.
    ///
    /// A gap is finite and non-negative ([`valid_packed_gap`]), so no
    /// value a caller can store lands here — which makes NaN free to
    /// carry the unset flag without widening the packed pair. Both
    /// readers below fold it back to `0.0` via `f32::max`, which returns
    /// the non-NaN operand and so costs a `maxss`, not a branch.
    const UNSET: u16 = 0x7E00;

    /// A pair with neither axis set. [`Node`]'s starting value, so a
    /// widget can tell an untouched gap from a caller's explicit `0.0`
    /// and fill in a themed default only for the former.
    pub(crate) const UNSET_PAIR: Self = Self([Self::UNSET; 2]);

    #[inline]
    pub(crate) fn gap(self) -> f32 {
        f16::from_bits(self.0[0]).to_f32().max(0.0)
    }

    #[inline]
    pub(crate) fn line_gap(self) -> f32 {
        f16::from_bits(self.0[1]).to_f32().max(0.0)
    }

    /// Both lanes as one `u32`, with unset axes folded to `0.0` — what
    /// layout actually sees. Equality and hashing downstream go through
    /// this, so an untouched gap and an explicit `0.0` can't split a
    /// cache key or an extras row when they render identically.
    ///
    /// Shifted rather than byte-cast so the key is the same number on
    /// either endianness; it never leaves the process, but a
    /// layout-dependent hash is a trap worth not setting.
    #[inline]
    pub(crate) fn resolved(self) -> u32 {
        let gap = if self.gap_is_set() { self.0[0] } else { 0 };
        let line_gap = if self.line_gap_is_set() { self.0[1] } else { 0 };
        gap as u32 | ((line_gap as u32) << 16)
    }

    #[inline]
    pub(crate) fn gap_is_set(self) -> bool {
        self.0[0] != Self::UNSET
    }

    #[inline]
    pub(crate) fn line_gap_is_set(self) -> bool {
        self.0[1] != Self::UNSET
    }

    #[inline]
    pub(crate) fn set_gap(&mut self, v: f32) {
        debug_assert!(
            valid_packed_gap(v),
            "gap must be finite, non-negative, and no greater than the f16 maximum, got {v}",
        );
        self.0[0] = f16::from_f32(v).to_bits();
    }

    #[inline]
    pub(crate) fn set_line_gap(&mut self, v: f32) {
        debug_assert!(
            valid_packed_gap(v),
            "line gap must be finite, non-negative, and no greater than the f16 maximum, got {v}",
        );
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
        self.position.hash_visual(h);
        self.grid.hash(h);
        self.min_size.hash_visual(h);
        self.max_size.hash_visual(h);
    }
}

impl Hash for PanelExtras {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        let gaps_u32 = self.gaps.resolved();
        let packed = (gaps_u32 as u64)
            | ((self.child_align.raw() as u64) << 32)
            | ((self.justify as u64) << 40);
        h.write_u64(packed);
        if !self.transform.is_noop() {
            h.write_u8(1);
            self.transform.translation.hash_visual(h);
            (self.transform.scale - 1.0).hash_visual(h);
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
        self.gaps.resolved() == Self::DEFAULT.gaps.resolved()
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
    pub(crate) meta: PackedLayoutMeta,
}

impl LayoutCore {
    pub(super) fn from_node(node: &Node) -> Self {
        let mode = node.mode.resolved();
        Self {
            size: node.size.unwrap_or_default(),
            padding: node.padding.unwrap_or(Spacing::ZERO),
            margin: node.margin.unwrap_or(Spacing::ZERO),
            meta: PackedLayoutMeta::new(mode, node.align, node.visibility),
        }
    }

    #[inline]
    pub(crate) fn hash_with_flags<H: std::hash::Hasher>(&self, flags: NodeFlags, h: &mut H) {
        h.write_u64(self.size.as_u64());
        h.write_u64(self.padding.as_u64());
        h.write_u64(self.margin.as_u64());
        let mode = self.meta.into();
        let [flags_lo, flags_hi] = flags.bits.to_ne_bytes();
        let tail = u32::from_ne_bytes([self.meta.metadata(), self.meta.tag(), flags_lo, flags_hi]);
        h.write_u32(tail);
        if let LayoutMode::Scroll(spec) = mode {
            spec.hash(h);
        }
    }
}

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
    const SCOPE_SHIFT: u16 = 9;
    const SCOPE_MASK: u16 = 0b1_1111 << Self::SCOPE_SHIFT;

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

    /// The key classes this node's input scope takes, or
    /// [`KeyFilter::empty`] when it declares no scope — the empty filter
    /// doubles as "not a scope", which is what lets this ride spare bits
    /// instead of costing a presence flag of its own.
    #[inline]
    pub(crate) fn key_filter(self) -> KeyFilter {
        KeyFilter::from_bits_truncate(((self.bits & Self::SCOPE_MASK) >> Self::SCOPE_SHIFT) as u8)
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

    #[inline]
    pub(crate) fn set_key_filter(&mut self, f: KeyFilter) {
        self.bits = (self.bits & !Self::SCOPE_MASK)
            | (((f.bits() as u16) << Self::SCOPE_SHIFT) & Self::SCOPE_MASK);
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
const _: () = assert!(
    ((KeyFilter::all().bits() as u16) << NodeFlags::SCOPE_SHIFT) <= NodeFlags::SCOPE_MASK,
    "KeyFilter uses more than 5 bits",
);

#[derive(Debug)]
pub(crate) struct NodeColumns {
    pub(crate) widget_id: WidgetId,
    pub(crate) layout: LayoutCore,
    pub(crate) attrs: NodeFlags,
    pub(crate) bounds: BoundsExtras,
    pub(crate) panel: PanelExtras,
}
