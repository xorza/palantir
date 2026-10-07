//! Cross-frame state rows and the scope that lends them.

use crate::internals::harness::UiHarness;
use crate::primitives::identity::widget_id::WidgetId;
use crate::ui::tests::support::SURFACE;
use crate::widgets::{button::Button, panel::Panel, text::Text};

/// `Ui::with_state` lends the row, live alongside the `Ui`.
#[test]
fn with_state_lends_a_row_across_widget_calls() {
    #[derive(Default, Debug, PartialEq)]
    struct Page {
        clicks: u32,
        note: String,
    }

    let id = WidgetId::from_hash("with-state-page");
    let mut h = UiHarness::new(SURFACE);
    h.frame(|ui| {
        ui.with_state::<Page, _>(id, |ui, page| {
            Panel::vstack().show(ui, |ui| {
                page.clicks += 1;
                Button::new().label("a").show(ui);
                page.note.push('x');
                Text::new(&page.note).show(ui);
                page.clicks += 10;
            });
        });
    });

    assert_eq!(
        h.ui.state::<Page>(id),
        Some(&Page {
            clicks: 11,
            note: "x".into(),
        }),
        "every write inside the scope lands back in the row",
    );
}

/// Restoring re-probes the store rather than holding a pointer; other same-`T` rows can reallocate it.
#[test]
fn with_state_survives_the_body_growing_its_own_store() {
    let outer = WidgetId::from_hash("grow-outer");
    let mut h = UiHarness::new(SURFACE);
    h.frame(|ui| {
        ui.with_state::<u32, _>(outer, |ui, value| {
            *value = 7;
            for i in 0..64u64 {
                ui.with_state::<u32, _>(WidgetId::from_hash(("filler", i)), |_, s| *s = i as u32);
            }
        });
    });
    assert_eq!(h.ui.state::<u32>(outer), Some(&7));
}

/// Rows of different types nest.
#[test]
fn with_state_scopes_nest_by_type() {
    #[derive(Default, Debug)]
    struct App(u32);
    #[derive(Default, Debug)]
    struct Page(u32);

    let app_id = WidgetId::from_hash("nest-app");
    let page_id = WidgetId::from_hash("nest-page");
    let mut h = UiHarness::new(SURFACE);
    h.frame(|ui| {
        ui.with_state::<App, _>(app_id, |ui, app| {
            app.0 = 1;
            ui.with_state::<Page, _>(page_id, |_ui, page| {
                page.0 = 2;
            });
            app.0 += 10;
        });
    });
    assert_eq!(h.ui.state::<App>(app_id).map(|a| a.0), Some(11));
    assert_eq!(h.ui.state::<Page>(page_id).map(|p| p.0), Some(2));
}

#[test]
fn with_state_returns_the_body_value() {
    let id = WidgetId::from_hash("with-state-return");
    let mut h = UiHarness::new(SURFACE);
    let out = h.frame_value(|ui| {
        ui.with_state::<u32, _>(id, |_ui, v| {
            *v = 5;
            *v * 3
        })
    });
    assert_eq!(out, 15);
    assert_eq!(h.ui.state::<u32>(id), Some(&5));
}

/// A singleton is one value per type for the `Ui`'s life, never swept as no id owns it.
#[test]
fn a_singleton_is_lent_across_widget_calls_and_kept() {
    #[derive(Default, Debug, PartialEq)]
    struct Shared(u32);

    let mut h = UiHarness::new(SURFACE);
    h.frame(|ui| {
        assert_eq!(ui.singleton::<Shared>(), None, "nothing stored yet");
        ui.with_singleton::<Shared, _>(|ui, shared| {
            shared.0 += 1;
            Button::new().label("a").show(ui);
            shared.0 += 10;
        });
    });
    assert_eq!(h.ui.singleton::<Shared>(), Some(&Shared(11)));
    h.frame(|_| {});
    h.frame(|_| {});
    h.ui.with_singleton::<Shared, _>(|_, shared| shared.0 += 100);
    assert_eq!(
        h.ui.singleton::<Shared>(),
        Some(&Shared(111)),
        "kept across frames"
    );
}
