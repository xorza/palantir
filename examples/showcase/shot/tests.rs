use std::path::PathBuf;
use std::time::Duration;
use std::{env, fs, process};

use palantir::glam::{UVec2, Vec2};
use palantir::internals::headless_test_gpu;
use palantir::{Action, Key, Locator, PointerButton, Rect, Shortcut, ShortcutMods};

use crate::shot;
use crate::shot::script::{Header, ParseError, ParseErrorKind, Region, Script, StepKind};
use crate::shot::{Folders, ShotError};

fn steps(text: &str) -> Vec<StepKind> {
    Script::parse(text)
        .unwrap_or_else(|error| panic!("{error}"))
        .steps
        .into_iter()
        .map(|step| step.kind)
        .collect()
}

fn error(text: &str) -> ParseError {
    Script::parse(text).expect_err("the script is refused")
}

fn text(text: &str) -> Locator {
    Locator::Text(text.to_owned())
}

/// The header takes each setting, the default naming the rest; a comment and a blank line are
/// skipped.
#[test]
fn the_header_sets_the_window() {
    let script = Script::parse(
        "// a comment\n\nsize 800x600\nscale 1.5\npage \"overlays\"\nmax-side 1024\n",
    )
    .unwrap();
    assert_eq!(
        script.header,
        Header {
            size: UVec2::new(800, 600),
            scale: 1.5,
            page: Some("overlays".to_owned()),
            max_side: 1024,
        }
    );
    assert!(script.steps.is_empty());
    assert_eq!(Script::parse("").unwrap().header, Header::default());
}

/// Each locator form: text, text with an index, a point, and either moved by an offset.
#[test]
fn locators_read_each_form() {
    let point = Vec2::new(10.0, 20.5);
    let by = Vec2::new(-3.0, 4.0);
    for (word, want) in [
        ("\"Open popup\"", text("Open popup")),
        ("\"Twin\"#2", Locator::Nth(Box::new(text("Twin")), 2)),
        ("10,20.5", Locator::Point(point)),
        (
            "\"Twin\"#1+-3,4",
            Locator::Offset(Box::new(Locator::Nth(Box::new(text("Twin")), 1)), by),
        ),
        (
            "10,20.5+-3,4",
            Locator::Offset(Box::new(Locator::Point(point)), by),
        ),
        ("\"say \\\"hi\\\"\"", text("say \"hi\"")),
    ] {
        assert_eq!(
            steps(&format!("hover {word}")),
            [StepKind::Act(Action::Move(want))],
            "{word}"
        );
    }
}

/// Each action and read, with their options.
#[test]
fn each_instruction_reads_its_words() {
    let at = || text("A");
    let script = "click \"A\"\n\
                  click \"A\" button right mods ctrl+shift\n\
                  double-click \"A\"\n\
                  drag \"A\" to 5,6 mods alt\n\
                  scroll \"A\" 0,-40\n\
                  pinch \"A\" 1.25\n\
                  key ctrl+z\n\
                  type \"hello world\"\n\
                  wait 300ms\n\
                  wait 2s\n\
                  shot whole\n\
                  shot part region 1,2,30,40 zoom 4\n\
                  shot item region \"A\"\n\
                  probe 3,4\n\
                  widgets\n\
                  widgets \"Save as\"\n";
    assert_eq!(
        steps(script),
        [
            StepKind::Act(Action::click("A")),
            StepKind::Act(Action::Click {
                at: at(),
                button: PointerButton::Right,
                mods: ShortcutMods::CTRL_SHIFT,
            }),
            StepKind::Act(Action::DoubleClick(at())),
            StepKind::Act(Action::Drag {
                from: at(),
                to: Locator::Point(Vec2::new(5.0, 6.0)),
                button: PointerButton::Left,
                mods: ShortcutMods::ALT,
                steps: Action::DRAG_STEPS,
            }),
            StepKind::Act(Action::Scroll {
                at: at(),
                delta: Vec2::new(0.0, -40.0),
            }),
            StepKind::Act(Action::Pinch {
                at: at(),
                factor: 1.25,
            }),
            StepKind::Act(Action::Key(Shortcut::ctrl('z'))),
            StepKind::Act(Action::Type("hello world".to_owned())),
            StepKind::Act(Action::Wait(Duration::from_millis(300))),
            StepKind::Act(Action::Wait(Duration::from_secs(2))),
            StepKind::Shot {
                name: "whole".to_owned(),
                region: None,
                zoom: 1,
            },
            StepKind::Shot {
                name: "part".to_owned(),
                region: Some(Region::Rect(Rect::new(1.0, 2.0, 30.0, 40.0))),
                zoom: 4,
            },
            StepKind::Shot {
                name: "item".to_owned(),
                region: Some(Region::At(at())),
                zoom: 1,
            },
            StepKind::Probe(Vec2::new(3.0, 4.0)),
            StepKind::Widgets(None),
            StepKind::Widgets(Some("Save as".to_owned())),
        ]
    );
    assert_eq!(
        steps("key enter"),
        [StepKind::Act(Action::Key(Shortcut::key(Key::Enter)))]
    );
}

/// Each refusal names its line and the case.
#[test]
fn each_refusal_names_its_line() {
    for (script, line, kind) in [
        ("jump 1,2", 1, ParseErrorKind::Unknown("jump".to_owned())),
        (
            "\nclick \"A\"\nsize 10x10",
            3,
            ParseErrorKind::LateHeader("size".to_owned()),
        ),
        (
            "click",
            1,
            ParseErrorKind::Missing("a place: \"text\", \"text\"#2 or x,y, with +dx,dy to move it"),
        ),
        (
            "size 0x10",
            1,
            ParseErrorKind::Bad {
                expected: "a size WxH in physical pixels",
                found: "0x10".to_owned(),
            },
        ),
        ("hover 1,2 now", 1, ParseErrorKind::Extra("now".to_owned())),
        ("type \"open", 1, ParseErrorKind::OpenQuote),
        (
            "key ctrl+banana",
            1,
            ParseErrorKind::Bad {
                expected: "a key, as ctrl+z or enter",
                found: "ctrl+banana".to_owned(),
            },
        ),
        (
            "click \"A\" button side",
            1,
            ParseErrorKind::Bad {
                expected: "left, right or middle",
                found: "side".to_owned(),
            },
        ),
        (
            "shot a zoom 0",
            1,
            ParseErrorKind::Bad {
                expected: "a zoom of 1 or more",
                found: "0".to_owned(),
            },
        ),
        (
            "wait soon",
            1,
            ParseErrorKind::Bad {
                expected: "a time as 300ms or 2s",
                found: "soon".to_owned(),
            },
        ),
    ] {
        assert_eq!(error(script), ParseError { line, kind }, "{script:?}");
    }
    assert_eq!(
        error("jump").to_string().split(';').next(),
        Some("line 1: unknown instruction `jump`")
    );
}

/// A folder of its own under the system's temporary one, removed when it drops.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let dir = env::temp_dir().join(format!("palantir-shot-{name}-{}", process::id()));
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

/// Pictures into `out`, compared with the ones in `earlier`.
fn against<'a>(out: &'a Scratch, earlier: &'a Scratch) -> Folders<'a> {
    Folders {
        out: &out.0,
        against: Some(&earlier.0),
    }
}

/// Pictures into `scratch`, compared with none.
fn folders(scratch: &Scratch) -> Folders<'_> {
    Folders {
        out: &scratch.0,
        against: None,
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
    }
}

/// A script that opens the overlays page, opens the popup and shoots writes a PNG of the window's
/// physical size, 640 × 400 at scale 1, whose legend lists the popup's items; an unknown page and
/// a failed step are refused, the step by its line.
#[test]
fn a_script_shoots_the_open_popup() {
    let gpu = headless_test_gpu();
    let scratch = Scratch::new("popup");
    let script =
        Script::parse("size 640x400\npage \"OVERLAYS\"\nclick \"Edit ▾\"\nshot popup\nprobe 1,1\n")
            .unwrap();
    let mut report = Vec::new();
    shot::play(gpu.handles(), &script, folders(&scratch), &mut report).unwrap();
    let report = String::from_utf8(report).unwrap();
    let path = scratch.0.join("popup.png");
    let png = image::open(&path).unwrap();
    assert_eq!((png.width(), png.height()), (640, 400));
    let head = format!("shot {} 640x400 region 0,0,640,400 scale 1", path.display());
    assert!(report.starts_with(&head), "{report}");
    for item in ["Copy", "Paste", "Delete"] {
        assert!(
            report.contains(&format!("  \"{item}\" ")),
            "{item}: {report}"
        );
    }
    assert!(
        report.lines().any(|line| line.starts_with("probe 1,1 #")),
        "{report}"
    );

    let unknown = Script::parse("page \"nowhere\"").unwrap();
    let refused = shot::play(gpu.handles(), &unknown, folders(&scratch), &mut Vec::new());
    assert!(matches!(refused, Err(ShotError::Page(_))));
    assert!(
        refused.unwrap_err().to_string().contains("\"overlays\""),
        "an unknown page lists the pages"
    );

    let missing = Script::parse("size 640x400\nclick \"Nothing here\"").unwrap();
    assert!(matches!(
        shot::play(gpu.handles(), &missing, folders(&scratch), &mut Vec::new()),
        Err(ShotError::Play { line: 2, .. })
    ));
}

/// A script shot twice against itself reports the same picture; shot again on another page it
/// reports the pixels that moved and writes the marked difference beside the picture.
#[test]
fn a_shot_compares_with_an_earlier_run() {
    let gpu = headless_test_gpu();
    let first = Scratch::new("first");
    let second = Scratch::new("second");
    let script =
        |page: &str| Script::parse(&format!("size 320x200\npage \"{page}\"\nshot view\n")).unwrap();
    shot::play(
        gpu.handles(),
        &script("controls"),
        folders(&first),
        &mut Vec::new(),
    )
    .unwrap();

    let mut report = Vec::new();
    shot::play(
        gpu.handles(),
        &script("controls"),
        against(&second, &first),
        &mut report,
    )
    .unwrap();
    let report = String::from_utf8(report).unwrap();
    let earlier = first.0.join("view.png");
    assert!(
        report.contains(&format!("  against {}: the same", earlier.display())),
        "{report}"
    );
    assert!(!second.0.join("view.diff.png").exists());

    let mut report = Vec::new();
    shot::play(
        gpu.handles(),
        &script("colors"),
        against(&second, &first),
        &mut report,
    )
    .unwrap();
    let report = String::from_utf8(report).unwrap();
    assert!(report.contains("pixels differ, by at most"), "{report}");
    assert!(second.0.join("view.diff.png").exists());

    let mut report = Vec::new();
    let other = Script::parse("size 160x100\nshot view\n").unwrap();
    shot::play(gpu.handles(), &other, against(&second, &first), &mut report).unwrap();
    let report = String::from_utf8(report).unwrap();
    assert!(report.contains(": 320x200, another size"), "{report}");
    let mut report = Vec::new();
    let unseen = Script::parse("size 160x100\nshot unseen\n").unwrap();
    shot::play(
        gpu.handles(),
        &unseen,
        against(&second, &first),
        &mut report,
    )
    .unwrap();
    assert!(String::from_utf8(report).unwrap().contains(": missing"));
}
