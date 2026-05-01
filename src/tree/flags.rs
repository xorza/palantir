use crate::primitives::{Align, Sense, Visibility};

/// Packed flags for a tree `Node`: `sense` (5-state enum, 3 bits), `disabled`,
/// `clip`, `visibility` (3 variants, 2 bits) packed into one byte, with
/// `align` stored in its native byte alongside. Built at `Tree::push_node`
/// from a `UiElement`; read everywhere else through accessors so callers
/// don't have to know the bit layout.
///
/// `bits`: 0-2=sense tag, 3=disabled, 4=clip, 5-6=visibility, 7=reserved.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NodeFlags {
    bits: u8,
    align: Align,
}

impl NodeFlags {
    const SENSE_MASK: u8 = 0b111;
    const DISABLED: u8 = 1 << 3;
    const CLIP: u8 = 1 << 4;
    const VIS_MASK: u8 = 0b11 << 5;
    const VIS_VISIBLE: u8 = 0 << 5;
    const VIS_HIDDEN: u8 = 1 << 5;
    const VIS_COLLAPSED: u8 = 2 << 5;

    pub(crate) fn pack(
        sense: Sense,
        disabled: bool,
        clip: bool,
        visibility: Visibility,
        align: Align,
    ) -> Self {
        let mut bits = sense as u8;
        if disabled {
            bits |= Self::DISABLED;
        }
        if clip {
            bits |= Self::CLIP;
        }
        bits |= match visibility {
            Visibility::Visible => Self::VIS_VISIBLE,
            Visibility::Hidden => Self::VIS_HIDDEN,
            Visibility::Collapsed => Self::VIS_COLLAPSED,
        };
        Self { bits, align }
    }

    pub fn sense(self) -> Sense {
        match self.bits & Self::SENSE_MASK {
            0 => Sense::None,
            1 => Sense::Hover,
            2 => Sense::Click,
            3 => Sense::Drag,
            4 => Sense::ClickAndDrag,
            _ => unreachable!(),
        }
    }
    pub fn is_disabled(self) -> bool {
        self.bits & Self::DISABLED != 0
    }
    pub fn is_clip(self) -> bool {
        self.bits & Self::CLIP != 0
    }
    pub fn visibility(self) -> Visibility {
        match self.bits & Self::VIS_MASK {
            Self::VIS_VISIBLE => Visibility::Visible,
            Self::VIS_HIDDEN => Visibility::Hidden,
            Self::VIS_COLLAPSED => Visibility::Collapsed,
            _ => unreachable!(),
        }
    }
    pub fn is_visible(self) -> bool {
        self.bits & Self::VIS_MASK == Self::VIS_VISIBLE
    }
    pub fn is_collapsed(self) -> bool {
        self.bits & Self::VIS_MASK == Self::VIS_COLLAPSED
    }
    pub fn align(self) -> Align {
        self.align
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::{HAlign, VAlign};

    fn pack_default() -> NodeFlags {
        NodeFlags::pack(
            Sense::NONE,
            false,
            false,
            Visibility::Visible,
            Align::default(),
        )
    }

    #[test]
    fn default_round_trips_to_inert_state() {
        let f = pack_default();
        assert_eq!(f.sense(), Sense::NONE);
        assert!(!f.is_disabled());
        assert!(!f.is_clip());
        assert_eq!(f.visibility(), Visibility::Visible);
        assert!(f.is_visible());
        assert!(!f.is_collapsed());
        assert_eq!(f.align(), Align::default());
    }

    #[test]
    fn every_sense_variant_round_trips() {
        for sense in [
            Sense::NONE,
            Sense::HOVER,
            Sense::CLICK,
            Sense::DRAG,
            Sense::CLICK_AND_DRAG,
        ] {
            let f = NodeFlags::pack(sense, false, false, Visibility::Visible, Align::default());
            assert_eq!(f.sense(), sense, "sense {sense:?}");
        }
    }

    #[test]
    fn disabled_and_clip_bits_round_trip() {
        for &(disabled, clip) in &[(false, false), (true, false), (false, true), (true, true)] {
            let f = NodeFlags::pack(
                Sense::NONE,
                disabled,
                clip,
                Visibility::Visible,
                Align::default(),
            );
            assert_eq!(f.is_disabled(), disabled);
            assert_eq!(f.is_clip(), clip);
        }
    }

    #[test]
    fn each_visibility_round_trips() {
        for vis in [
            Visibility::Visible,
            Visibility::Hidden,
            Visibility::Collapsed,
        ] {
            let f = NodeFlags::pack(Sense::NONE, false, false, vis, Align::default());
            assert_eq!(f.visibility(), vis);
            assert_eq!(f.is_visible(), vis == Visibility::Visible);
            assert_eq!(f.is_collapsed(), vis == Visibility::Collapsed);
        }
    }

    #[test]
    fn every_halign_variant_round_trips() {
        for h in [
            HAlign::Auto,
            HAlign::Left,
            HAlign::Center,
            HAlign::Right,
            HAlign::Stretch,
        ] {
            let a = Align::new(h, VAlign::Auto);
            let f = NodeFlags::pack(Sense::NONE, false, false, Visibility::Visible, a);
            assert_eq!(f.align(), a, "halign {h:?}");
        }
    }

    #[test]
    fn every_valign_variant_round_trips() {
        for v in [
            VAlign::Auto,
            VAlign::Top,
            VAlign::Center,
            VAlign::Bottom,
            VAlign::Stretch,
        ] {
            let a = Align::new(HAlign::Auto, v);
            let f = NodeFlags::pack(Sense::NONE, false, false, Visibility::Visible, a);
            assert_eq!(f.align(), a, "valign {v:?}");
        }
    }

    #[test]
    fn fields_do_not_alias_each_other() {
        let f = NodeFlags::pack(
            Sense::CLICK_AND_DRAG,
            true,
            true,
            Visibility::Collapsed,
            Align::new(HAlign::Right, VAlign::Bottom),
        );
        assert_eq!(f.sense(), Sense::CLICK_AND_DRAG);
        assert!(f.is_disabled());
        assert!(f.is_clip());
        assert_eq!(f.visibility(), Visibility::Collapsed);
        assert_eq!(f.align(), Align::new(HAlign::Right, VAlign::Bottom));
    }

    #[test]
    fn fits_in_two_bytes() {
        assert_eq!(std::mem::size_of::<NodeFlags>(), 2);
    }
}
