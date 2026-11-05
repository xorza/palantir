//! The showcase's look outside palantir's widgets: tokens read from the live palette, the type
//! ramp, chips and the rail.

pub(crate) mod chip;
pub(crate) mod rail;
pub(crate) mod tokens;

/// The checked spelling of an API a page shows: expands to the path as text after the compiler
/// resolves it, so a rename breaks the build.
///
/// - `api!(Ui::keep_open)`: a value path (function, method, associated const or unit variant).
/// - `api!(Ui::animate as fn(&mut Ui, ..) -> Animated<f32>)`: a generic function; the pointer type fixes
///   arguments a turbofish can't.
/// - `api!(type TabbedView)`: a type, trait or module item, for a method whose generic receiver
///   neither form can name.
macro_rules! api {
    ($head:ident $(:: $tail:ident)* as $sig:ty) => {{
        let _: $sig = $head $(:: $tail)*;
        concat!(stringify!($head) $(, "::", stringify!($tail))*)
    }};
    (type $head:ident $(:: $tail:ident)*) => {{
        #[expect(unused_imports, reason = "the import is the check")]
        use $head $(:: $tail)* as _;
        concat!(stringify!($head) $(, "::", stringify!($tail))*)
    }};
    ($head:ident $(:: $tail:ident)*) => {{
        let _ = $head $(:: $tail)*;
        concat!(stringify!($head) $(, "::", stringify!($tail))*)
    }};
}
pub(crate) use api;
