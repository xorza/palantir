//! Every number a theme file can carry is rejected on load or safe for the bundled widgets to
//! render.
//!
//! Each numeric literal in the serialized default theme is replaced in turn with hostile values, and a document that still loads is rendered.

use crate::host::harness::Harness;
use crate::input::interaction::response_state::ResponseState;
use crate::input::shortcut::Shortcut;
use crate::internals::dock_fixture::DockFixture;
use crate::internals::frame_fixture::FrameFixture;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::paint::color::RgbaF32;
use crate::scene::layer::Layer;
use crate::theme::Theme;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widget_core::response::ResponseSnapshot;
use crate::widgets::color_picker::ColorPicker;
use crate::widgets::context_menu::ContextMenu;
use crate::widgets::context_menu::menu_item::MenuItem;
use crate::widgets::context_menu::menu_separator::MenuSeparator;
use crate::widgets::modal::Modal;
use crate::widgets::spinner::Spinner;
use crate::widgets::tooltip::Tooltip;
use glam::{UVec2, Vec2};
use ron::ser::{self, PrettyConfig};
use std::ops::Range;
use std::panic;
use std::time::Duration;

/// The default theme as RON text, the document each number is edited in.
fn default_theme_text() -> String {
    ser::to_string_pretty(&Theme::default(), PrettyConfig::default()).expect("serialize")
}

/// The values each number is set to in turn. `8192.0` is the largest length a file holds
/// (`domain::MAX_FILE_LENGTH`), and `1e30` one far past it.
const SUSPECTS: [&str; 8] = [
    "NaN", "inf", "-inf", "-1.0", "0.0", "1e-30", "8192.0", "1e30",
];

/// Byte ranges of numeric literals in `ron`, outside strings and identifiers.
fn numbers(ron: &str) -> Vec<Range<usize>> {
    let bytes = ron.as_bytes();
    let mut out = Vec::new();
    let (mut i, mut in_string) = (0, false);
    while i < bytes.len() {
        let c = bytes[i];
        if in_string {
            match c {
                b'\\' => i += 1,
                b'"' => in_string = false,
                _ => {}
            }
            i += 1;
            continue;
        }
        if c == b'"' {
            in_string = true;
            i += 1;
            continue;
        }
        let starts_number =
            c.is_ascii_digit() || (c == b'-' && bytes.get(i + 1).is_some_and(u8::is_ascii_digit));
        let after_identifier =
            i > 0 && (bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_');
        if starts_number && !after_identifier {
            let start = i;
            i += 1;
            while i < bytes.len()
                && (bytes[i].is_ascii_digit()
                    || matches!(bytes[i], b'.' | b'e' | b'E' | b'+' | b'-'))
            {
                i += 1;
            }
            out.push(start..i);
            continue;
        }
        i += 1;
    }
    out
}

/// Every themed surface a frame can draw: the fixture's forms and lists, a dock with its tab
/// strips, an open context menu and a shown tooltip.
fn probe(ui: &mut Ui, scene: &mut Scene) {
    scene.fixture.render(1, ui);
    scene.dock.record(ui);
    ContextMenu::for_id(menu_id()).show(ui, |ui, popup| {
        MenuItem::new("Copy")
            .shortcut(Shortcut::ctrl('C'))
            .show(ui, popup);
        MenuSeparator::new().show(ui);
        MenuItem::new("Paste").show(ui, popup);
    });
    let hovered = ResponseSnapshot {
        id: WidgetId::from_hash("probe-tip-trigger"),
        state: ResponseState {
            rect: Some(Rect::new(40.0, 40.0, 40.0, 24.0)),
            pointer_over: true,
            ..ResponseState::default()
        },
    };
    Tooltip::on(&hovered, "tip").delay(Duration::ZERO).show(ui);
    Spinner::new()
        .id(WidgetId::from_hash("probe-spinner"))
        .show(ui);
    ColorPicker::new(&mut scene.color)
        .id(WidgetId::from_hash("probe-picker"))
        .show(ui);
    Modal::new()
        .id(WidgetId::from_hash("probe-modal"))
        .show(ui, |_, _| {});
}

fn menu_id() -> WidgetId {
    WidgetId::from_hash("probe-menu")
}

#[derive(Debug, Default)]
struct Scene {
    fixture: FrameFixture,
    dock: DockFixture,
    color: RgbaF32,
}

/// One frame of [`probe`] under `theme`, with the menu opened before it. Real shaping, so a font
/// size reaches the shaper and the ink it measures.
fn render(theme: Theme) -> Harness {
    let mut h = Harness::new(UVec2::new(640, 480));
    h.ui().set_theme(theme);
    ContextMenu::open(h.ui(), menu_id(), Vec2::new(20.0, 20.0));
    let mut scene = Scene {
        color: RgbaF32::srgb(0.2, 0.4, 0.6),
        ..Scene::default()
    };
    h.frame(|ui| probe(ui, &mut scene));
    h
}

fn line_of(ron: &str, at: usize) -> &str {
    let start = ron[..at].rfind('\n').map_or(0, |i| i + 1);
    let end = ron[at..].find('\n').map_or(ron.len(), |i| at + i);
    ron[start..end].trim()
}

/// Every `SHARDS`-th number.
fn walk(shard: usize) {
    let base = default_theme_text();
    for span in numbers(&base).into_iter().skip(shard).step_by(SHARDS) {
        for suspect in SUSPECTS {
            let document = format!("{}{suspect}{}", &base[..span.start], &base[span.end..]);
            let Ok(theme) = ron::from_str::<Theme>(&document) else {
                continue;
            };
            let field = line_of(&base, span.start);
            let rendered = panic::catch_unwind(|| render(theme));
            assert!(
                rendered.is_ok(),
                "`{field}` set to {suspect} loads, and rendering it panics",
            );
        }
    }
}

/// Every number that loads at 8192 on its own set to 8192 at once: code adds theme lengths before
/// it packs them into f16 lanes, which one number at a time never reaches.
#[test]
fn every_number_at_the_length_bound_at_once_renders() {
    const BOUND: &str = "8192.0";
    let base = default_theme_text();
    let splice = |document: &mut String, span: &Range<usize>| {
        document.replace_range(span.clone(), BOUND);
    };
    let taken: Vec<Range<usize>> = numbers(&base)
        .into_iter()
        .filter(|span| {
            let mut document = base.clone();
            splice(&mut document, span);
            ron::from_str::<Theme>(&document).is_ok()
        })
        .collect();
    assert!(
        taken.len() > 50,
        "only {} numbers take {BOUND}",
        taken.len()
    );
    let mut document = base;
    for span in taken.iter().rev() {
        splice(&mut document, span);
    }
    let theme = ron::from_str::<Theme>(&document).expect("each number loads at the bound");
    let _ = render(theme);
}

/// The default theme's probe frame opens the menu and shows the tooltip, so the walk reaches
/// their theme values.
#[test]
fn the_probe_draws_the_overlays() {
    let mut h = render(Theme::default());
    for layer in [Layer::Popup, Layer::Tooltip] {
        assert!(
            !h.ui().tree(layer).shapes.records.is_empty(),
            "{layer:?} records nothing",
        );
    }
}

#[test]
fn the_walk_finds_the_theme_numbers() {
    let base = default_theme_text();
    let spans = numbers(&base);
    assert!(spans.len() > 100, "found only {} numbers", spans.len());
    assert!(
        spans
            .iter()
            .all(|span| base[span.clone()].parse::<f64>().is_ok())
    );
}

/// Fits the per-test budget with real shaping: about 0.3 s a shard, beside the 0.4 s a test
/// binary takes to start with its fonts.
const SHARDS: usize = 12;

#[test]
fn file_values_shard_0() {
    walk(0);
}

#[test]
fn file_values_shard_1() {
    walk(1);
}

#[test]
fn file_values_shard_2() {
    walk(2);
}

#[test]
fn file_values_shard_3() {
    walk(3);
}

#[test]
fn file_values_shard_4() {
    walk(4);
}

#[test]
fn file_values_shard_5() {
    walk(5);
}

#[test]
fn file_values_shard_6() {
    walk(6);
}

#[test]
fn file_values_shard_7() {
    walk(7);
}

#[test]
fn file_values_shard_8() {
    walk(8);
}

#[test]
fn file_values_shard_9() {
    walk(9);
}

#[test]
fn file_values_shard_10() {
    walk(10);
}

#[test]
fn file_values_shard_11() {
    walk(11);
}
