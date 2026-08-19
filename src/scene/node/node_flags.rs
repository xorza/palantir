//! The per-node bitset column: sense, disabled, clip mode, key scope.

use crate::input::key_class::KeyFilter;
use crate::input::sense::Sense;
use crate::layout::types::clip_mode::ClipMode;

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

    /// The whole bitset, for callers that fold it into a hash rather
    /// than reading one field — [`LayoutCore::hash_with_flags`] mixes
    /// these two bytes in with the packed layout metadata.
    ///
    /// [`LayoutCore::hash_with_flags`]:
    ///     crate::scene::node::layout_core::LayoutCore::hash_with_flags
    #[inline]
    pub(super) fn bits(self) -> u16 {
        self.bits
    }

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
