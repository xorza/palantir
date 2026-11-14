use crate::driver::action::Action;
use crate::driver::error::{LocateError, PlayError};
use crate::driver::locator::{Found, Locate, Locator, Reach};
use crate::driver::play::{self, Stage};
use crate::host::harness::Harness;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::input::input_event::InputEvent;
use crate::primitives::input::keyboard::key::Key;
use crate::primitives::input::keyboard::modifiers::Modifiers;
use crate::primitives::input::pointer::PointerButton;
use crate::primitives::input::sense::Sense;
use crate::primitives::input::shortcut::{Shortcut, ShortcutMods};
use crate::primitives::layout::sizing::{SizeSpec, Sizing};
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widgets::button::Button;
use crate::widgets::panel::Panel;
use crate::widgets::text::Text;
use glam::{UVec2, Vec2};
use std::convert::Infallible;
use std::num::NonZeroU32;
use std::time::Duration;

/// Buttons 60 × 20 on a canvas: "One" at (10, 10), "Two" at (10, 40), two "Twin" at (10, 70) and
/// (10, 100), "Over" recorded on top of "Under" at (100, 10), a "Label" that only hovers on top of
/// "Behind" at (100, 40), a "Dup" under "Cover" at (100, 70) and another "Dup" free at (100, 100),
/// a disabled "Off" at (200, 70), and "Hidden" placed below a 40 × 20 clip at (200, 10).
fn scene(ui: &mut Ui) {
    let button = |ui: &mut Ui, id: &str, label: &str, pos: Vec2| {
        Button::new()
            .id(WidgetId::from_hash(id))
            .label(label)
            .size((Sizing::fixed(60.0), Sizing::fixed(20.0)))
            .at(pos)
            .disabled(label == "Off")
            .show(ui);
    };
    Panel::canvas()
        .auto_id()
        .size(SizeSpec::FILL)
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
                .at(Vec2::new(100.0, 40.0))
                .show(ui);
            button(ui, "dup-covered", "Dup", Vec2::new(100.0, 70.0));
            button(ui, "cover", "Cover", Vec2::new(100.0, 70.0));
            button(ui, "dup-free", "Dup", Vec2::new(100.0, 100.0));
            button(ui, "off", "Off", Vec2::new(200.0, 70.0));
            Panel::canvas()
                .auto_id()
                .size((Sizing::fixed(40.0), Sizing::fixed(20.0)))
                .at(Vec2::new(200.0, 10.0))
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

fn find(h: &Harness, locator: &Locator) -> Result<Found, LocateError> {
    locator.find(&(), h.ui())
}

fn text(text: &str) -> Locator {
    Locator::from(text)
}

/// A widget found by its text and by its id is its visible rect, pressed at the center; a point is
/// itself with an empty rect; an offset moves both the rect and the point.
#[test]
fn a_locator_finds_a_widget_by_text_id_and_point() {
    let h = primed();
    let one = Found::center_of(Rect::new(10.0, 10.0, 60.0, 20.0));
    assert_eq!(find(&h, &text("One")), Ok(one));
    assert_eq!(find(&h, &WidgetId::from_hash("one").into()), Ok(one));
    let point = Vec2::new(3.0, 4.0);
    assert_eq!(
        find(&h, &point.into()),
        Ok(Found {
            rect: Rect::new(3.0, 4.0, 0.0, 0.0),
            point,
        })
    );
    assert_eq!(
        find(&h, &text("One").offset(Vec2::new(5.0, -2.0))),
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
    let h = primed();
    assert_eq!(
        find(&h, &text("Twin")),
        Err(LocateError::Ambiguous {
            locator: r#"Text("Twin")"#.to_owned(),
            matches: vec![
                find(&h, &text("Twin").nth(0)).unwrap(),
                find(&h, &text("Twin").nth(1)).unwrap(),
            ],
        })
    );
    let nth = |n| text("Twin").nth(n);
    assert_eq!(
        find(&h, &nth(1)),
        Ok(Found::center_of(Rect::new(10.0, 100.0, 60.0, 20.0)))
    );
    assert_eq!(
        find(&h, &nth(2)),
        Err(LocateError::NoNth {
            locator: r#"Nth(Text("Twin"), 2)"#.to_owned(),
            count: 2,
        })
    );
    let of_id = Locator::from(WidgetId::from_hash("one")).nth(0);
    assert!(matches!(
        find(&h, &of_id),
        Err(LocateError::NoNth { count: 1, .. })
    ));
}

/// A widget under another is covered at its center, and a text that a covered widget and a free
/// one show finds the free one; one only a clip shows is off screen; a text no widget shows lists
/// the visible texts that contain it, or the first ten; an id no interactive widget has lists
/// nothing. A press refuses a disabled widget, which a hover still finds.
#[test]
fn a_locator_says_why_it_found_nothing() {
    let h = primed();
    assert_eq!(
        find(&h, &text("Under")),
        Err(LocateError::Covered {
            locator: r#"Text("Under")"#.to_owned(),
            point: Vec2::new(130.0, 20.0),
        })
    );
    assert!(find(&h, &text("Over")).is_ok());
    assert_eq!(
        find(&h, &text("Dup")),
        Ok(Found::center_of(Rect::new(100.0, 100.0, 60.0, 20.0)))
    );
    assert!(
        find(&h, &text("Behind")).is_ok(),
        "a label that only hovers takes no press from the button under it"
    );
    assert_eq!(
        find(&h, &text("Hidden")),
        Err(LocateError::OffScreen {
            locator: r#"Text("Hidden")"#.to_owned()
        })
    );
    let listed = |texts: &[&str]| texts.iter().map(|text| (*text).to_owned()).collect();
    assert_eq!(
        find(&h, &text("tw")),
        Err(LocateError::NotFound {
            locator: r#"Text("tw")"#.to_owned(),
            nearest: listed(&["Two", "Twin", "Twin"]),
        })
    );
    assert_eq!(
        find(&h, &text("zzz")),
        Err(LocateError::NotFound {
            locator: r#"Text("zzz")"#.to_owned(),
            nearest: listed(&[
                "One", "Two", "Twin", "Twin", "Under", "Over", "Behind", "Label", "Dup", "Cover"
            ]),
        })
    );
    assert!(matches!(
        find(&h, &WidgetId::from_hash("absent").into()),
        Err(LocateError::NotFound { nearest, .. }) if nearest.is_empty()
    ));
    let off = text("Off");
    assert_eq!(
        off.point_for(&(), h.ui(), Reach::Press),
        Err(LocateError::Disabled {
            locator: r#"Text("Off")"#.to_owned()
        })
    );
    assert_eq!(
        off.point_for(&(), h.ui(), Reach::Hover),
        Ok(Vec2::new(230.0, 80.0))
    );
    assert_eq!(
        text("One").point_for(&(), h.ui(), Reach::Press),
        Ok(Vec2::new(40.0, 20.0))
    );
}

/// The places an app names: a list of rects, by index.
#[derive(Clone, Debug)]
struct Slot(usize);

impl Locate<Vec<Rect>> for Slot {
    fn find_all(&self, app: &Vec<Rect>, _ui: &Ui, out: &mut Vec<Found>) -> Result<(), LocateError> {
        out.extend(app.iter().skip(self.0).map(|rect| Found::center_of(*rect)));
        if out.is_empty() {
            return Err(LocateError::NotFound {
                locator: format!("{self:?}"),
                nearest: Vec::new(),
            });
        }
        Ok(())
    }
}

/// A place that names nothing and returns no error of its own.
#[derive(Clone, Debug)]
struct Nowhere;

impl Locate<Vec<Rect>> for Nowhere {
    fn find_all(
        &self,
        _app: &Vec<Rect>,
        _ui: &Ui,
        _out: &mut Vec<Found>,
    ) -> Result<(), LocateError> {
        Ok(())
    }
}

/// An app's place resolves through its `Locate`: one match is found, several are ambiguous until
/// `Nth` picks one, and its own error passes through. One that returns `Ok` with nothing reads as
/// `NotFound` with no nearest, alone and under `Nth`.
#[test]
fn an_app_names_its_own_places() {
    let h = primed();
    let app = vec![
        Rect::new(0.0, 0.0, 10.0, 10.0),
        Rect::new(20.0, 0.0, 10.0, 10.0),
    ];
    let slot = |n| Locator::App(Slot(n));
    let ui = h.ui();
    assert_eq!(slot(1).find(&app, ui), Ok(Found::center_of(app[1])));
    assert!(matches!(
        slot(0).find(&app, ui),
        Err(LocateError::Ambiguous { matches, .. }) if matches.len() == 2
    ));
    assert_eq!(slot(0).nth(1).find(&app, ui), Ok(Found::center_of(app[1])));
    assert!(matches!(
        slot(2).find(&app, ui),
        Err(LocateError::NotFound { .. })
    ));
    let nowhere = LocateError::NotFound {
        locator: "App(Nowhere)".to_owned(),
        nearest: Vec::new(),
    };
    let named = Locator::App(Nowhere);
    assert_eq!(named.find(&app, ui), Err(nowhere.clone()));
    assert_eq!(named.nth(0).find(&app, ui), Err(nowhere));
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
            matches: vec![
                Found::center_of(Rect::new(30.0, 70.0, 20.0, 20.0)),
                Found::center_of(Rect::new(30.5, 100.0, 20.0, 20.0)),
            ],
        }),
        "L matches 2 places; pick one with Nth: 0: at (40, 80), 1: at (40.5, 110)"
    );
    assert_eq!(
        error(LocateError::Covered {
            locator: "L".to_owned(),
            point: Vec2::new(1.0, 2.5),
        }),
        "L lies under another widget at (1, 2.5); close what covers it"
    );
    assert_eq!(
        error(LocateError::Disabled {
            locator: "L".to_owned()
        }),
        "L is disabled; a press there does nothing"
    );
}

/// What a [`ScriptedStage`] saw, in order.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Seen {
    Mods(Modifiers),
    Press,
    Release,
    MoveTo(f32),
    KeyDown(Key),
    KeyUp(Key),
    Frame,
    Settle,
    Space,
    Noted,
}

/// A window whose frames fail from `fail_from` on, as a remote's do once a native dialog holds the
/// window's thread, each failure naming its frame in `waited`.
#[derive(Debug, Default)]
struct ScriptedStage {
    fail_from: Option<u64>,
    frames: u64,
    seen: Vec<Seen>,
}

impl Stage<Infallible> for ScriptedStage {
    fn point(&mut self, at: &Locator, _reach: Reach) -> Result<Vec2, PlayError> {
        let Locator::Point(point) = at else {
            panic!("the table names points: {at:?}");
        };
        Ok(*point)
    }

    fn input(&mut self, event: InputEvent<'_>) {
        self.seen.push(match event {
            InputEvent::ModifiersChanged(mods) => Seen::Mods(mods),
            InputEvent::PointerPressed(PointerButton::Left) => Seen::Press,
            InputEvent::PointerReleased(PointerButton::Left) => Seen::Release,
            InputEvent::PointerMoved(point) => Seen::MoveTo(point.x),
            InputEvent::KeyDown { key, .. } => Seen::KeyDown(key),
            InputEvent::KeyUp { key } => Seen::KeyUp(key),
            event => panic!("the table sends no {event:?}"),
        });
    }

    fn frame(&mut self) -> Result<(), PlayError> {
        self.seen.push(Seen::Frame);
        let frame = self.frames;
        self.frames += 1;
        if self.fail_from.is_some_and(|from| frame >= from) {
            return Err(PlayError::NoFrame {
                waited: Duration::from_millis(frame),
            });
        }
        Ok(())
    }

    fn settle(&mut self) -> Result<(), PlayError> {
        self.seen.push(Seen::Settle);
        Ok(())
    }

    fn wait(&mut self, _dt: Duration) -> Result<(), PlayError> {
        panic!("the table waits for nothing");
    }

    fn space_press(&mut self) -> Result<(), PlayError> {
        self.seen.push(Seen::Space);
        Ok(())
    }

    fn note_press(&mut self) {
        self.seen.push(Seen::Noted);
    }
}

/// A frame that fails mid-step leaves nothing held: each press and key still comes back up and the
/// modifiers clear, a drag takes no move past the failure, no later frame runs, the first failure
/// is the step's error, and a press still counts for the next one's spacing. Only a `KeyDown` that
/// succeeds keeps its key down. A step with no modifiers sends no change of them, so a key a
/// person holds stays held.
#[test]
fn a_failed_frame_releases_what_the_step_holds() {
    use Seen::{Frame, MoveTo, Noted, Press, Release, Settle, Space};

    let chord = Shortcut::ctrl('O');
    let (down, up) = (Seen::KeyDown(chord.key), Seen::KeyUp(chord.key));
    let (ctrl, none) = (Seen::Mods(Modifiers::CTRL), Seen::Mods(Modifiers::NONE));
    let mods = chord.mods;
    let plain = Shortcut::key(chord.key);
    let at = Locator::Point(Vec2::new(10.0, 0.0));
    let drag = Action::Drag {
        from: at.clone(),
        to: Locator::Point(Vec2::new(30.0, 0.0)),
        button: PointerButton::Left,
        mods,
        steps: NonZeroU32::new(2).unwrap(),
    };
    let plain_drag = Action::Drag {
        from: at.clone(),
        to: Locator::Point(Vec2::new(30.0, 0.0)),
        button: PointerButton::Left,
        mods: ShortcutMods::NONE,
        steps: NonZeroU32::MIN,
    };
    let click = Action::Click {
        at: at.clone(),
        button: PointerButton::Left,
        mods,
    };
    let plain_click = Action::click(at.clone());
    let double = Action::DoubleClick(at);
    let hover = [MoveTo(10.0), Settle, Space];
    let one_click = [Press, Frame, Release, Frame];
    let cases: [(&str, Action, Option<u64>, Vec<Seen>); 19] = [
        (
            "a key up",
            Action::KeyUp(chord),
            None,
            vec![ctrl, up, none, Settle],
        ),
        (
            "a key with no modifiers",
            Action::Key(plain),
            None,
            vec![down, Frame, up, Settle],
        ),
        (
            "a key down with no modifiers",
            Action::KeyDown(plain),
            None,
            vec![down, Frame, Settle],
        ),
        (
            "a key up with no modifiers",
            Action::KeyUp(plain),
            None,
            vec![up, Settle],
        ),
        (
            "a click with no modifiers",
            plain_click,
            None,
            [&hover[..], &one_click[..], &[Noted, Settle]].concat(),
        ),
        (
            "a drag with no modifiers",
            plain_drag,
            None,
            [
                &hover[..],
                &[
                    Press,
                    Frame,
                    MoveTo(30.0),
                    Frame,
                    Release,
                    Frame,
                    Noted,
                    Settle,
                ],
            ]
            .concat(),
        ),
        (
            "a key",
            Action::Key(chord),
            None,
            vec![ctrl, down, Frame, up, none, Settle],
        ),
        (
            "a key's frame fails",
            Action::Key(chord),
            Some(0),
            vec![ctrl, down, Frame, up, none],
        ),
        (
            "a key down",
            Action::KeyDown(chord),
            None,
            vec![ctrl, down, Frame, none, Settle],
        ),
        (
            "a key down's frame fails",
            Action::KeyDown(chord),
            Some(0),
            vec![ctrl, down, Frame, up, none],
        ),
        (
            "a drag",
            drag.clone(),
            None,
            [
                &hover[..],
                &[
                    ctrl,
                    Press,
                    Frame,
                    MoveTo(20.0),
                    Frame,
                    MoveTo(30.0),
                    Frame,
                    Release,
                    Frame,
                    none,
                    Noted,
                    Settle,
                ],
            ]
            .concat(),
        ),
        (
            "a drag's press frame fails",
            drag.clone(),
            Some(0),
            [&hover[..], &[ctrl, Press, Frame, Release, none, Noted]].concat(),
        ),
        (
            "a drag's first move fails",
            drag.clone(),
            Some(1),
            [
                &hover[..],
                &[
                    ctrl,
                    Press,
                    Frame,
                    MoveTo(20.0),
                    Frame,
                    Release,
                    none,
                    Noted,
                ],
            ]
            .concat(),
        ),
        (
            "a drag's release frame fails",
            drag,
            Some(3),
            [
                &hover[..],
                &[
                    ctrl,
                    Press,
                    Frame,
                    MoveTo(20.0),
                    Frame,
                    MoveTo(30.0),
                    Frame,
                    Release,
                    Frame,
                    none,
                    Noted,
                ],
            ]
            .concat(),
        ),
        (
            "a click",
            click.clone(),
            None,
            [
                &hover[..],
                &[ctrl, Press, Frame, Release, Frame, none, Noted, Settle],
            ]
            .concat(),
        ),
        (
            "a click's press frame fails",
            click.clone(),
            Some(0),
            [&hover[..], &[ctrl, Press, Frame, Release, none, Noted]].concat(),
        ),
        (
            "a click's release frame fails",
            click,
            Some(1),
            [
                &hover[..],
                &[ctrl, Press, Frame, Release, Frame, none, Noted],
            ]
            .concat(),
        ),
        (
            "a double click's first click fails",
            double.clone(),
            Some(0),
            [&hover[..], &[Press, Frame, Release, Noted]].concat(),
        ),
        (
            "a double click's second click fails",
            double,
            Some(2),
            [&hover[..], &one_click[..], &[Press, Frame, Release, Noted]].concat(),
        ),
    ];
    for (label, action, fail_from, want) in cases {
        let mut stage = ScriptedStage {
            fail_from,
            ..ScriptedStage::default()
        };
        let played = play::play(&mut stage, &action);
        let error = fail_from.map(|frame| PlayError::NoFrame {
            waited: Duration::from_millis(frame),
        });
        assert_eq!(played.err(), error, "{label}");
        assert_eq!(stage.seen, want, "{label}");
    }
}

/// A typed character's key: a printing one is its `Char`, a newline Enter and a tab Tab, as a
/// keyboard types them; any other control character, escape and NUL among them, types no key.
#[test]
fn a_typed_character_is_the_key_a_keyboard_types_it_with() {
    for (c, key) in [
        ('a', Some(Key::Char('a'))),
        (' ', Some(Key::Char(' '))),
        ('ζ', Some(Key::Char('ζ'))),
        ('\n', Some(Key::Enter)),
        ('\t', Some(Key::Tab)),
        ('\u{1b}', None),
        ('\r', None),
        ('\0', None),
    ] {
        assert_eq!(play::typed_key(c), key, "{c:?}");
    }
}
