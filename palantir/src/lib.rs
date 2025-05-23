#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(unused_variables)]
#![allow(unreachable_code)]

mod elements;
mod fragment;
mod layout;
mod style;
mod utils;
mod view;

pub use elements::*;
pub use fragment::*;
pub use layout::*;
pub use style::*;
pub use utils::*;
pub use view::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_compiles() {
        VStack::default()
            .set_padding(10.0)
            .set_margin(5.0)
            .add_item(
                Label::from("Hello, world!")
                    .set_font_size(18)
                    .set_font_color(Colors::BLUE),
            )
            .add_item(
                Button::default()
                    .set_width(100.0)
                    .set_background_color(Colors::RED)
                    .set_item(
                        Label::from("Hello, world!")
                            .set_v_align(Align::Center)
                            .set_h_align(Align::Center)
                            .set_font_color(Colors::WHITE),
                    )
                    .onclick(|| {
                        println!("Button clicked!");
                    }),
            );
    }
}
