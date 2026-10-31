//! Layout mechanics: Sizing, child alignment with per-child override, Justify, padding/margin, gap, Visibility. The chips are demo content.

#![expect(
    clippy::cast_sign_loss,
    reason = "the showcase casts non-negative sizes, ids and color channels"
)]

use crate::scaffold::api;
use crate::support;
use crate::support::{Col, Kit, Knob, Section};
use palantir::{
    Align, Block, ComboBox, Configure, HAlign, Justify, Metrics, Panel, RgbaF32, Sizing, Spacing,
    Text, Ui, VAlign, Visibility, WidgetId,
};

const M: Metrics = Metrics::DEFAULT;

pub(crate) fn build(ui: &mut Ui, k: Kit) {
    playground(ui, k);
    support::cols(ui, |ui, col| match col {
        Col::Left => {
            sizing(ui, k);
            justify(ui, k);
            visibility(ui, k);
        }
        Col::Right => {
            alignment(ui, k);
            spacing(ui, k);
            gap(ui, k);
        }
    });
}

fn sizing(ui: &mut Ui, k: Kit) {
    Section::new("Sizing")
        .apis(&[api!(Sizing::fixed), api!(Sizing::HUG), api!(Sizing::fill)])
        .show(ui, k, |ui| {
            support::note(
                ui,
                k,
                "Fixed takes exact pixels, Hug takes its content's size — here only padding \
                 of 20 and 40 — and Fill shares the leftover by weight, 1 : 2 : 1.",
            );
            support::row(ui, |ui| {
                for (i, w) in [50.0, 100.0, 200.0].into_iter().enumerate() {
                    support::swatch(k.swatch.orange)
                        .id_salt(("fx", i))
                        .size((Sizing::fixed(w), Sizing::fixed(32.0)))
                        .show(ui);
                }
            });
            support::row(ui, |ui| {
                for (i, pad) in [20.0, 40.0].into_iter().enumerate() {
                    Block::new()
                        .id_salt(i)
                        .size((Sizing::HUG, Sizing::fixed(32.0)))
                        .padding((pad, 0.0, pad, 0.0))
                        .chrome(support::swatch_chrome(k.swatch.lime))
                        .show(ui);
                }
            });
            support::row(ui, |ui| {
                for (i, weight) in [1.0, 2.0, 1.0].into_iter().enumerate() {
                    support::swatch(k.swatch.teal)
                        .id_salt(("fill", i))
                        .size((Sizing::fill(weight), Sizing::fixed(32.0)))
                        .show(ui);
                }
            });
        });
}

fn justify(ui: &mut Ui, k: Kit) {
    Section::new("Justify")
        .apis(&[api!(Panel::justify)])
        .show(ui, k, |ui| {
            support::note(
                ui,
                k,
                "Where a stack puts its leftover room along its axis: Start, Center, End, \
                 SpaceBetween and SpaceAround, top to bottom.",
            );
            for (id, j) in [
                ("j-start", Justify::Start),
                ("j-center", Justify::Center),
                ("j-end", Justify::End),
                ("j-between", Justify::SpaceBetween),
                ("j-around", Justify::SpaceAround),
            ] {
                Panel::hstack()
                    .id_salt(id)
                    .size((Sizing::FILL, Sizing::fixed(32.0)))
                    .padding((6.0, 4.0, 6.0, 4.0))
                    .justify(j)
                    .chrome(k.t.card())
                    .show(ui, |ui| {
                        for i in 0..3 {
                            support::swatch(k.swatch.teal)
                                .id_salt((id, i))
                                .size((Sizing::fixed(36.0), Sizing::fixed(22.0)))
                                .show(ui);
                        }
                    });
            }
        });
}

fn visibility(ui: &mut Ui, k: Kit) {
    Section::new("Visibility")
        .apis(&[api!(Block::visibility)])
        .show(ui, k, |ui| {
            support::note(
                ui,
                k,
                "The orange chip is Visible, then Hidden — it keeps its slot — then \
                 Collapsed, which gives the slot up.",
            );
            for (id, vis) in [
                ("v-visible", Visibility::Visible),
                ("v-hidden", Visibility::Hidden),
                ("v-collapsed", Visibility::Collapsed),
            ] {
                Panel::hstack()
                    .id_salt(id)
                    .size((Sizing::FILL, Sizing::fixed(44.0)))
                    .padding(6.0)
                    .gap(12.0)
                    .chrome(k.t.card())
                    .show(ui, |ui| {
                        for (key, c, v) in [
                            ("a", k.swatch.teal, Visibility::Visible),
                            ("mid", k.swatch.orange, vis),
                            ("c", k.swatch.lime, Visibility::Visible),
                        ] {
                            Block::new()
                                .id_salt((id, key))
                                .size((Sizing::fixed(70.0), Sizing::fixed(28.0)))
                                .visibility(v)
                                .chrome(support::swatch_chrome(c))
                                .show(ui);
                        }
                    });
            }
        });
}

fn alignment(ui: &mut Ui, k: Kit) {
    Section::new("Alignment")
        .apis(&[api!(Panel::child_align), api!(Block::align)])
        .show(ui, k, |ui| {
            support::note(
                ui,
                k,
                "child_align places a container's children across its axis: centered in the \
                 row, right in the column. The orange chip overrides it with its own align.",
            );
            Panel::hstack()
                .size((Sizing::FILL, Sizing::fixed(96.0)))
                .gap(8.0)
                .padding(8.0)
                .child_align(Align::v(VAlign::Center))
                .chrome(k.t.card())
                .show(ui, |ui| {
                    aligned_chip(k.swatch.teal, Align::default())
                        .id_salt("a")
                        .show(ui);
                    aligned_chip(k.swatch.teal, Align::default())
                        .id_salt("b")
                        .show(ui);
                    aligned_chip(k.swatch.orange, Align::v(VAlign::Bottom))
                        .id_salt("c-self-bot")
                        .show(ui);
                    aligned_chip(k.swatch.teal, Align::default())
                        .id_salt("d")
                        .show(ui);
                });
            Panel::vstack()
                .size((Sizing::FILL, Sizing::fixed(110.0)))
                .gap(8.0)
                .padding(8.0)
                .child_align(Align::h(HAlign::Right))
                .chrome(k.t.card())
                .show(ui, |ui| {
                    aligned_chip(k.swatch.teal, Align::default())
                        .id_salt("a-vs")
                        .show(ui);
                    aligned_chip(k.swatch.orange, Align::h(HAlign::Left))
                        .id_salt("b-self-left")
                        .show(ui);
                    aligned_chip(k.swatch.teal, Align::default())
                        .id_salt("c-vs")
                        .show(ui);
                });
        });
}

fn spacing(ui: &mut Ui, k: Kit) {
    Section::new("Padding and margin")
        .apis(&[api!(type Spacing)])
        .show(ui, k, |ui| {
            support::note(
                ui,
                k,
                "Padding reserves room inside the parent, a margin shrinks the child's slot, \
                 and a negative margin overlaps the neighbor.",
            );
            Panel::hstack()
                .size((Sizing::FILL, Sizing::fixed(60.0)))
                .padding(20.0)
                .gap(8.0)
                .chrome(k.t.card())
                .show(ui, |ui| {
                    for i in 0..3 {
                        support::swatch(k.swatch.teal)
                            .id_salt(("p", i))
                            .size((Sizing::fixed(40.0), Sizing::FILL))
                            .show(ui);
                    }
                });
            Panel::hstack()
                .size((Sizing::FILL, Sizing::fixed(60.0)))
                .gap(8.0)
                .chrome(k.t.card())
                .show(ui, |ui| {
                    Block::new()
                        .size((Sizing::fixed(60.0), Sizing::fixed(40.0)))
                        .margin(8.0)
                        .chrome(support::swatch_chrome(k.swatch.teal))
                        .show(ui);
                    Block::new()
                        .size((Sizing::fixed(60.0), Sizing::fixed(40.0)))
                        .margin((16.0, 16.0, 0.0, 0.0))
                        .chrome(support::swatch_chrome(k.swatch.teal))
                        .show(ui);
                });
            // The orange box's left margin pulls it back 30 px so the two overlap.
            Panel::hstack()
                .size((Sizing::FILL, Sizing::fixed(60.0)))
                .padding(8.0)
                .chrome(k.t.card())
                .show(ui, |ui| {
                    support::swatch(k.swatch.teal)
                        .id_salt(("neg", "a"))
                        .size((Sizing::fixed(80.0), Sizing::fixed(40.0)))
                        .show(ui);
                    Block::new()
                        .size((Sizing::fixed(80.0), Sizing::fixed(40.0)))
                        .margin((-30.0, 0.0, 0.0, 0.0))
                        .chrome(support::swatch_chrome(k.swatch.orange))
                        .show(ui);
                });
        });
}

fn gap(ui: &mut Ui, k: Kit) {
    Section::new("Gap")
        .apis(&[api!(Panel::gap)])
        .show(ui, k, |ui| {
            support::note(ui, k, "0, 8 and 24 px between siblings.");
            for g in [0.0, 8.0, 24.0] {
                Panel::hstack()
                    .id_salt(("gap", g as u32))
                    .size((Sizing::FILL, Sizing::fixed(40.0)))
                    .padding(6.0)
                    .gap(g)
                    .chrome(k.t.card())
                    .show(ui, |ui| {
                        for i in 0..5 {
                            support::swatch(k.swatch.teal)
                                .id_salt(("gap-tile", g as u32, i))
                                .size((Sizing::fixed(32.0), Sizing::fixed(24.0)))
                                .show(ui);
                        }
                    });
            }
        });
}

fn aligned_chip(c: RgbaF32, align: Align) -> Block {
    support::swatch(c)
        .size((Sizing::fixed(56.0), Sizing::fixed(24.0)))
        .align(align)
}

/// The sizings a playground child picks among.
const SIZINGS: [&str; 3] = ["Fixed 80", "Hug", "Fill"];
const JUSTIFIES: [&str; 5] = ["Start", "Center", "End", "SpaceBetween", "SpaceAround"];

/// The playground's knobs: each child's sizing, and the row's padding, gap and justification.
#[derive(Debug)]
struct Playground {
    sizing: [usize; 3],
    padding: f64,
    gap: f64,
    justify: usize,
}

impl Default for Playground {
    fn default() -> Self {
        Self {
            sizing: [0, 1, 2],
            padding: 8.0,
            gap: 8.0,
            justify: 0,
        }
    }
}

/// Three children in one row, each with a sizing of its own, under the row's padding, gap and
/// justification: every knob of the sections below on one stage.
fn playground(ui: &mut Ui, k: Kit) {
    Section::new("Playground")
        .apis(&[api!(type Sizing), api!(Panel::justify)])
        .show(ui, k, |ui| {
            support::note(
                ui,
                k,
                "Pick each child's sizing and the row's padding, gap and justification. Fill \
                 children share what is left, so Justify moves only what Fill leaves unclaimed.",
            );
            let id = WidgetId::from_hash("showcase::sizing::playground");
            ui.with_state::<Playground, _>(id, |ui, p| {
                support::row(ui, |ui| {
                    for (i, (sizing, name)) in p.sizing.iter_mut().zip(["A", "B", "C"]).enumerate()
                    {
                        Text::new(name).style(&k.t.caption()).show(ui);
                        ComboBox::new(sizing, &SIZINGS)
                            .id_salt(("sizing", i))
                            .size((Sizing::fixed(110.0), Sizing::HUG))
                            .show(ui);
                    }
                    Text::new("justify").style(&k.t.caption()).show(ui);
                    ComboBox::new(&mut p.justify, &JUSTIFIES)
                        .id_salt("justify")
                        .size((Sizing::fixed(140.0), Sizing::HUG))
                        .show(ui);
                });
                support::row(ui, |ui| {
                    Knob {
                        label: "padding",
                        range: 0.0..=40.0,
                        step: 2.0,
                        unit: " px",
                    }
                    .show(ui, k, &mut p.padding);
                    Knob {
                        label: "gap",
                        range: 0.0..=40.0,
                        step: 2.0,
                        unit: " px",
                    }
                    .show(ui, k, &mut p.gap);
                });
                let justify = [
                    Justify::Start,
                    Justify::Center,
                    Justify::End,
                    Justify::SpaceBetween,
                    Justify::SpaceAround,
                ][p.justify];
                Panel::hstack()
                    .id_salt("playground row")
                    .size((Sizing::FILL, Sizing::HUG))
                    .padding(p.padding as f32)
                    .gap(p.gap as f32)
                    .justify(justify)
                    .chrome(k.t.card())
                    .show(ui, |ui| {
                        let colors = [k.swatch.teal, k.swatch.orange, k.swatch.violet];
                        for ((sizing, name), color) in
                            p.sizing.iter().zip(["A", "B", "C"]).zip(colors)
                        {
                            let width = match sizing {
                                0 => Sizing::fixed(80.0),
                                1 => Sizing::HUG,
                                _ => Sizing::FILL,
                            };
                            Panel::hstack()
                                .id_salt(name)
                                .size((width, Sizing::fixed(36.0)))
                                .padding((M.space.md, M.space.sm))
                                .child_align(Align::v(VAlign::Center))
                                .chrome(support::swatch_chrome(color))
                                .show(ui, |ui| {
                                    Text::new(name).style(&support::on_swatch_style()).show(ui);
                                });
                        }
                    });
            });
        });
}
