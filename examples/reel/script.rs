//! A slide's choreography: the steps a person and the camera take, run one after another on the
//! frame clock.

use std::time::Duration;

use palantir::glam::Vec2;
use palantir::{Harness, PointerButton, Ui, WidgetId};

use crate::deck::Deck;
use crate::motion::{self, Glide, View};

/// One thing that happens, in order: each step starts when the one before ends.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Step {
    /// The camera glides to `to` over `length`.
    Glide { to: View, length: Duration },
    /// The cursor reaches for `target`, in the time Fitts' law gives its distance and size.
    Move(Target),
    /// A press and a release where the cursor is.
    Click,
    /// A press where the cursor is, a reach to `target`, and a release there.
    Drag(Target),
    /// Text typed a character every [`TYPE_STEP`].
    Type(&'static str),
    /// A change the slide makes to itself or the window, as a person's setting would.
    Act(fn(&mut Deck, &mut Ui)),
    /// Nothing for this long.
    Hold(Duration),
}

/// Where a reach ends.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Target {
    /// The center of a widget, where the last frame laid it out.
    Widget(WidgetId),
    /// A point of a widget at a share of its width and height.
    WidgetAt(WidgetId, Vec2),
}

/// A typist's calm pace: 70 ms a character, about 170 words a minute.
pub(crate) const TYPE_STEP: Duration = Duration::from_millis(70);

/// A click's press, held as a finger holds it.
const PRESS: Duration = Duration::from_millis(90);

/// The pause after a click before the next step, so its ring is seen.
const AFTER_CLICK: Duration = Duration::from_millis(350);

/// Two presses closer than this in time and space read as a double click: palantir's own
/// window, 500 ms, and a 4 px slop.
const DOUBLE_CLICK: Duration = Duration::from_millis(500);
const DOUBLE_CLICK_SLOP: f32 = 4.0;

/// Fitts' law, `a + b · log2(D / W + 1)`, with MacKenzie's mouse constants (a ≈ 0.2 s,
/// b ≈ 0.1 s a bit; "Fitts' law as a research and design tool", 1992), slowed by [`CALM`] so a
/// viewer follows the reach.
fn reach_time(distance: f32, width: f32) -> Duration {
    const CALM: f64 = 1.8;
    let bits = (f64::from(distance) / f64::from(width.max(8.0)) + 1.0).log2();
    Duration::from_secs_f64((0.2 + 0.1 * bits) * CALM)
}

/// The cursor's reach in flight.
#[derive(Clone, Copy, Debug)]
struct Reach {
    from: Vec2,
    to: Vec2,
    start: Duration,
    length: Duration,
}

impl Reach {
    /// Where it is at `now`: along [`motion::min_jerk`], bowed sideways by an eighth of its
    /// length at the middle, as a wrist's arc bends a straight reach.
    fn at(&self, now: Duration) -> Vec2 {
        let t = now.saturating_sub(self.start).as_secs_f64() / self.length.as_secs_f64().max(1e-9);
        let s = motion::min_jerk(t) as f32;
        let line = self.to - self.from;
        let bow = line.perp() * (0.125 * (std::f32::consts::PI * s).sin());
        self.from + line * s + bow
    }
}

/// The camera's glide in flight.
#[derive(Clone, Copy, Debug)]
struct CameraRun {
    glide: Glide,
    start: Duration,
    length: Duration,
}

/// Runs a list of steps on a harness, frame by frame. Everything it does is a function of the
/// frame times it is given, so two runs feed the same input.
#[derive(Debug)]
pub(crate) struct Runner {
    steps: Vec<Step>,
    next: usize,
    /// When the step running now ends; the next starts there.
    step_end: Duration,
    cursor: Vec2,
    reach: Option<Reach>,
    drag_release: bool,
    camera: View,
    camera_run: Option<CameraRun>,
    typing: Option<(&'static str, usize)>,
    /// When a click's press is released.
    release_at: Option<Duration>,
    last_press: Option<(Duration, Vec2)>,
}

impl Runner {
    pub(crate) const fn new(steps: Vec<Step>, cursor: Vec2, camera: View) -> Self {
        Self {
            steps,
            next: 0,
            step_end: Duration::ZERO,
            cursor,
            reach: None,
            drag_release: false,
            camera,
            camera_run: None,
            typing: None,
            release_at: None,
            last_press: None,
        }
    }

    /// When the last step ended, once every step has run.
    pub(crate) fn finished(&self) -> Option<Duration> {
        (self.next == self.steps.len() && self.idle()).then_some(self.step_end)
    }

    pub(crate) const fn cursor(&self) -> Vec2 {
        self.cursor
    }

    /// The last press, when and where, for the click's ring.
    pub(crate) const fn last_click(&self) -> Option<(Duration, Vec2)> {
        self.last_press
    }

    pub(crate) const fn camera(&self) -> View {
        self.camera
    }

    /// Feed `h` what happens up to `now`: steps that start by then, the cursor's place and the
    /// camera's view at `now`.
    pub(crate) fn feed(&mut self, now: Duration, h: &mut Harness, deck: &mut Deck) {
        loop {
            if let Some(at) = self.release_at
                && at <= now
            {
                self.release_at = None;
                h.release(PointerButton::Left);
                continue;
            }
            if let Some((text, done)) = self.typing {
                let due = self
                    .step_end
                    .saturating_sub(TYPE_STEP * (text.chars().count() - done) as u32);
                if due <= now {
                    let c = text.chars().nth(done).expect("a character left");
                    let mut buf = [0; 4];
                    h.type_text(c.encode_utf8(&mut buf));
                    self.typing = (done + 1 < text.chars().count()).then_some((text, done + 1));
                    continue;
                }
            }
            if let Some(reach) = self.reach
                && reach.start + reach.length <= now
            {
                self.cursor = reach.to;
                h.hover(self.cursor);
                self.reach = None;
                if std::mem::take(&mut self.drag_release) {
                    h.release(PointerButton::Left);
                }
                continue;
            }
            if let Some(run) = self.camera_run
                && run.start + run.length <= now
            {
                self.camera = run.glide.at(1.0);
                self.camera_run = None;
                continue;
            }
            if self.step_end <= now && self.next < self.steps.len() && self.idle() {
                let step = self.steps[self.next];
                self.next += 1;
                self.begin(step, self.step_end, h, deck);
                continue;
            }
            break;
        }
        if let Some(reach) = self.reach {
            self.cursor = reach.at(now);
        }
        if let Some(run) = self.camera_run {
            let t = now.saturating_sub(run.start).as_secs_f64() / run.length.as_secs_f64();
            self.camera = run.glide.at(t);
        }
        h.hover(self.cursor);
    }

    /// No step still runs: the next may start.
    const fn idle(&self) -> bool {
        self.release_at.is_none()
            && self.typing.is_none()
            && self.reach.is_none()
            && self.camera_run.is_none()
    }

    fn begin(&mut self, step: Step, start: Duration, h: &mut Harness, deck: &mut Deck) {
        match step {
            Step::Glide { to, length } => {
                self.camera_run = Some(CameraRun {
                    glide: Glide::new(self.camera, to),
                    start,
                    length,
                });
                self.step_end = start + length;
            }
            Step::Move(target) => self.reach_for(target, start, h),
            Step::Drag(target) => {
                self.press(start, h);
                self.reach_for(target, start + PRESS, h);
                self.drag_release = true;
            }
            Step::Click => {
                self.press(start, h);
                self.release_at = Some(start + PRESS);
                self.step_end = start + PRESS + AFTER_CLICK;
            }
            Step::Type(text) => {
                self.typing = Some((text, 0));
                self.step_end = start + TYPE_STEP * text.chars().count() as u32;
            }
            Step::Act(act) => {
                act(deck, h.ui_mut());
                self.step_end = start;
            }
            Step::Hold(length) => self.step_end = start + length,
        }
    }

    fn reach_for(&mut self, target: Target, start: Duration, h: &Harness) {
        let (to, width) = match target {
            Target::Widget(id) => {
                let rect = h.rect(id).unwrap_or_else(|| panic!("{id:?} is laid out"));
                (rect.center(), rect.size.w.min(rect.size.h))
            }
            Target::WidgetAt(id, share) => {
                let rect = h.rect(id).unwrap_or_else(|| panic!("{id:?} is laid out"));
                (rect.min + Vec2::from(rect.size) * share, 16.0)
            }
        };
        let length = reach_time(self.cursor.distance(to), width);
        self.reach = Some(Reach {
            from: self.cursor,
            to,
            start,
            length,
        });
        self.step_end = start + length;
    }

    /// Press where the cursor is, refusing a second press near the last one inside the
    /// double-click interval: a reel that reads as a double click where it means two clicks would
    /// show a selection it never meant.
    fn press(&mut self, at: Duration, h: &mut Harness) {
        if let Some((last, place)) = self.last_press
            && at.saturating_sub(last) < DOUBLE_CLICK
            && place.distance(self.cursor) < DOUBLE_CLICK_SLOP
        {
            panic!(
                "two clicks at {:?} {:?} apart read as a double click",
                self.cursor,
                at - last
            );
        }
        self.last_press = Some((at, self.cursor));
        h.press(self.cursor, PointerButton::Left);
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::script::reach_time;

    /// A reach of 600 px to a 40 px target carries log2(16) = 4 bits: (0.2 + 0.1 · 4) · 1.8 =
    /// 1.08 s; one twice as far adds about a bit.
    #[test]
    fn a_reach_takes_the_time_fitts_law_gives() {
        assert_eq!(
            reach_time(600.0, 40.0),
            Duration::from_secs_f64((0.2 + 0.1 * 4.0) * 1.8)
        );
        assert!(reach_time(1200.0, 40.0) > reach_time(600.0, 40.0));
    }
}
