use crate::primitives::paint::color::RgbaF32;
use crate::widgets::color_picker::history::History;

/// The row starts full so it never changes length, though a default history holds nothing until first read.
#[test]
fn presets_fill_the_row() {
    let mut history = History::default();
    assert!(history.colors.is_empty(), "no presets built before use");
    assert_eq!(history.colors().len(), History::CAP);
}

/// The presets are the derived list: hue 11/12 last, then the neutrals ending at black and white.
#[test]
fn presets_are_the_derived_list() {
    use crate::primitives::paint::color::okhsv::Okhsv;
    let mut history = History::default();
    let colors = history.colors();
    assert_eq!(colors[11], Okhsv::new(11.0 / 12.0, 1.0, 1.0).to_color());
    assert_eq!(colors[12], RgbaF32::BLACK);
    assert_eq!(colors[15].to_srgba_u8(), RgbaF32::WHITE.to_srgba_u8());
}

#[test]
fn a_pick_moves_to_the_front_and_the_row_keeps_its_length() {
    let mut history = History::default();
    let last = history.colors()[History::CAP - 1];
    let picked = RgbaF32::hex(0x4cd3ff);
    history.push(picked);
    assert_eq!(history.colors()[0], picked);
    assert_eq!(history.colors().len(), History::CAP);
    assert!(!history.colors().contains(&last), "the oldest fell off");
}

#[test]
fn a_repeat_pick_moves_instead_of_growing() {
    let mut history = History::default();
    let held = history.colors()[5];
    history.push(RgbaF32::hex(0x123456));
    history.push(held);
    assert_eq!(history.colors()[0], held);
    assert_eq!(history.colors().len(), History::CAP);
    assert_eq!(
        history.colors().iter().filter(|c| **c == held).count(),
        1,
        "one copy only",
    );
}
