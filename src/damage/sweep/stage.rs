//! Where a sweep measures: a screen's content on a surface.

use crate::bench::scene::{BENCH_DPR, BENCH_SURFACE, Scene, SceneFrame};
use crate::damage::sweep::policy::FrameFacts;
use crate::damage::sweep::scenario::{REFERENCE, Scenario, Scope};
use crate::display::system_scale::SystemScale;
use crate::host::harness::Harness;
use crate::host::window_driver::present_facts::rows_per_mpx;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::paint::backdrop_filter::BackdropFilter;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::shape::Shape;
use crate::ui::Ui;
use crate::ui::bench::settle_look;
use crate::widget_core::configure::Configure;
use crate::widgets::panel::Panel;
use crate::widgets::text::Text;
use glam::{UVec2, Vec2};

/// What a frame repaints under its damage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Content {
    /// The bench's screen, a real page.
    Scene,
    /// A uniform grid of filled, labelled cells, so a pixel costs alike wherever damage falls.
    Dense,
    /// One fill, so what is left of a frame is what repainting costs by itself.
    Plain,
    /// [`Self::Dense`] under two frosted panes, whose captures a change under them repeats.
    Glass,
    /// The bench's screen under a toolbar, a sidebar and a popup of frosted glass: few rows, and
    /// much of a whole frame's cost in its panes.
    SceneGlass,
}

/// The panes of [`Content::SceneGlass`].
const SCENE_PANES: [Rect; 3] = [
    Rect::new(0.0, 0.0, 1280.0, 48.0),
    Rect::new(0.0, 48.0, 240.0, 672.0),
    Rect::new(500.0, 200.0, 320.0, 220.0),
];

/// How far a pane's footprint reaches past its rect, near what its frosted blur reads.
const PANE_REACH: f32 = 48.0;

const DENSE_ROWS: u32 = 36;
const DENSE_COLS: u32 = 32;

/// The glass panes on the reference window, far enough apart that one's change leaves the other.
const PANES: [Rect; 2] = [
    Rect::new(500.0, 200.0, 320.0, 220.0),
    Rect::new(40.0, 400.0, 240.0, 240.0),
];

fn dense(ui: &mut Ui) {
    Panel::vstack()
        .id_salt("sweep-dense")
        .gap(2.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            for row in 0..DENSE_ROWS {
                Panel::hstack()
                    .id_salt(("sweep-row", row))
                    .gap(2.0)
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui, |ui| {
                        for col in 0..DENSE_COLS {
                            let shade = 0.1 + 0.2 * ((row + col) % 4) as f32 / 4.0;
                            Panel::zstack()
                                .id_salt(("sweep-cell", row, col))
                                .size((Sizing::FILL, Sizing::FILL))
                                .chrome(Chrome::fill(RgbaF32::srgb(shade, shade, shade + 0.05)))
                                .show(ui, |ui| {
                                    Text::new("cell").id_salt("sweep-label").show(ui);
                                });
                        }
                    });
            }
        });
}

/// `rect` of the reference window on `surface`.
fn scaled(rect: Rect, surface: Rect) -> Rect {
    let scale = Vec2::new(surface.size.w, surface.size.h) / REFERENCE;
    Rect::new(
        rect.min.x * scale.x,
        rect.min.y * scale.y,
        rect.size.w * scale.x,
        rect.size.h * scale.y,
    )
}

/// A content on a surface, which a sweep's arms take turns on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Stage {
    pub(crate) name: &'static str,
    pub(crate) content: Content,
    pub(crate) physical: UVec2,
    pub(crate) scale: SystemScale,
}

impl Stage {
    /// The bench's screen on the bench surface, the stage whose answers count first.
    pub(crate) const SCENE: Self = Self {
        name: "scene",
        content: Content::Scene,
        physical: BENCH_SURFACE,
        scale: BENCH_DPR,
    };

    /// Every stage: the two whole screens, then one change at a time from the dense one: no
    /// content, a quarter of the pixels of the same window, more pixels of the same aspect, and
    /// panes; and panes over the bench's own screen.
    pub(crate) const ALL: [Self; 7] = [
        Self::SCENE,
        Self {
            name: "dense",
            content: Content::Dense,
            ..Self::SCENE
        },
        Self {
            name: "plain",
            content: Content::Plain,
            ..Self::SCENE
        },
        Self {
            name: "dense_1x",
            content: Content::Dense,
            physical: UVec2::new(1280, 720),
            scale: SystemScale::ONE,
        },
        Self {
            name: "dense_4k",
            content: Content::Dense,
            physical: UVec2::new(3840, 2160),
            scale: BENCH_DPR,
        },
        Self {
            name: "glass",
            content: Content::Glass,
            ..Self::SCENE
        },
        Self {
            name: "scene_glass",
            content: Content::SceneGlass,
            ..Self::SCENE
        },
    ];

    /// This stage's panes on the reference window.
    const fn panes(&self) -> &'static [Rect] {
        match self.content {
            Content::Scene | Content::Dense | Content::Plain => &[],
            Content::Glass => &PANES,
            Content::SceneGlass => &SCENE_PANES,
        }
    }

    /// The logical window of this stage's surface.
    pub(crate) fn surface(&self) -> Rect {
        let scale = self.scale.get();
        Rect::new(
            0.0,
            0.0,
            self.physical.x as f32 / scale,
            self.physical.y as f32 / scale,
        )
    }

    /// What a frame's decision reads on this stage, from a record of its content under the look
    /// its frames are timed in.
    pub(crate) fn facts<S: Scene>(&self) -> FrameFacts {
        let mut h = Harness::builder(self.physical)
            .system_scale(self.scale)
            .build();
        let mut scene = SceneFrame::<S>::default();
        h.frame(|ui| self.record(&mut scene, ui));
        settle_look(h.ui_mut(), 1);
        h.frame(|ui| self.record(&mut scene, ui));
        let frame_scene = h.frames().frame_scene();
        FrameFacts {
            surface: self.surface(),
            rows_per_mpx: rows_per_mpx(frame_scene.visible_row_count(), self.physical),
            pane_share: frame_scene.pane_share(),
        }
    }

    /// Whether this stage measures scenarios of `scope`.
    pub(crate) const fn runs(&self, scope: Scope) -> bool {
        match scope {
            Scope::Core => true,
            Scope::Primary => {
                matches!(self.content, Content::Scene | Content::Dense)
                    && self.physical.x == BENCH_SURFACE.x
            }
            Scope::Captured => matches!(self.content, Content::Scene),
        }
    }

    /// The raw rects of `scenario`'s frames on this stage: scaled to its window, and where a rect
    /// reaches a pane, within [`PANE_REACH`] of it, that pane's rect after them, as a damage walk
    /// pushes a stale pane's.
    pub(crate) fn raw_frames(&self, scenario: &Scenario) -> Vec<Vec<Rect>> {
        let surface = self.surface();
        scenario
            .frames
            .iter()
            .map(|frame| {
                let mut raw: Vec<Rect> = frame.iter().map(|r| scaled(*r, surface)).collect();
                for pane in self.panes() {
                    let pane = scaled(*pane, surface);
                    let footprint = pane.inflated(PANE_REACH);
                    if raw.iter().any(|r| r.intersects(footprint)) {
                        raw.push(pane);
                    }
                }
                raw
            })
            .collect()
    }

    /// Record this stage's content, the bench's screen being `scene`.
    pub(crate) fn record<S: Scene>(&self, scene: &mut SceneFrame<S>, ui: &mut Ui) {
        match self.content {
            Content::Scene => scene.record(ui),
            Content::Dense => dense(ui),
            Content::Plain => {
                Panel::zstack()
                    .id_salt("sweep-plain")
                    .size((Sizing::FILL, Sizing::FILL))
                    .chrome(Chrome::fill(RgbaF32::srgb(0.1, 0.1, 0.12)))
                    .show(ui, |_| {});
            }
            Content::Glass | Content::SceneGlass => {
                let surface = self.surface();
                let panes = self.panes();
                Panel::zstack()
                    .id_salt("sweep-glass")
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui, |ui| {
                        if self.content == Content::Glass {
                            dense(ui);
                        } else {
                            scene.record(ui);
                        }
                        Panel::canvas()
                            .id_salt("sweep-panes")
                            .size((Sizing::FILL, Sizing::FILL))
                            .show(ui, |ui| {
                                for pane in panes {
                                    ui.add_shape(
                                        Shape::rect()
                                            .at(scaled(*pane, surface))
                                            .fill(RgbaF32::new(1.0, 1.0, 1.0, 0.2))
                                            .backdrop_filter(BackdropFilter {
                                                saturation: 1.8,
                                                noise: 0.04,
                                                ..BackdropFilter::blur(16.0)
                                            }),
                                    );
                                }
                            });
                    });
            }
        }
    }
}
