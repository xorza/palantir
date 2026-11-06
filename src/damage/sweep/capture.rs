//! What a screen damages when a person uses it: a scripted session, each frame's raw rects kept, the
//! frequent ones run as scenarios, so a mean regret weighs what happens by how often it happens.

use crate::bench::scene::{BENCH_DPR, BENCH_SURFACE, Scene};
use crate::damage::sweep::scenario::{Scenario, Scope};
use crate::host::harness::Harness;
use crate::input::input_event::InputEvent;
use crate::primitives::geometry::rect::Rect;
use crate::ui::frame_report::FramePaint;
use glam::Vec2;
use std::collections::BTreeMap;
use std::time::Duration;

/// One display frame at 60 Hz, which moves the screen's animations as a window's frames do.
const FRAME_DT: Duration = Duration::from_nanos(16_666_667);
/// The most distinct frames a session keeps as scenarios: each costs its arms' turns.
const KEPT_MAX: usize = 16;

/// A session's most frequent frames, and how much of it they hold.
#[derive(Debug)]
pub(super) struct Captured {
    pub(super) scenarios: Vec<Scenario>,
    /// Painted frames of the session.
    pub(super) frame_count: u32,
    /// Distinct rect lists among them.
    pub(super) distinct_count: usize,
    /// Painted frames [`Self::scenarios`] stand for.
    pub(super) kept_frame_count: u32,
}

/// A rect list's bits, which compare and order exactly.
type RectBits = Vec<[u32; 4]>;

/// How many frames damaged one set of rects, and the order the first of them pushed the rects in.
#[derive(Debug)]
pub(super) struct Seen {
    count: u32,
    first: RectBits,
}

/// The session's frames by their rects in sorted order: which widget a fade pushes first is no
/// other frame.
pub(super) type SeenFrames = BTreeMap<RectBits, Seen>;

/// Count a frame of `rects`.
pub(super) fn note(seen: &mut SeenFrames, rects: RectBits) {
    let mut sorted = rects.clone();
    sorted.sort_unstable();
    seen.entry(sorted)
        .or_insert(Seen {
            count: 0,
            first: rects,
        })
        .count += 1;
}

/// Runs a scripted session and counts its frames' rect lists.
#[derive(Debug)]
struct Session<'a, S> {
    h: Harness,
    scene: &'a mut S,
    seen: SeenFrames,
}

impl<S: Scene> Session<'_, S> {
    fn step(&mut self) {
        self.h.advance(FRAME_DT);
        let scene = &mut *self.scene;
        let report = self.h.frame(|ui| scene.record(ui));
        if report.paint() == FramePaint::Skip {
            return;
        }
        let surface = self.h.ui().display().logical_rect();
        let mut key: RectBits = Vec::new();
        for r in self.h.frames_mut().damage.raw_rects() {
            let clipped = r.clamp_to(surface);
            let bits =
                [clipped.min.x, clipped.min.y, clipped.size.w, clipped.size.h].map(f32::to_bits);
            if !clipped.is_paint_empty() && !key.contains(&bits) {
                key.push(bits);
            }
        }
        if !key.is_empty() {
            note(&mut self.seen, key);
        }
    }

    fn steps(&mut self, frames: u32) {
        for _ in 0..frames {
            self.step();
        }
    }

    /// The pointer across the window in a raster, each widget under it fading in and out.
    fn hover_across(&mut self) {
        let window = self.h.ui().display().logical_rect();
        let mut y = 12.0;
        while y < window.size.h {
            let mut x = 24.0;
            while x < window.size.w {
                self.h.on_input(InputEvent::PointerMoved(Vec2::new(x, y)));
                self.steps(3);
                x += 96.0;
            }
            y += 48.0;
        }
    }

    /// A click on each of the screen's targets, its press and release fading through.
    fn click_targets(&mut self) {
        for target in self.scene.hover_targets() {
            self.h.click(target);
            self.steps(12);
            self.h.advance_past_double_click();
        }
    }

    /// Typing into the field, one key a few frames, and its caret blinking after.
    fn type_in_field(&mut self) {
        self.h.click(self.scene.field());
        self.steps(4);
        for key in "the quick brown fox".chars() {
            let mut buf = [0; 4];
            self.h.type_text(key.encode_utf8(&mut buf));
            self.steps(3);
        }
        self.steps(120);
    }

    /// The wheel down and back up over the window's middle.
    fn scroll_middle(&mut self) {
        let window = self.h.ui().display().logical_rect();
        let middle = window.min + Vec2::new(window.size.w, window.size.h) / 2.0;
        for delta in [-24.0, 24.0] {
            for _ in 0..30 {
                self.h.scroll_pixels(middle, Vec2::new(0.0, delta));
                self.step();
            }
        }
    }
}

/// The frames a scripted session of `scene` on `h` damages. Its first frames, which paint whole for
/// a new window, are not counted.
pub(super) fn session<S: Scene>(h: Harness, scene: &mut S) -> Captured {
    let mut session = Session {
        h,
        scene,
        seen: SeenFrames::new(),
    };
    session.steps(4);
    session.seen.clear();
    session.hover_across();
    session.click_targets();
    session.type_in_field();
    session.scroll_middle();
    kept(session.seen)
}

/// What a session over a new `S` on the bench surface damages.
pub(super) fn capture<S: Scene>() -> Captured {
    let harness = Harness::builder(BENCH_SURFACE)
        .system_scale(BENCH_DPR)
        .build();
    session(harness, &mut S::default())
}

/// `rects`' name, from what they cover, so a scenario reads alike on every host and run however
/// its count moved: their bounds and how many there are.
fn captured_name(rects: &[Rect]) -> String {
    let bounds = rects
        .iter()
        .copied()
        .reduce(Rect::union)
        .expect("a captured frame holds a rect");
    format!(
        "captured/{:.0}_{:.0}_{:.0}x{:.0}_{}r",
        bounds.min.x,
        bounds.min.y,
        bounds.size.w,
        bounds.size.h,
        rects.len()
    )
}

/// `seen`'s most frequent frames as scenarios, the most frequent first, a tie by name so two runs
/// read alike. Two frames of one name take `#n` by their rects, not their counts.
pub(super) fn kept(seen: SeenFrames) -> Captured {
    let frame_count = seen.values().map(|frame| frame.count).sum();
    let distinct_count = seen.len();
    let mut lists: Vec<(RectBits, Seen)> = seen.into_iter().collect();
    lists.sort_by(|a, b| b.1.count.cmp(&a.1.count).then_with(|| a.0.cmp(&b.0)));
    lists.truncate(KEPT_MAX);
    lists.sort_by(|a, b| a.0.cmp(&b.0));
    let kept_frame_count = lists.iter().map(|(_, frame)| frame.count).sum();
    let mut scenarios: Vec<Scenario> = Vec::with_capacity(lists.len());
    for (_, frame) in lists {
        let rects: Vec<Rect> = frame
            .first
            .iter()
            .map(|bits| {
                let [x, y, w, h] = bits.map(f32::from_bits);
                Rect::new(x, y, w, h)
            })
            .collect();
        let base = captured_name(&rects);
        let same = scenarios
            .iter()
            .filter(|s| s.name == base || s.name.starts_with(&format!("{base} #")))
            .count();
        let name = if same == 0 {
            base
        } else {
            format!("{base} #{}", same + 1)
        };
        scenarios.push(Scenario {
            name,
            frames: vec![rects],
            weight: frame.count,
            scope: Scope::Captured,
        });
    }
    scenarios.sort_by(|a, b| b.weight.cmp(&a.weight).then_with(|| a.name.cmp(&b.name)));
    Captured {
        scenarios,
        frame_count,
        distinct_count,
        kept_frame_count,
    }
}
