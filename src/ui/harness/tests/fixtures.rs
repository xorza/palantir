//! The non-frame affordances: the arena, the clipboard, a shared text
//! cache, and the collision report.

use crate::scene::node::configure::Configure;
use crate::ui::harness::tests::support::{SURFACE, button, target};
use crate::ui::harness::*;
use crate::widgets::button::Button;
use crate::widgets::panel::Panel;

#[test]
fn collisions_surface_duplicate_explicit_ids() {
    // Two siblings under one explicit id — invisible at runtime except
    // as a magenta overlay, and invisible to a test without this.
    let mut harness = UiHarness::new(SURFACE);
    harness.frame(|ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            for _ in 0..2 {
                Button::new().id(target()).label("dup").show(ui);
            }
        });
    });

    let collisions = harness.collisions();
    assert_eq!(collisions.len(), 1, "one colliding pair");
    assert_eq!(collisions[0].0, target(), "reported under the explicit id");

    let mut clean = UiHarness::new(SURFACE);
    clean.frame(button);
    assert!(clean.collisions().is_empty(), "distinct ids do not collide");
}

#[test]
fn clipboard_round_trips_through_the_harness() {
    let mut harness = UiHarness::new(SURFACE);
    assert_eq!(harness.clipboard_text(), "");
    harness.set_clipboard_text("copied");
    assert_eq!(harness.clipboard_text(), "copied");
}

#[test]
fn arena_interns_without_ever_recording() {
    let mut harness = UiHarness::arena();
    let interned = harness.ui().intern("label");
    // Lowered through the real path rather than read off the handle:
    // `InternedStr` is a span plus an epoch and owns nothing, so the
    // store is the only thing that can resolve it — which is exactly the
    // property this harness exists to make reachable without a frame.
    let store = &harness.ui.forest.record_store;
    let recorded = store.record_text(interned);
    let payloads = store.payloads.borrow();
    assert_eq!(payloads.interned_text().resolve(recorded.span), "label");
}

#[test]
fn from_resources_pairs_two_harnesses_onto_one_text_cache() {
    let shared = HostShared::new(TextShaper::new(), TextureLimit::default());
    let mut first = UiHarness::from_resources(shared.resources.clone(), SURFACE);
    let second = UiHarness::from_resources(shared.resources.clone(), SURFACE);

    first.set_clipboard_text("shared");
    assert_eq!(
        second.clipboard_text(),
        "shared",
        "one HostShared, one clipboard",
    );
}
