//! Every number a theme file can carry is either rejected on load or
//! safe to render.
//!
//! The walk finds the numbers in the serialized default theme rather than
//! in a hand list, so a field added without a validator fails here the day
//! it is added. Each literal is replaced, one at a time, with values a
//! hand-edited file can hold; a document that still loads is rendered
//! through a scene that draws every themed widget.

use crate::internals::frame_fixture::FrameFixture;
use crate::internals::harness::UiHarness;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::paint::color::RgbaF32;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widgets::color_picker::ColorPicker;
use crate::widgets::modal::Modal;
use crate::widgets::spinner::Spinner;
use crate::widgets::theme::Theme;
use crate::widgets::theme::tests::pretty;
use glam::UVec2;
use std::ops::Range;
use std::panic;

/// What a hand-edited file can put where a number belongs.
const SUSPECTS: [&str; 5] = ["NaN", "inf", "-inf", "-1.0", "0.0"];

/// Byte ranges of the numeric literals in `ron`, skipping anything inside
/// a string and any digit that is part of an identifier.
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

/// Every themed widget, so a loaded theme is read by each consumer.
fn probe(ui: &mut Ui, fixture: &mut FrameFixture, color: &mut RgbaF32) {
    fixture.render(1, ui);
    Spinner::new()
        .id(WidgetId::from_hash("probe-spinner"))
        .show(ui);
    ColorPicker::new(color)
        .id(WidgetId::from_hash("probe-picker"))
        .show(ui);
    Modal::new()
        .id(WidgetId::from_hash("probe-modal"))
        .show(ui, |_, _| {});
}

/// The line a byte offset sits on, to say which field a failure was.
fn line_of(ron: &str, at: usize) -> &str {
    let start = ron[..at].rfind('\n').map_or(0, |i| i + 1);
    let end = ron[at..].find('\n').map_or(ron.len(), |i| at + i);
    ron[start..end].trim()
}

/// Walk shard `shard` of [`SHARDS`]: every `SHARDS`-th number, so the
/// shards stay even however many numbers the theme holds.
fn walk(shard: usize) {
    let base = pretty(&Theme::default());
    for span in numbers(&base).into_iter().skip(shard).step_by(SHARDS) {
        for suspect in SUSPECTS {
            let document = format!("{}{suspect}{}", &base[..span.start], &base[span.end..]);
            let Ok(theme) = ron::from_str::<Theme>(&document) else {
                continue;
            };
            let field = line_of(&base, span.start);
            let rendered = panic::catch_unwind(|| {
                let mut h = UiHarness::new(UVec2::new(640, 480));
                h.ui().set_theme(theme);
                let mut fixture = FrameFixture::default();
                let mut color = RgbaF32::srgb(0.2, 0.4, 0.6);
                h.frame(|ui| probe(ui, &mut fixture, &mut color));
            });
            assert!(
                rendered.is_ok(),
                "`{field}` set to {suspect} loads, and rendering it panics",
            );
        }
    }
}

#[test]
fn the_walk_finds_the_theme_numbers() {
    let base = pretty(&Theme::default());
    let spans = numbers(&base);
    assert!(spans.len() > 100, "found only {} numbers", spans.len());
    assert!(
        spans
            .iter()
            .all(|span| base[span.clone()].parse::<f64>().is_ok())
    );
}

/// Split so each shard stays under the suite's per-test budget and the
/// shards run in parallel.
const SHARDS: usize = 8;

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
