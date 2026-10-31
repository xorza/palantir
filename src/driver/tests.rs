use crate::driver::error::LocateError;
use crate::driver::locator::{Found, Locate, Locator};
use crate::host::harness::Harness;
use crate::input::sense::Sense;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widgets::button::Button;
use crate::widgets::panel::Panel;
use crate::widgets::text::Text;
use glam::{UVec2, Vec2};

/// Buttons 60 × 20 on a canvas: "One" at (10, 10), "Two" at (10, 40), two "Twin" at (10, 70) and
/// (10, 100), "Over" recorded on top of "Under" at (100, 10), a "Label" that only hovers on top of
/// "Behind" at (100, 40), and "Hidden" placed below a 40 × 20
/// clip at (200, 10).
fn scene(ui: &mut Ui) {
    let button = |ui: &mut Ui, id: &str, label: &str, pos: Vec2| {
        Button::new()
            .id(WidgetId::from_hash(id))
            .label(label)
            .size((Sizing::fixed(60.0), Sizing::fixed(20.0)))
            .pos(pos)
            .show(ui);
    };
    Panel::canvas()
        .auto_id()
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            button(ui, "one", "One", Vec2::new(10.0, 10.0));
            button(ui, "two", "Two", Vec2::new(10.0, 40.0));
            button(ui, "twin-a", "Twin", Vec2::new(10.0, 70.0));
            button(ui, "twin-b", "Twin", Vec2::new(10.0, 100.0));
            button(ui, "under", "Under", Vec2::new(100.0, 10.0));
            button(ui, "over", "Over", Vec2::new(100.0, 10.0));
            button(ui, "behind", "Behind", Vec2::new(100.0, 40.0));
            Text::new("Label")
                .id(WidgetId::from_hash("label"))
                .sense(Sense::HOVER)
                .pos(Vec2::new(100.0, 40.0))
                .show(ui);
            Panel::canvas()
                .auto_id()
                .size((Sizing::fixed(40.0), Sizing::fixed(20.0)))
                .pos(Vec2::new(200.0, 10.0))
                .clip_rect()
                .show(ui, |ui| {
                    button(ui, "hidden", "Hidden", Vec2::new(0.0, 50.0));
                });
        });
}

fn primed() -> Harness {
    let mut h = Harness::new(UVec2::new(400, 300));
    h.prime(2, scene);
    h
}

fn find(h: &mut Harness, locator: &Locator) -> Result<Found, LocateError> {
    locator.find(&(), h.ui())
}

fn text(text: &str) -> Locator {
    Locator::from(text)
}

/// A widget found by its text and by its id is its visible rect, pressed at the center; a point is
/// itself with an empty rect; an offset moves both the rect and the point.
#[test]
fn a_locator_finds_a_widget_by_text_id_and_point() {
    let mut h = primed();
    let one = Found::rect(Rect::new(10.0, 10.0, 60.0, 20.0));
    assert_eq!(find(&mut h, &text("One")), Ok(one));
    assert_eq!(find(&mut h, &WidgetId::from_hash("one").into()), Ok(one));
    let point = Vec2::new(3.0, 4.0);
    assert_eq!(
        find(&mut h, &point.into()),
        Ok(Found {
            rect: Rect::new(3.0, 4.0, 0.0, 0.0),
            point,
        })
    );
    assert_eq!(
        find(
            &mut h,
            &Locator::Offset(Box::new(text("One")), Vec2::new(5.0, -2.0))
        ),
        Ok(Found {
            rect: Rect::new(15.0, 8.0, 60.0, 20.0),
            point: Vec2::new(45.0, 18.0),
        })
    );
}

/// Two widgets showing one text are ambiguous, each listed at its center; `Nth` picks by paint
/// order and refuses an index past them, or around a locator that names one place.
#[test]
fn nth_picks_among_ambiguous_matches() {
    let mut h = primed();
    assert_eq!(
        find(&mut h, &text("Twin")),
        Err(LocateError::Ambiguous {
            locator: r#"Text("Twin")"#.to_owned(),
            matches: vec![
                "0: at Vec2(40.0, 80.0)".to_owned(),
                "1: at Vec2(40.0, 110.0)".to_owned()
            ],
        })
    );
    let nth = |n| Locator::Nth(Box::new(text("Twin")), n);
    assert_eq!(
        find(&mut h, &nth(1)),
        Ok(Found::rect(Rect::new(10.0, 100.0, 60.0, 20.0)))
    );
    assert_eq!(
        find(&mut h, &nth(2)),
        Err(LocateError::NoNth {
            locator: r#"Nth(Text("Twin"), 2)"#.to_owned(),
            count: 2,
        })
    );
    let of_id = Locator::Nth(Box::new(WidgetId::from_hash("one").into()), 0);
    assert!(matches!(
        find(&mut h, &of_id),
        Err(LocateError::NoNth { count: 1, .. })
    ));
}

/// A widget under another is covered at its center; one only a clip shows is off screen; a text
/// no widget shows lists the visible texts that contain it, or the first ones; an id no
/// interactive widget has lists nothing.
#[test]
fn a_locator_says_why_it_found_nothing() {
    let mut h = primed();
    assert_eq!(
        find(&mut h, &text("Under")),
        Err(LocateError::Covered {
            locator: r#"Text("Under")"#.to_owned(),
            point: Vec2::new(130.0, 20.0),
        })
    );
    assert!(find(&mut h, &text("Over")).is_ok());
    assert!(
        find(&mut h, &text("Behind")).is_ok(),
        "a label that only hovers takes no press from the button under it"
    );
    assert_eq!(
        find(&mut h, &text("Hidden")),
        Err(LocateError::OffScreen {
            locator: r#"Text("Hidden")"#.to_owned()
        })
    );
    let listed = |texts: &[&str]| texts.iter().map(|text| (*text).to_owned()).collect();
    assert_eq!(
        find(&mut h, &text("tw")),
        Err(LocateError::NotFound {
            locator: r#"Text("tw")"#.to_owned(),
            nearest: listed(&["Two", "Twin", "Twin"]),
        })
    );
    assert_eq!(
        find(&mut h, &text("zzz")),
        Err(LocateError::NotFound {
            locator: r#"Text("zzz")"#.to_owned(),
            nearest: listed(&[
                "One", "Two", "Twin", "Twin", "Under", "Over", "Behind", "Label"
            ]),
        })
    );
    assert!(matches!(
        find(&mut h, &WidgetId::from_hash("absent").into()),
        Err(LocateError::NotFound { nearest, .. }) if nearest.is_empty()
    ));
}

/// The places an app names: a list of rects, by index.
#[derive(Debug)]
struct Slot(usize);

impl Locate<Vec<Rect>> for Slot {
    fn find_all(&self, app: &Vec<Rect>, _ui: &Ui, out: &mut Vec<Found>) -> Result<(), LocateError> {
        out.extend(app.iter().skip(self.0).map(|rect| Found::rect(*rect)));
        if out.is_empty() {
            return Err(LocateError::NotFound {
                locator: format!("{self:?}"),
                nearest: Vec::new(),
            });
        }
        Ok(())
    }
}

/// An app's place resolves through its `Locate`: one match is found, several are ambiguous until
/// `Nth` picks one, and its own error passes through.
#[test]
fn an_app_names_its_own_places() {
    let mut h = primed();
    let app = vec![
        Rect::new(0.0, 0.0, 10.0, 10.0),
        Rect::new(20.0, 0.0, 10.0, 10.0),
    ];
    let slot = |n| Locator::App(Slot(n));
    let ui = h.ui();
    assert_eq!(slot(1).find(&app, ui), Ok(Found::rect(app[1])));
    assert!(matches!(
        slot(0).find(&app, ui),
        Err(LocateError::Ambiguous { matches, .. }) if matches.len() == 2
    ));
    assert_eq!(
        Locator::Nth(Box::new(slot(0)), 1).find(&app, ui),
        Ok(Found::rect(app[1]))
    );
    assert!(matches!(
        slot(2).find(&app, ui),
        Err(LocateError::NotFound { .. })
    ));
}

/// Each error tells the reader what to change.
#[test]
fn locate_errors_read_as_corrections() {
    let error = |error: LocateError| error.to_string();
    assert_eq!(
        error(LocateError::NotFound {
            locator: "L".to_owned(),
            nearest: vec!["a".to_owned(), "b".to_owned()],
        }),
        "L matches nothing; the nearest are: a, b"
    );
    assert_eq!(
        error(LocateError::NotFound {
            locator: "L".to_owned(),
            nearest: Vec::new(),
        }),
        "L matches nothing"
    );
    assert_eq!(
        error(LocateError::Ambiguous {
            locator: "L".to_owned(),
            matches: vec!["0: x".to_owned(), "1: y".to_owned()],
        }),
        "L matches 2 places; pick one with Nth: 0: x, 1: y"
    );
    assert_eq!(
        error(LocateError::Covered {
            locator: "L".to_owned(),
            point: Vec2::new(1.0, 2.5),
        }),
        "L lies under another widget at (1, 2.5); close what covers it"
    );
}
