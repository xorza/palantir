//! What `ScrollWrappers::split` routes onto which wrapper.

use crate::input::key_class::KeyFilter;
use crate::input::sense::Sense;
use crate::layout::types::justify::Justify;
use crate::layout::types::sizing::{Sizes, Sizing};
use crate::primitives::size::Size;
use crate::primitives::spacing::Spacing;
use crate::widgets::configure::Configure;
use crate::widgets::scroll::{Scroll, ScrollWrappers};

/// The outer wrapper is what the caller's interaction flags land on, the
/// key filter included: a scope declared on the scroll has to reach the
/// tree, or `Scroll::vertical().input_scope(..)` records no scope at all.
#[test]
fn split_carries_every_interaction_flag_onto_the_outer_wrapper() {
    let scroll = Scroll::vertical()
        .sense(Sense::CLICK)
        .disabled(true)
        .focusable(true)
        .input_scope(KeyFilter::ALL);
    let ScrollWrappers { outer, inner } = ScrollWrappers::split(&scroll.widget);
    assert_eq!(outer.authored_sense(), Sense::CLICK);
    assert!(outer.authored_disabled());
    assert!(outer.authored_focusable());
    assert_eq!(outer.authored_input_scope(), KeyFilter::ALL);
    assert_eq!(inner.authored_input_scope(), KeyFilter::empty());
}

/// Sizing is the outer wrapper's, and the box the caller sees; padding
/// and the panel knobs are the inner viewport's, where the children are.
#[test]
fn split_routes_sizing_outward_and_panel_knobs_inward() {
    let size: Sizes = (Sizing::fixed(120.0), Sizing::HUG).into();
    let scroll = Scroll::vertical()
        .size(size)
        .min_size(Size::new(10.0, 20.0))
        .max_size(Size::new(300.0, 400.0))
        .padding(Spacing::all(4.0))
        .gap(3.0)
        .line_gap(5.0)
        .justify(Justify::SpaceBetween);
    let ScrollWrappers { outer, inner } = ScrollWrappers::split(&scroll.widget);

    assert_eq!(outer.authored_size(), Some(size));
    assert_eq!(outer.authored_min_size(), Some(Size::new(10.0, 20.0)));
    assert_eq!(outer.authored_max_size(), Some(Size::new(300.0, 400.0)));
    assert_eq!(outer.authored_padding(), None);
    assert_eq!(outer.authored_gap(), None);

    assert_eq!(
        inner.authored_size(),
        Some((Sizing::FILL, Sizing::FILL).into())
    );
    assert_eq!(inner.authored_padding(), Some(Spacing::all(4.0)));
    assert_eq!(inner.authored_gap(), Some(3.0));
    assert_eq!(inner.authored_line_gap(), Some(5.0));
    assert_eq!(inner.authored_justify(), Justify::SpaceBetween);
}
