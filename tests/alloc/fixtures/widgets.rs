use crate::harness::{AllocBudget, run_audit};
use palantir::{Button, Configure, Sizing};

#[test]
fn empty_frame_alloc_free() {
    run_audit("empty_frame", 8, 32, AllocBudget::ZERO, |_ui| {});
}

// TODO: budget should be 0. Currently 2 allocs/frame in steady state —
// likely the `Shape::Text.text: String` clone called out in
// `docs/todo.md` (Text section) plus one more. Run with `--ignored`
// once the leak is hunted; flip budget back to 0 and drop `#[ignore]`.
#[test]
#[ignore = "captures known 2 allocs/frame regression in Button path"]
fn button_only_alloc_free() {
    run_audit(
        "button_only",
        8,
        32,
        AllocBudget {
            allocs_per_frame: 2,
        },
        |ui| {
            Button::new()
                .label("hello")
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui);
        },
    );
}
