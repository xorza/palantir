# Open issues

- A `Popup` or `PopupTrigger` with no `.chrome(..)` paints no surface: the stock theme's
  `panel_chrome` is `None`, so the popup's rows draw straight over the page under it (the
  showcase's focus page, Filters ▾).

