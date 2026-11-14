# Plan: the widget kinds a scene cannot hold yet

The `widget` component's kinds are in `crates/balaur_ui/src/vocabulary.rs`
(`WIDGET_KINDS`). These are the theme keys, containers and pickers still
missing, in the order the roadmap ships them.

## Theme icons and colours

1. **The pictures the other kinds draw.** A `widget_theme` entry takes
   `disabled` and `focus` tables beside `hover` and `active`, and a `fold`
   names its `arrow`. Still fixed: a `checkbox`'s mark, a `number_field`'s
   arrows, a `tabs` close, a `slider`'s grabber and a `dropdown`'s arrow.
   Each gets a picture key in its kind's theme table, read in
   `crates/balaur_ui/src/theme.rs`. Done: a theme naming each picture changes
   what its kind draws, with a test per key.
2. **Text colours.** Caret, selection, placeholder and text-shadow colours as
   theme keys. Done: a theme sets each and a `text_field` draws it.

## Containers

3. **`aspect`.** A child held at a ratio inside whatever rect it is given,
   Godot's `AspectRatioContainer`. Done: the child keeps its ratio as its
   parent resizes.
4. **`view`.** A camera's texture drawn as a widget the layout sizes, Godot's
   `SubViewportContainer`. Waits on the render target
   `PLAN-views-and-culling.md` adds.
5. **`graph`.** A pan and zoom canvas of nodes with ports, and links a drag
   connects, on `egui-snarl` at a release matching the workspace's egui. The
   kind implements `SnarlViewer` over its child nodes and keeps the graph in
   the scene's TOML, so the editor's undo and a replay see it. It lands
   before the script graph (`PLAN-authoring-without-code.md`) and the shader
   graph (`PLAN-shaders.md`), which both draw on it.

## Pickers and drag

6. **`file`.** Project files as a `list` over `fs.list` with a filter, so it
   sees the web's `MemoryFs` and an export pack; an OS file goes through
   `rfd`. Not `egui-file-dialog`, which browses only `std::fs` and shows the
   machine's drives.
7. **Drag and drop.** `drag` names the value a widget hands over, read off
   the node, and `drop` the method called with it, resolved as a `call` row
   is. Built on egui's own `DragAndDrop` payload seam; a `list` or `tree`
   reorder (`reorderable`) stays apart, since it never leaves its widget.
   Done: a row dragged from the Assets dock lands on the node tree.
