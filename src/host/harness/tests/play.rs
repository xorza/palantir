use crate::app::App;
use crate::display::Display;
use crate::driver::action::Action;
use crate::driver::error::{ActError, LocateError, PlayError};
use crate::driver::locator::Locator;
use crate::gpu::test_gpu::headless_test_gpu;
use crate::host::harness::Harness;
use crate::host::harness::harness_builder::Depth;
use crate::input::keyboard::key::Key;
use crate::input::keyboard::modifiers::Modifiers;
use crate::input::pointer::PointerButton;
use crate::input::shortcut::{Shortcut, ShortcutMods};
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::ui::Ui;
use crate::ui::frame_report::FramePaint;
use crate::widget_core::configure::Configure;
use crate::widget_core::response::Response;
use crate::widgets::block::Block;
use crate::widgets::button::Button;
use crate::widgets::panel::Panel;
use crate::widgets::text_edit::TextEdit;
use crate::window::window_commands::WindowCommand;
use crate::window::window_config::WindowConfig;
use crate::window::window_token::WindowToken;
use glam::{UVec2, Vec2};
use std::convert::Infallible;
use std::time::Duration;

fn tile(ui: &mut Ui) {
    Block::new()
        .id_salt("tile")
        .size(20.0)
        .chrome(Chrome::fill(RgbaF32::WHITE))
        .show(ui);
}

/// A record frame builds no draw list; a compose frame builds the scene's, here the one tile at
/// the origin, physical as logical at scale 1. A second still frame of either plans no paint and
/// asks no repaint, so it builds nothing: the buffer, emptied, stays empty.
#[test]
fn each_depth_builds_what_it_names() {
    let tile_rect = Rect::new(0.0, 0.0, 20.0, 20.0);
    for (depth, built) in [(Depth::Record, vec![]), (Depth::Compose, vec![tile_rect])] {
        let mut h = Harness::builder(UVec2::new(64, 32))
            .mono()
            .depth(depth)
            .build();
        let quads = |h: &Harness| -> Vec<Rect> {
            h.render_buffer()
                .quads
                .iter()
                .map(|quad| quad.rect)
                .collect()
        };
        let first = h.frame(tile);
        assert_eq!(first.paint(), FramePaint::Full);
        assert_eq!(quads(&h), built);

        let still = h.frame(tile);
        assert_eq!(still.paint(), FramePaint::Skip);
        assert!(!still.repaint_requested);
        assert_eq!(quads(&h), built, "a still frame builds nothing new");
    }
}

/// A frame that opens and closes windows records each command, in order, and runs on.
#[test]
fn a_frame_that_opens_a_window_records_the_command() {
    let mut h = Harness::mono(UVec2::new(64, 32));
    let mut opened = false;
    h.frame(|ui: &mut Ui| {
        if !opened {
            ui.open_window(WindowToken(3), WindowConfig::new("Inspector"));
            opened = true;
        }
    });
    h.frame(|ui: &mut Ui| ui.close_window(WindowToken(3)));
    let commands = h.window_commands();
    assert_eq!(commands.len(), 2, "{commands:?}");
    assert!(
        matches!(&commands[0], WindowCommand::Open { token: WindowToken(3), config } if config.title == "Inspector")
    );
    assert!(matches!(commands[1], WindowCommand::Close(WindowToken(3))));
}

/// The app a person plays: buttons 80 × 24 on a canvas, "One" at (10, 10) and two "Twin" at
/// (10, 40) and (10, 70), a text edit at (10, 100), and "Hidden" placed below a 40 × 20 clip at
/// (200, 10). It counts what each button saw and keeps what the editor committed.
#[derive(Debug, Default)]
struct Board {
    one: u32,
    twin_b: u32,
    double: u32,
    text: String,
    committed: Option<String>,
}

fn edit() -> WidgetId {
    WidgetId::from_hash("edit")
}

fn button<'a>(ui: &'a mut Ui, id: &str, label: &str, pos: Vec2) -> Response<'a> {
    Button::new()
        .id(WidgetId::from_hash(id))
        .label(label)
        .size((Sizing::fixed(80.0), Sizing::fixed(24.0)))
        .pos(pos)
        .show(ui)
}

impl App for Board {
    fn record(&mut self, _window: WindowToken, ui: &mut Ui) {
        Panel::canvas()
            .auto_id()
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                let one = button(ui, "one", "One", Vec2::new(10.0, 10.0));
                if one.clicked() {
                    self.one += 1;
                }
                if one.double_clicked() {
                    self.double += 1;
                }
                button(ui, "twin-a", "Twin", Vec2::new(10.0, 40.0));
                if button(ui, "twin-b", "Twin", Vec2::new(10.0, 70.0)).clicked() {
                    self.twin_b += 1;
                }
                let edit = TextEdit::new(&mut self.text)
                    .id(edit())
                    .size((Sizing::fixed(120.0), Sizing::fixed(24.0)))
                    .pos(Vec2::new(10.0, 100.0))
                    .show(ui);
                if edit.committed {
                    self.committed = Some(self.text.clone());
                }
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
}

/// A person plays the board on a painting window: a click by text clicks once; "Twin" names two
/// and refuses, `Nth` picks the second; a clipped button is out of view; two clicks are two clicks
/// and a double click one double click; typed text commits on Enter. A failed step names its
/// index.
#[test]
fn play_acts_as_a_person() {
    let gpu = headless_test_gpu();
    let mut h = Harness::builder(UVec2::new(400, 200))
        .gpu(gpu.handles())
        .build();
    let mut app = Board::default();
    h.settle(&mut app).unwrap();

    h.play(&mut app, &[Action::click("One")]).unwrap();
    assert_eq!((app.one, app.double), (1, 0));

    let twin = Locator::from("Twin");
    let Err(ActError {
        ran: 0,
        error: PlayError::Locate(LocateError::Ambiguous { matches, .. }),
    }) = h.play(&mut app, &[Action::click(twin.clone())])
    else {
        panic!("two buttons show \"Twin\"");
    };
    assert_eq!(matches.len(), 2);
    h.play(&mut app, &[Action::click(Locator::Nth(Box::new(twin), 1))])
        .unwrap();
    assert_eq!(app.twin_b, 1);

    assert!(matches!(
        h.play(
            &mut app,
            &[Action::Move("One".into()), Action::click("Hidden")]
        ),
        Err(ActError {
            ran: 1,
            error: PlayError::Locate(LocateError::OffScreen { .. }),
        })
    ));

    h.play(&mut app, &[Action::click("One"), Action::click("One")])
        .unwrap();
    assert_eq!((app.one, app.double), (3, 0), "two clicks, spaced apart");
    h.play(&mut app, &[Action::<Infallible>::DoubleClick("One".into())])
        .unwrap();
    assert_eq!(app.double, 1, "one double click");

    h.play(
        &mut app,
        &[
            Action::click(edit()),
            Action::Type("hello".to_owned()),
            Action::key(Shortcut::key(Key::Enter)),
        ],
    )
    .unwrap();
    assert_eq!(app.committed.as_deref(), Some("hello"));
}

/// Each step settles: a `Wait` runs frames of one frame's time until it passed, and a click with
/// modifiers releases them, so no modifier reads as held after it.
#[test]
fn a_step_settles_and_lets_go() {
    let mut h = Harness::mono(UVec2::new(400, 200));
    let mut app = Board::default();
    h.settle(&mut app).unwrap();
    let before = h.time;
    h.play(
        &mut app,
        &[Action::<Infallible>::Wait(Duration::from_millis(100))],
    )
    .unwrap();
    assert!(h.time >= before + Duration::from_millis(100));
    h.play(
        &mut app,
        &[Action::<Infallible>::Click {
            at: "One".into(),
            button: PointerButton::Left,
            mods: ShortcutMods::CTRL_SHIFT,
        }],
    )
    .unwrap();
    assert_eq!(app.one, 1);
    assert_eq!(
        h.ui_ref().input().modifiers_after_held_input(),
        Modifiers::NONE
    );
}

/// An app that asks a repaint each frame never settles: the settle stops at the builder's bound,
/// 600 frames, and names the step.
#[test]
fn an_app_that_always_repaints_is_unsettled() {
    let mut h = Harness::mono(UVec2::new(64, 32));
    let mut app = |ui: &mut Ui| ui.request_repaint();
    assert_eq!(
        h.settle(&mut app),
        Err(PlayError::Unsettled {
            step: "the settle".to_owned(),
            frames: 600
        })
    );
}

/// A shot at scale 1.5 of a 400 × 200 logical window is 600 × 300 texels; a shot of the logical
/// region (10, 10, 20, 10) covers its physical pixels, 15..45 × 15..30: 30 × 15. A window that
/// paints nothing has no shot, and a region outside the window holds no pixel.
#[test]
fn a_shot_is_the_painted_window() {
    let gpu = headless_test_gpu();
    let mut h = Harness::builder(UVec2::new(600, 300))
        .scale(1.5)
        .gpu(gpu.handles())
        .build();
    let mut app = Board::default();
    h.settle(&mut app).unwrap();
    let whole = h.shot(&mut app, None).unwrap();
    assert_eq!(whole.image().size(), UVec2::new(600, 300));
    assert_eq!(whole.scale(), 1.5);
    let region = h
        .shot(&mut app, Some(Rect::new(10.0, 10.0, 20.0, 10.0)))
        .unwrap();
    assert_eq!(region.image().size(), UVec2::new(30, 15));
    assert_eq!(
        h.shot(&mut app, Some(Rect::new(900.0, 900.0, 5.0, 5.0))),
        Err(PlayError::EmptyRegion)
    );

    let mut cpu = Harness::mono(UVec2::new(64, 32));
    assert_eq!(cpu.shot(&mut app, None), Err(PlayError::NoPixels));
}

/// The input methods aim by text, by id and by point alike, and `widgets` lists what a locator
/// reads, by text, ignoring case, the clipped one left out.
#[test]
fn input_aims_by_text_id_and_point() {
    let mut h = Harness::new(UVec2::new(400, 200));
    let mut app = Board::default();
    h.prime(2, |ui: &mut Ui| app.record(WindowToken(0), ui));
    h.click("One");
    h.frame(|ui: &mut Ui| app.record(WindowToken(0), ui));
    h.advance_past_double_click();
    h.click(WidgetId::from_hash("one"));
    h.frame(|ui: &mut Ui| app.record(WindowToken(0), ui));
    h.advance_past_double_click();
    h.click(Vec2::new(20.0, 20.0));
    h.frame(|ui: &mut Ui| app.record(WindowToken(0), ui));
    assert_eq!(app.one, 3);

    let texts: Vec<String> = h
        .widgets(Some("twin"))
        .into_iter()
        .map(|widget| widget.text)
        .collect();
    assert_eq!(texts, ["Twin", "Twin"]);
    assert!(h.widgets(Some("hidden")).is_empty());
}

/// A white 2 × 1 logical block at the origin over a black clear. At scale 1 on 8 × 6 it covers
/// physical x 0..2, y 0..1; moved to 4 × 4 at scale 2 it covers x 0..4, y 0..2. Each read-back is
/// the frame just drawn, at the window's size.
#[test]
fn a_new_display_reads_back_the_new_size_and_scale() {
    let gpu = headless_test_gpu();
    let mut h = Harness::builder(UVec2::new(8, 6))
        .mono()
        .gpu(gpu.handles())
        .build();
    h.ui().theme_mut().window_clear = RgbaF32::BLACK;
    let app = |ui: &mut Ui| {
        Block::new()
            .id_salt("white")
            .size((2.0, 1.0))
            .chrome(Chrome::fill(RgbaF32::WHITE))
            .show(ui);
    };
    let white = |x0: u32, x1: u32, y0: u32, y1: u32| {
        move |x: u32, y: u32| (x0..x1).contains(&x) && (y0..y1).contains(&y)
    };
    for (size, scale, inside) in [
        (UVec2::new(8, 6), 1.0, white(0, 2, 0, 1)),
        (UVec2::new(4, 4), 2.0, white(0, 4, 0, 2)),
    ] {
        let display = Display {
            physical: size,
            system_scale: scale,
            ..h.ui().display()
        };
        h.set_display(display);
        h.frame(app);
        let image = h.read_back();
        assert_eq!(image.dimensions(), (size.x, size.y), "{size}");
        for (x, y, pixel) in image.enumerate_pixels() {
            let want = if inside(x, y) {
                [255; 4]
            } else {
                [0, 0, 0, 255]
            };
            assert_eq!(pixel.0, want, "{size} at ({x}, {y})");
        }
    }
}
