//! The theme file's own reference: what `balaur api` and the editor's asset
//! picker show for a `widget_theme`.

/// The doc string `balaur api` and the editor's asset picker show.
pub(crate) const ASSET_DOC: &str = r##"How each widget kind is drawn, one table per kind. `[colors]`, `[sizes]` and `[fonts]` hold the tokens every table may name, and `[roles.<name>]` is a look a widget picks with `role`. Seven source colours and four sizes derive every other token; a token the file states wins.

```toml
type = "widget_theme"            # a widget takes the theme of the nearest ancestor naming one

[colors]                         # the sources; bg_panel, text_muted, primary_fill, ... derive from them
background = "#151f2a"           # dark or light follows from it; `dark = true` overrides
foreground = "#e6e9ee"
primary = "#4287cc"              # also secondary, success, warning, danger
contrast = 0.05                  # how far apart the surfaces step
row_selected = "#2f6fb0"         # a picked row of a `list`, `tree` or `table`
row_selected_text = "#ffffff"    # and the ink on it
row_hover = "#ffffff12"          # what a row takes under the pointer; `row_active` while held
row_stripe = "#ffffff08"         # a table's every other row; "#00000000" hides it
table_header = "#ffffff08"       # a table's header plate
table_rule = "#00000000"         # the lines down its columns, hidden here
tree_guide = "#8a8a8a8c"         # the lines down a tree's indent

[sizes]                          # font_size, radius, control_height, stroke_width; the rest derive
font_size = 16                   # font_size_small, font_size_large and font_size_title follow
radius = 6                       # radius_small and radius_large follow

[fonts]                          # fonts by name; `default` is every text naming none, `icon` the glyphs
default = "fonts/Inter-Regular.ttf"  # a widget draws beside its caption, `heading` and `mono` egui's own
heading = "fonts/Inter-Semibold.ttf"

[icons]                          # what the icon font's glyphs mean
mirrored = ["chevron-left", "chevron-right", "arrow-left", "arrow-right"]  # turned round in a UI
                                 # mirrored for a language written right to left; the rest keep their way

[button]                         # one table per kind: [panel], [row], ...; a kind left out keeps the built-in look
fill = "primary_fill"
stroke = "border_default"
stroke_width = 1.0
corner_radius = "radius_large"   # a number, a size's name, or "full" for a pill
padding = 8.0                    # or per side, [left, top, right, bottom]
gap = 4.0
font_size = "font_size_large"
text_color = "text_on_primary"
icon_color = "text_on_primary"
font = "heading"                 # a name [fonts] gives a font, or a font itself
font_weight = 700
text_align = "center"            # start, center or end
cursor = "hand"                  # the pointer over it, a cursor word; a node's own wins
transition = 0.2                 # seconds every state eases over, as CSS's transition; here, never in a state table

[button.hover]                   # under the pointer; [button.active] while held, [button.focus] with
                                 # keyboard focus, [button.disabled] while off, [button.checked] while on
fill = "primary_fill_hover"

[button.active]
offset = [0, 0.5]                # drawn this far from its place, as CSS's translate; the layout stays
scale = [0.98, 0.98]             # drawn this many times its width and height about its middle, as CSS's scale

[button.focus]                   # with the pointer's table over it when both apply, as on the web
outline = "primary"              # the ring outside the edge, as CSS's outline; the edge stays
outline_width = 2                # its width, and outline_offset its gap from the edge, 2 and 2 unset
                                 # in another state's table, [button.checked], it rings that state

[spinner]                        # three quarters of a ring, turning, in the text's colour
height = 24                      # its side; a widget's own height wins
duration = 0.75                  # seconds a turn takes; stroke_width is the ring's, an eighth of the side unset
                                 # a `loading` button's spinner is this table's too

[switch]
height = 18                      # the track; width is 1.75 of it unset

[roles.calendar]                 # a role's least and greatest box; a widget's own min_width wins on its side
min_width = 300                  # max_width, min_height and max_height as well
max_width = 384

[roles.pick.checked]             # a state paints, and checked alone also takes font_weight: a write checks
font_weight = 600                # a widget, never the pointer, so it is measured at that weight

[roles.tab]
flex_direction = "column"        # the caption under the icon, as a tab bar's; `row` unset
icon_size = 24                   # the icon apart from the caption's size; a widget's own wins

[roles.bar]
stroke = "border_default"
border = [0, 1, 0, 0]            # the sides the edge is drawn on, left, top, right, bottom: here the top alone

[list]
row_height = "control_height"    # a row's pitch, a tree's and a table's too; a widget's own wins

[panel]
image = "art/panel.png"          # a nine-patch, sliced in its own pixels
slice = [8, 8, 8, 8]             # left, top, right, bottom

[table]                          # a row view is dressed like any other kind
fill = "bg_control"
stroke = "border_default"
corner_radius = 6.0
padding_x = 10.0                 # the air either side of a cell's text

[fold]                           # the header; [fold.checked] while open
arrow = "art/folded.png"         # the header's arrow picture; the ▸ and ▾ glyphs where none

[fold.body]                      # the frame around what an open fold shows
fill = "bg_panel"
padding = 8.0

[dialog]
backdrop = "#0000004c"           # what it dims the screen with, a sheet's too, as the web's ::backdrop

[menu.body]                      # the frame round what a menu opens, its shadows too: a dropdown, a sheet,
                                 # a context menu; egui's popup where none, and on a submenu
fill = "bg_panel"
padding = 8.0

[menu.narrow.body]               # a phone's, whole in its place: the sheet `[narrow] placement = "sheet"` opens
fill = "bg_panel"                # its foot square on the screen's
padding = 12.0

[roles.card]                     # any table may carry a gradient and a shadow
gradient = { kind = "linear", stops = ["primary_fill", "bg_panel"], angle = 90 }  # in place of fill; 0 runs left to right
# kind = "radial" with center = [0.5, 0.5] and radius = 0.5 runs out from the middle; at = [0, 0.3, 1] places three stops
shadows = [                      # blurred boxes under it, the first on top; [] for none
  { y = 2, blur = 8, spread = 0, color = "#00000040" },              # x too; a negative spread is smaller
  { inset = true, y = 0.5, spread = 0.5, color = "#ffffff0f" },      # inset: inside the edge, over the fill
]

[roles.eyebrow]                  # a small heading over a section
letter_spacing = 2.4             # extra space between glyphs, design pixels, as CSS's letter-spacing
text_transform = "uppercase"     # uppercase, lowercase, capitalize or none, as CSS's text-transform

[roles.danger]                   # what a widget with role = "danger" takes
fill = "danger_fill"
text_color = "text_on_danger"
```"##;
