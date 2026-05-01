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
