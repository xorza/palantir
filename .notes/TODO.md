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
- instead of stencil mask generate geometry, drop stentil at all
- flag whether to reduce fps on lowpower
- material for texts and strokes?
- remove image shape
- Draw::Effect - do we need it if we have box?

- table widget: sortable header, virtual rows over a fixed row height
- reorderable list: a drag handle and Alt+arrows
- hover card at a point for content that is not a widget

- audit hot structs for compact opportunities
- audit platform dependent behavior

investigate:
Under reduced transparency

    - With Ground, the material reads the window's clear color in place of the blurred backdrop. The fill and the border then draw on top as usual, so the box paints opaque.
    - With Nothing, the material is dropped. The fill paints as a plain translucent fill, and the border stays.

- measure one glass shader with its spectrum branch on V3D (`gpu/backdrop`, "One glass")
- measure the f32 corner points of a closed shape on V3D (`Quad` 64 → 100 B, step `3c010b1d1`)
- bench step `3c010b1d1` against its parent: `atomic/main_pass/gpu/*`, `integration/frame/gpu/*`
