use crate::harness::{AllocBudget, run_audit};
use palantir::{Button, Configure, Sizing};

#[test]
fn empty_frame_alloc_free() {
    run_audit("empty_frame", 8, 32, AllocBudget::ZERO, |_ui| {});
}

#[test]
fn button_only_alloc_free() {
    run_audit("button_only", 16, 64, AllocBudget::ZERO, |ui| {
        Button::new()
            .label("hello")
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui);
    });
}
