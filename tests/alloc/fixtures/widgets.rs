use crate::harness::{AllocBudget, audit_until_stable};
use palantir::{Button, Configure, Sizing};

#[test]
fn empty_frame_alloc_free() {
    audit_until_stable("empty_frame", AllocBudget::ZERO, |_ui| {});
}

#[test]
fn button_only_alloc_free() {
    audit_until_stable("button_only", AllocBudget::ZERO, |ui| {
        Button::new()
            .label("hello")
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui);
    });
}
