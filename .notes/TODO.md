- widget-authoring surface
- custom panels
- wgpu features and limits request public api
- mesh once registrations
- flexbox
- image handle with closure for reupload?
- build-in colors
- rich text edit
- md viewer
- optimal resolve strategies
- box material?
- Brush::Linear add conveninet ctors
- material for texts and strokes?
- consolidate gpu atlas

- table widget: sortable header, virtual rows over a fixed row height
- reorderable list: a drag handle and Alt+arrows
- hover card at a point for content that is not a widget

- audit platform dependent behavior

investigate:
Under reduced transparency

    - With Ground, the material reads the window's clear color in place of the blurred backdrop. The fill and the border then draw on top as usual, so the box paints opaque.
    - With Nothing, the material is dropped. The fill paints as a plain translucent fill, and the border stays.
