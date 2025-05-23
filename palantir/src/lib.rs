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
            .padding(10.0)
            .margin(5.0)
            .add_item(
                Label::from("Hello, world!")
                    .font_size(18)
                    .color(Colors::BLUE),
            )
            .add_item(
                Button::default()
                    .width(100.0)
                    .background_color(Colors::RED)
                    .set_item(
                        Label::from("Hello, world!")
                            .v_align(Align::Center)
                            .h_align(Align::Center)
                            .color(Colors::WHITE),
                    )
                    .onclick(|| {
                        println!("Button clicked!");
                    }),
            );
    }
}
