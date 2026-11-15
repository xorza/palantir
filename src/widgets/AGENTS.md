# Widgets

How a widget is built and behaves. The naming and argument rules of the top `AGENTS.md`, "Public
API design", hold here too.

## Shape of a widget

- **Lifecycle**: constructor, setters, `show(self, ui)` or `show(self, ui, body)`; a widget that
  also shows alone takes its body through `show_with`. `Configure` carries every node setting, and
  a widget does not repeat one.
- **Theme**: `style(s)` takes the slot, a second slot `<slot>_style`; an override of one axis is a
  named setter.
- **A setter each widget repeats is written out on each**, one name, argument and doc for one
  axis. Refused: a macro, which hides the API from a reader of the source; a mixin trait of
  default methods, which takes `const` off each setter and adds a trait each caller imports.
- **Node values a widget supplies** go through `default_*` in `show`, so a caller's setter wins.
- **State between frames** lives in the state map under the widget's id, driven from outside by
  associated functions that take `ui` and the id (`open`, `close`, `is_open`).
- **Responses**: `show` returns `Response`, or a `*Response` that holds `response` beside the
  frame's edges, none derefing to `Response`; a bound value returns `ValueResponse`, and a
  discrete pick commits at once.
- **A reveal aligns the nearest edge**, as CSSOM's `scrollIntoView` does at `nearest`
  ([CSSOM View](https://drafts.csswg.org/cssom-view/#element-scrolling-members)): a span in view
  or covering it moves nothing, else the edge it crosses aligns, or its other edge when it is
  longer than the view. One `reveal_pan` serves a scroll's `reveal`, its focus reveal, a text
  edit's caret and a tab strip's selected chip. Refused: always the start, which jumps a long
  span away from where the view was.
- **A value commits a change**, as HTML's `change` event fires: a release commits when it leaves
  the value apart from where its press found it, a key when it moved the value, so a click on the
  spot the value holds and a key at a bound commit nothing. A text edit commits its session
  instead, on Enter or the blur that ends it, changed or not, as a form's field submits what it
  holds. Refused: a commit on every release, an undo step that changes nothing.
- **A `Response` is owned**: `show` probes the widget's state once, with no borrow of the `Ui`, so
  one type serves a read, an overlay's anchor and a composite's gathering. Refused: a lazy
  `Response` over `&Ui`, which needs an owned twin of each response and a lifetime on each;
  `Response::tooltip`, which needs `&mut Ui` and names a widget above `widget_core`.
- **One `Response`, its reads inherent**: the id and the frame's state in one plain struct, built
  idle through `Response::idle(id)`, with no `Default`, as a response names its widget. Refused:
  an id beside a state the response derefs to, `Deref` being for smart pointers alone
  ([C-DEREF](https://rust-lang.github.io/api-guidelines/predictability.html#only-smart-pointers-implement-deref-c-deref)).
- **A wrapper finishes the widget it wraps through its public setters**. Widgets of the crate that
  cooperate do so through a `pub(crate)` protocol documented on the widget that offers it.
  Refused: publishing those hooks, a protocol no app needs that each refactor would break.

## Behavior

- **A button holds shapes and shows a selected state**, as `aria-pressed`. Refused: a container
  button; a toggle bound to `&mut bool`
  ([WAI-ARIA button](https://www.w3.org/WAI/ARIA/apg/patterns/button/)).
- **A field can edit a draft of its own.** Refused: a read-only field, which reaches typing, the
  edit menu, undo and the input method for one caller
  ([Qt `editingFinished`](https://doc.qt.io/qt-6/qlineedit.html#editingFinished),
  [GTK4 `EditableLabel`](https://docs.gtk.org/gtk4/class.EditableLabel.html)).
- **A popup raised at a point stays where it landed** until it stops fitting; one beside a rect
  is placed each frame
  ([`TrackPopupMenu`](https://learn.microsoft.com/windows/win32/api/winuser/nf-winuser-trackpopupmenu)).
- **A pick binds a value**, `&mut T` beside options that yield `T`, the picked row's value moved
  in, as egui's `selectable_value` hands each row its value
  ([egui](https://docs.rs/egui/latest/egui/struct.Ui.html#method.selectable_value)); `T` may be a
  reference, so a caller that holds a borrow into a table binds it and clones nothing, the reason
  iced's `pick_list` takes its selection as a `Borrow`
  ([iced](https://docs.rs/iced/latest/iced/widget/fn.pick_list.html)). A value no option equals
  shows as itself and is never written back. Refused: a slice of owned values, which makes a
  caller holding a reference clone it every frame; an index, which each caller maps back (egui's
  `show_index`). A pick over a list the widget itself shortens or reorders binds the index, as a
  value there names no row the widget keeps.
- **One pick from a few is a `RadioGroup`**, whatever it looks like, as SwiftUI's `pickerStyle`
  restyles one `Picker`: one Tab stop, an arrow moving focus and the pick together
  ([APG radio group](https://www.w3.org/WAI/ARIA/apg/patterns/radio/)). Refused: a free-standing
  radio button, which answers no arrow; a segmented control of its own.
- **One key map per role**: a slider takes the APG slider's keys
  ([APG slider](https://www.w3.org/WAI/ARIA/apg/patterns/slider/)), a drag value the spin
  button's, leaving Left and Right to the text
  ([APG spin button](https://www.w3.org/WAI/ARIA/apg/patterns/spinbutton/)).
- **A bound number is any of the crate's scalars**, a closed set and so an enum, where egui uses
  a `Numeric` trait; an `f32` is snapped in `f64`, then rounded once.
- **A step is a snap and the arrow unit**, from `min` or from zero, as HTML's step base
  ([HTML `step`](https://html.spec.whatwg.org/multipage/input.html#the-step-attribute)), applied
  where every write passes.
- **A disabled color surface lies under a veil** of the panel's color, as its data colors no look
  can dim, as libadwaita dims with `filter: opacity`
  ([libadwaita !307](https://gitlab.gnome.org/GNOME/libadwaita/-/merge_requests/307)). Refused: a
  subtree opacity in the renderer for one widget family. Cost: exact only over the panel's color.
