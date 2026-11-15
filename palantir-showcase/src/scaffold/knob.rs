//! [`Knob`]: one labelled slider of a stage.

use std::ops::RangeInclusive;

use palantir::{Configure, Panel, Sizing, Slider, Text, Ui, VAlign, fmt};

use crate::scaffold::kit::Kit;
use crate::scaffold::tokens::M;

/// One knob of a stage: a label, a slider over `range` in `step`s, and the value in `unit`.
#[derive(Debug)]
pub(crate) struct Knob {
    pub(crate) label: &'static str,
    pub(crate) range: RangeInclusive<f64>,
    pub(crate) step: f64,
    /// Printed after the value, which shows with as many decimals as the step has.
    pub(crate) unit: &'static str,
}

impl Knob {
    #[track_caller]
    pub(crate) fn show(self, ui: &mut Ui, k: Kit, value: &mut f32) {
        let decimals = self.decimals();
        Panel::hstack()
            .auto_id()
            .gap(M.space.sm)
            .child_align(VAlign::Center)
            .show(ui, |ui| {
                Text::new(self.label)
                    .style(&k.t.caption())
                    .min_size((KNOB_LABEL_W, 0.0))
                    .show(ui);
                Slider::new(&mut *value, self.range)
                    .step(self.step)
                    .size((Sizing::fixed(KNOB_W), Sizing::HUG))
                    .show(ui);
                let text = fmt!(ui, "{:.*}{}", decimals, *value, self.unit);
                Text::new(text)
                    .style(&k.t.mono(k.t.ink))
                    .min_size((KNOB_VALUE_W, 0.0))
                    .show(ui);
            });
    }

    /// The decimals the step has, so the value never shows a digit the slider cannot move: the
    /// fewest that make the step whole. The tolerance is f64's error in such a step, under 1e-15
    /// of it, well inside 1e-9.
    pub(crate) fn decimals(&self) -> usize {
        (0_u8..=3)
            .find(|&d| {
                let scaled = self.step * 10_f64.powi(i32::from(d));
                (scaled - scaled.round()).abs() < 1e-9
            })
            .map_or(3, usize::from)
    }
}

const KNOB_LABEL_W: f32 = 64.0;
const KNOB_W: f32 = 140.0;
const KNOB_VALUE_W: f32 = 56.0;
