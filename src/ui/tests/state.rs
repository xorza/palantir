//! Cross-frame state: the row a widget keeps, and the scope a subtree
//! borrows it in.

use crate::primitives::widget_id::WidgetId;
use crate::ui::harness::UiHarness;
use crate::ui::tests::support::SURFACE;
use crate::widgets::{button::Button, panel::Panel, text::Text};

/// The whole reason [`Ui::with_state`](crate::Ui::with_state) exists: the
/// row is live *at the same time as* the `Ui`, so a subtree can read and
/// write it around the widget calls it drives. `state_mut`'s borrow cannot
/// survive the first of those.
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
        h.ui.try_state::<Page>(id),
        Some(&Page {
            clicks: 11,
            note: "x".into(),
        }),
        "every write inside the scope lands back in the row",
    );
}

/// Restoring re-probes the store instead of holding a pointer across the
/// body — rows of the same `T` inserted at other ids can reallocate it.
#[test]
fn with_state_survives_the_body_growing_its_own_store() {
    let outer = WidgetId::from_hash("grow-outer");
    let mut h = UiHarness::new(SURFACE);
    h.frame(|ui| {
        ui.with_state::<u32, _>(outer, |ui, value| {
            *value = 7;
            // Enough same-`T` rows to force the dense store's `Vec` to
            // reallocate while the outer row is out on loan.
            for i in 0..64u64 {
                *ui.state_mut::<u32>(WidgetId::from_hash(("filler", i))) = i as u32;
            }
        });
    });
    assert_eq!(h.ui.try_state::<u32>(outer), Some(&7));
}

/// Rows of different types nest, which is what lets a page scope sit
/// inside an app scope.
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
    assert_eq!(h.ui.try_state::<App>(app_id).map(|a| a.0), Some(11));
    assert_eq!(h.ui.try_state::<Page>(page_id).map(|p| p.0), Some(2));
}

/// The scope returns whatever the body returns, so a page can hand a
/// decision back out without a captured cell.
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
    assert_eq!(h.ui.try_state::<u32>(id), Some(&5));
}
