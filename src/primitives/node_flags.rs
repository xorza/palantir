use crate::primitives::{Align, HAlign, Sense, VAlign, Visibility};

/// Packed boolean + small-enum flags from a `UiElement`. Collapses `sense`
/// (3 bools), `disabled`, `clip`, `visibility` (3 variants, 2 bits) and
/// `align` (5 variants per axis, 3 bits each) into a single `u16`. Set at
/// `Tree::push_node`; read everywhere else through accessors so callers
/// don't have to know the bit layout.
///
/// Bits: 0=click, 1=drag, 2=hover, 3=disabled, 4=clip, 5-6=visibility,
/// 7-9=HAlign, 10-12=VAlign, 13-15=reserved.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NodeFlags(u16);

impl NodeFlags {
    const CLICK: u16 = 1 << 0;
    const DRAG: u16 = 1 << 1;
    const HOVER: u16 = 1 << 2;
    const DISABLED: u16 = 1 << 3;
    const CLIP: u16 = 1 << 4;
    const VIS_MASK: u16 = 0b11 << 5;
    const VIS_VISIBLE: u16 = 0 << 5;
    const VIS_HIDDEN: u16 = 1 << 5;
    const VIS_COLLAPSED: u16 = 2 << 5;
    const HALIGN_SHIFT: u16 = 7;
    const HALIGN_MASK: u16 = 0b111 << Self::HALIGN_SHIFT;
    const VALIGN_SHIFT: u16 = 10;
    const VALIGN_MASK: u16 = 0b111 << Self::VALIGN_SHIFT;

    pub(crate) fn pack(
        sense: Sense,
        disabled: bool,
        clip: bool,
        visibility: Visibility,
        align: Align,
    ) -> Self {
        let mut bits = 0u16;
        if sense.click {
            bits |= Self::CLICK;
        }
        if sense.drag {
            bits |= Self::DRAG;
        }
        if sense.hover {
            bits |= Self::HOVER;
        }
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
        bits |= (align.halign() as u16) << Self::HALIGN_SHIFT;
        bits |= (align.valign() as u16) << Self::VALIGN_SHIFT;
        Self(bits)
    }

    pub fn sense(self) -> Sense {
        Sense {
            click: self.0 & Self::CLICK != 0,
            drag: self.0 & Self::DRAG != 0,
            hover: self.0 & Self::HOVER != 0,
        }
    }
    pub fn is_disabled(self) -> bool {
        self.0 & Self::DISABLED != 0
    }
    pub fn is_clip(self) -> bool {
        self.0 & Self::CLIP != 0
    }
    pub fn visibility(self) -> Visibility {
        match self.0 & Self::VIS_MASK {
            Self::VIS_VISIBLE => Visibility::Visible,
            Self::VIS_HIDDEN => Visibility::Hidden,
            Self::VIS_COLLAPSED => Visibility::Collapsed,
            _ => unreachable!(),
        }
    }
    pub fn is_visible(self) -> bool {
        self.0 & Self::VIS_MASK == Self::VIS_VISIBLE
    }
    pub fn is_invisible(self) -> bool {
        !self.is_visible()
    }
    pub fn is_collapsed(self) -> bool {
        self.0 & Self::VIS_MASK == Self::VIS_COLLAPSED
    }
    pub fn align(self) -> Align {
        let h = (self.0 & Self::HALIGN_MASK) >> Self::HALIGN_SHIFT;
        let v = (self.0 & Self::VALIGN_MASK) >> Self::VALIGN_SHIFT;
        let h = match h {
            0 => HAlign::Auto,
            1 => HAlign::Left,
            2 => HAlign::Center,
            3 => HAlign::Right,
            4 => HAlign::Stretch,
            _ => unreachable!(),
        };
        let v = match v {
            0 => VAlign::Auto,
            1 => VAlign::Top,
            2 => VAlign::Center,
            3 => VAlign::Bottom,
            4 => VAlign::Stretch,
            _ => unreachable!(),
        };
        Align::new(h, v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(!f.is_invisible());
        assert!(!f.is_collapsed());
        assert_eq!(f.align(), Align::default());
    }

    #[test]
    fn each_sense_bit_round_trips_independently() {
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
            assert_eq!(f.is_invisible(), vis != Visibility::Visible);
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
