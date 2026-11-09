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
- genrate shape geometry in vertex shader to simplify fragment
- box mavterial?
- materials for other shapes
- Draw::Effect - do we need it if we have box?
- Brush::Linear add conveninet ctors

- table widget: sortable header, virtual rows over a fixed row height
- reorderable list: a drag handle and Alt+arrows
- hover card at a point for content that is not a widget

- audit hot structs for compact opportunities
- audit platform dependent behavior

investigate:
Under reduced transparency

    - With Ground, the material reads the window's clear color in place of the blurred backdrop. The fill and the border then draw on top as usual, so the box paints opaque.
    - With Nothing, the material is dropped. The fill paints as a plain translucent fill, and the border stays.

- rename the renderer's internal "pane" to "backdrop" (AGENTS.md, "One word, one meaning for what is behind")
- measure one glass shader with its spectrum branch on V3D (`gpu/backdrop`, "One glass")
