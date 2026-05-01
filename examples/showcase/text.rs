use palantir::{Color, Configure, Frame, Panel, Sizing, Styled, Text, Ui};

const PARAGRAPH: &str = "The quick brown fox jumps over the lazy dog. \
    Pack my box with five dozen liquor jugs. \
    How vexingly quick daft zebras jump!";

pub fn build(ui: &mut Ui) {
    Panel::vstack()
        .gap(16.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            // Single-line label at the default 16 px size.
            Text::new("Heading — single line, hugs natural width")
                .size_px(20.0)
                .show(ui);

            // Wrapping paragraph in a fixed-width frame. Resize the window to
            // confirm the height grows when the paragraph stays the same width.
            Frame::with_id("para-wide")
                .size((Sizing::Fixed(360.0), Sizing::Hug))
                .fill(Color::rgb(0.16, 0.18, 0.22))
                .radius(6.0)
                .padding(12.0)
                .show(ui);
            Panel::vstack_with_id("para-wide-text")
                .size((Sizing::Fixed(360.0), Sizing::Hug))
                .padding(12.0)
                .show(ui, |ui| {
                    Text::new(PARAGRAPH).size_px(14.0).wrapping().show(ui);
                });

            // Same text in a much narrower slot — should wrap onto many lines.
            Panel::vstack_with_id("para-narrow")
                .size((Sizing::Fixed(140.0), Sizing::Hug))
                .padding(8.0)
                .show(ui, |ui| {
                    Text::new(PARAGRAPH).size_px(14.0).wrapping().show(ui);
                });

            // Single unbreakable word in a 40 px slot — overflows rather than
            // breaking inside the word (intrinsic_min floor).
            Panel::vstack_with_id("overflow")
                .size((Sizing::Fixed(40.0), Sizing::Hug))
                .padding(4.0)
                .show(ui, |ui| {
                    Text::new("supercalifragilistic")
                        .size_px(14.0)
                        .wrapping()
                        .show(ui);
                });
        });
}
