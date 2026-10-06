//! The per-node bitset column: sense, disabled, clip mode, key scope.

use crate::input::key_class::KeyFilter;
use crate::input::sense::Sense;
use crate::primitives::layout::axis::Axis;
use crate::primitives::layout::clip_mode::ClipMode;

#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, bytemuck::NoUninit)]
pub(crate) struct NodeFlags {
    bits: u32,
}

impl NodeFlags {
    const SENSE_MASK: u32 = 0b11_1111;
    const DISABLED: u32 = 1 << 6;
    const CLIP_SHIFT: u32 = 7;
    const CLIP_MASK: u32 = 0b11 << Self::CLIP_SHIFT;
    const FOCUSABLE: u32 = 1 << 9;
    const SCOPE_SHIFT: u32 = 10;
    const SCOPE_MASK: u32 = 0xff << Self::SCOPE_SHIFT;
    /// Set when the node is *not* a Tab stop, so the zero default is one.
    const NOT_TAB_STOP: u32 = 1 << 18;
    /// The axis whose arrows move focus between the stops inside the
    /// node: `0` none, `1 + Axis`.
    const ARROW_SHIFT: u32 = 19;
    const ARROW_MASK: u32 = 0b11 << Self::ARROW_SHIFT;
    /// How many low bits of [`Self::bits`] can be set — what a caller
    /// packing the word beside others reserves for it.
    pub(crate) const WIDTH: u32 = (Self::SENSE_MASK
        | Self::DISABLED
        | Self::CLIP_MASK
        | Self::FOCUSABLE
        | Self::SCOPE_MASK
        | Self::NOT_TAB_STOP
        | Self::ARROW_MASK)
        .bit_width();

    /// The whole bitset, for callers that fold it into a hash rather
    /// than reading one field — [`LayoutCore::hash_with_flags`] mixes
    /// these bits in with the packed layout metadata.
    ///
    /// [`LayoutCore::hash_with_flags`]:
    ///     crate::scene::node::layout_core::LayoutCore::hash_with_flags
    #[inline]
    pub(crate) const fn bits(self) -> u32 {
        self.bits
    }

    #[inline]
    pub(crate) const fn sense(self) -> Sense {
        Sense::from_bits_truncate((self.bits & Self::SENSE_MASK) as u8)
    }

    #[inline]
    pub(crate) const fn is_disabled(self) -> bool {
        self.bits & Self::DISABLED != 0
    }

    #[inline]
    pub(crate) const fn clip_mode(self) -> ClipMode {
        match (self.bits & Self::CLIP_MASK) >> Self::CLIP_SHIFT {
            0 => ClipMode::None,
            1 => ClipMode::Rect,
            2 => ClipMode::Rounded,
            _ => unreachable!(),
        }
    }

    #[inline]
    pub(crate) const fn is_focusable(self) -> bool {
        self.bits & Self::FOCUSABLE != 0
    }

    #[inline]
    pub(crate) const fn is_tab_stop(self) -> bool {
        self.bits & Self::NOT_TAB_STOP == 0
    }

    /// The key classes this node's input scope takes, or
    /// [`KeyFilter::NONE`] when it declares no scope — the empty filter
    /// doubles as "not a scope", which is what lets this ride spare bits
    /// instead of costing a presence flag of its own.
    #[inline]
    pub(crate) const fn key_filter(self) -> KeyFilter {
        KeyFilter::from_bits_truncate(((self.bits & Self::SCOPE_MASK) >> Self::SCOPE_SHIFT) as u8)
    }

    #[inline]
    pub(crate) const fn set_sense(&mut self, s: Sense) {
        self.bits = (self.bits & !Self::SENSE_MASK) | ((s.bits() as u32) & Self::SENSE_MASK);
    }

    #[inline]
    pub(crate) const fn set_disabled(&mut self, v: bool) {
        self.bits = (self.bits & !Self::DISABLED) | (if v { Self::DISABLED } else { 0 });
    }

    #[inline]
    pub(crate) const fn set_clip(&mut self, c: ClipMode) {
        self.bits = (self.bits & !Self::CLIP_MASK) | ((c as u32) << Self::CLIP_SHIFT);
    }

    #[inline]
    pub(crate) const fn set_focusable(&mut self, v: bool) {
        self.bits = (self.bits & !Self::FOCUSABLE) | (if v { Self::FOCUSABLE } else { 0 });
    }

    #[inline]
    pub(crate) const fn arrow_focus(self) -> Option<Axis> {
        match (self.bits & Self::ARROW_MASK) >> Self::ARROW_SHIFT {
            0 => None,
            1 => Some(Axis::X),
            2 => Some(Axis::Y),
            _ => unreachable!(),
        }
    }

    #[inline]
    pub(crate) const fn set_arrow_focus(&mut self, axis: Option<Axis>) {
        let lane = match axis {
            None => 0,
            Some(axis) => 1 + axis as u32,
        };
        self.bits = (self.bits & !Self::ARROW_MASK) | (lane << Self::ARROW_SHIFT);
    }

    #[inline]
    pub(crate) const fn set_tab_stop(&mut self, v: bool) {
        self.bits = (self.bits & !Self::NOT_TAB_STOP) | (if v { 0 } else { Self::NOT_TAB_STOP });
    }

    #[inline]
    pub(crate) const fn set_key_filter(&mut self, f: KeyFilter) {
        self.bits = (self.bits & !Self::SCOPE_MASK)
            | (((f.bits() as u32) << Self::SCOPE_SHIFT) & Self::SCOPE_MASK);
    }
}

const _: () = assert!(
    (ClipMode::Rounded as u32) <= (NodeFlags::CLIP_MASK >> NodeFlags::CLIP_SHIFT),
    "ClipMode discriminant exceeds 2 bits",
);
const _: () = assert!(
    Sense::ALL.bits() as u32 <= NodeFlags::SENSE_MASK,
    "Sense uses more than 6 bits",
);
const _: () = assert!(
    ((KeyFilter::ALL.bits() as u32) << NodeFlags::SCOPE_SHIFT) <= NodeFlags::SCOPE_MASK,
    "KeyFilter uses more than 8 bits",
);
