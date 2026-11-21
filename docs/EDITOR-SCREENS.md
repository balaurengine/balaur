# The editor's screens

Every surface the editor draws: an ASCII mockup, the script that fills it, and
the screenshot that proves what it looks like. The review sheet: read a mockup,
open the PNG, write the complaint in §10.

The shell is a scene, `editor/scenes/main.toml`: each region is a node, and
each panel's node runs one script under `editor/scripts/panels/`, filled when
what it shows moved. `editor/scripts/layout.bs` computes every rect from the
room the screen leaves.

`scripts/uiaudit.sh` shoots whole shells, one a state, into `target/uiaudit/`;
`scripts/uiaudit.sh 03-script` shoots one. `scripts/views.sh` shoots one view
at a time: each dock panel cut out of the shell to its own PNG in
`target/views/`, plus two contact sheets, `_sides.png` and `_bottoms.png`.
Captures are offscreen at 1920 × 1080 device px (`OFFSCREEN_SIZE`) and
`ui_scale` 1.25, so the shell lays out at 1536 × 864 design px, above the
1240 px `COMPACT_WIDTH`. `26`, `27`, `29`, `30` and `38`–`41` change the docks,
the scale or the size on purpose. The screenshots are regenerated; the prose
is not.

---

## 1. The skeleton

A workspace re-fills the three docks and says which panels are open in each; a
tab's close mark takes one away and the mark menu at the head of the bar puts
it back.

```
┌──────────────────────────────────────────────────────────────────────────────┐
│ ● balaur  (Scene Script Animation Physics UI)        ▶ ⏸ ■   ⌕ command   ☾  │ 40
├───────────────┬──┬─────────────────────────────────────────┬─────────────────┤
│ Scene Outline │  │ ◇ main.toml  ‹›spinner.bs  ⚯events  ◫Split│ Inspector Import│
│               │t │─────────────────────────────────────────│ ◉ NodeName      │
│  World        │o │                                         │ ▾ SECTION ──────│
│   ├ Ground    │o │                                         │  label  [value] │
│   ├ Ball    ‹›│l │        viewport  /  code  /  events      │  label  [value] │
│   └ Player  ‹›│  │                                         │                 │
│     ├ Sprite  │r │                                         │ ▾ EVENTS ───────│
│     └ Hitbox  │a │                                         │  ● hook() →     │
│               │i │                                         │                 │
│               │l │                                         │                 │
├───────────────┴──┴─────────────────────────────────────────┤                 │
│ Output Problems Assets                                      │                 │
│ ─────────────────────────────────────────────────────────  │                 │
│ 0.2 s  project  scene key 'color' has no handler            │ [＋ Add compo.] │ 174
├─────────────────────────────────────────────────────────────────────────────-┤
│ ● editing  16 nodes · 4 scripts  ● Scripts warm          Scene · Select    │ 24
└──────────────────────────────────────────────────────────────────────────────┘
  236               44      central (fills)                    288
```

| Region | Size | Node, script | Resizable |
|---|---|---|---|
| top bar | 40 px | `Head`, `panels/topbar.bs` | no |
| status bar | 24 px | `Status`, `panels/status.bs` | no |
| left dock | 236 px, at least 180 | `LeftSheet`, `panels/dock.bs` | no |
| right dock | 288 px, at least 220 | `RightSheet`, `panels/dock.bs` | no |
| tool rail | 44 px | `RailSlot`, `panels/rail.bs` | no, hides with the viewport |
| bottom dock | 174 px, at least 84 | `Bottom`, `panels/dock.bs` | no |
| hooks list | 172 px | `Hooks`, `panels/hooks.bs` | no |
| stage | fills, at least 260 | `panels/stage.bs`, `panels/chips.bs` | no |
| split code pane | `split_width` | `panels/code.bs` | **yes**, the only one |

---

## 2. Workspaces

Five, `defs::workspaces()`. Switching resets the tool to Select and opens the
panels `defs::workspace_docks` names, in the docks it names them for. That is
all a workspace does to the docks: what it opens closes again from the tab,
and what it leaves out opens from the mark menu without changing workspace.
Selection is workspace-independent.

| | Scene | Script | Animation | Physics | UI |
|---|---|---|---|---|---|
| tool rail (`defs::tools`) | select move rotate scale pen tiles zoom | *(none; the rail hides)* | select move bone polygon key zoom | select move polygon zoom | select move zoom |
| viewport chips (`defs::chips`) | 3D · Perspective, Snap 8 px, Guides, Lights | none | Motion path, Snap 8 px | Show colliders, Sleep bodies | Safe area, 1920 × 1080 |
| left · bottom · right | scene outline · output problems assets · inspector import | scene outline · output problems docs debugger · inspector | scene outline · timeline statemachine output library · inspector | scene · output problems profiler · inspector | scene outline · output problems assets · inspector import |
| screenshot | `01-scene-3d`, `02-scene-2d` | `03-script` | `04-animate` | `05-physics` | `06-interface` |

The Scene workspace in other states: `08` is the light theme and `09` a game
playing in the viewport with its HUD. `22` is the same logo drawn plain and
through a material's shader, `23` a 3D skin on three bones, `34` a selected
spot light with its cone, and `35` the pen tool. `21` is the Animation
workspace with the bone tool.

---

## 3. Left dock: `panels/scene_tree.bs`, `panels/outline.bs`

The tree's header carries the search field and a facet chip row (3D, 2D,
Physics, Draws, Interface, Script); a chosen chip flattens the tree to the
nodes carrying a component with that tag. A row shows a lock or a hidden mark
where it has one, and a selected row that is not the active one reads bold.

```
┌──────────────────────────────┐ 236
│ Scene  Outline            ＋ │     tabs, and the add button
├──────────────────────────────┤
│ ▾ ◉ World                    │     depth 0, bold
│ ├─ ▣ Ground                  │     rails are mono ├─ └─ │ glyphs
│ ├─ ○ Ball                 ‹› │     trailing ‹› = has script
│ ├─ ◆ Spinner              ‹› │     selected = solid accent pill
│ ├─ ◐ Platform             ‹› │
│ ├─ ▾ ○ CrateA             ⧉  │     trailing ⧉ = prefab instance
│ │  ├─ ● Crate               │     an instance's children are dimmed
│ │  └─ ◆ Lid                 │
│   … scrolls …                │
└──────────────────────────────┘
```

---

## 4. Centre

### 4a. Viewport: `panels/stage.bs`, `panels/chips.bs` (`01`, `02`, `05`)

```
┌────────────────────────────────────────────────────────┐
│ ┌3D · Perspective┐ ┌Snap 8 px┐ ┌Guides┐ ┌Lights┐       │ chips
│                                                        │
│                    (the game's scene renders here)     │
│                                                        │
│ ┌x → y ↑ z ↓┐                        ┌ − │ 100 % │ ＋ ┐│ axis, zoom
└────────────────────────────────────────────────────────┘
```

Selection, gizmos, colliders, guides and the motion path are 3D lines drawn
by `gizmo.bs`, `gizmo2d.bs`, `overlays.bs`, `rig.bs` and `polygon.bs`, under
the `ViewportLines` node.

### 4b. Code: `panels/code.bs`, `panels/hooks.bs` (`03`, `24`)

```
┌────────────────────────────────────────────┬──────────────┐
│  1 // Spins in place; the `reverse` action │ HOOKS IN FILE│ 172
│  2                                         │ ┌──────────┐ │
│  4 pub class Spinner {                     │ │● init    │ │
│  5     pub speed: float = 2.0,             │ ├──────────┤ │
│  6     pub clockwise: bool = true,         │ │● reverse │ │
│    breakpoints, problems, warnings and the │ │● update  │ │
│    stopped line all live in the gutter     │ └──────────┘ │
│                                            │ ● unsaved·⌘S │
└────────────────────────────────────────────┴──────────────┘
```

One pane for `.bs` and `.wesl`; a shader swaps the hooks list for a `SHADER`
label and turns gutter clicks into value previews.

### 4c. Events: `events.bs`, `panels/events.bs` (`16`, `33`)

One row per `[[nodes.bindings.rows]]` entry on the selected node: an event
dropdown, a `when` field, an action, a target picked from the scene's nodes or
its variables, a value, and a delete. *Add row* and *Convert to script* sit in
the header; the scene's variables and the node's own script hooks are listed
below the rows.

### 4d. Split (`17`)

Code as a resizable right panel, viewport in what is left. The only resizable
region in the shell.

### 4e. Focus: `shell::toggle_focus` (`37`)

The code pane with the window to itself: `⇧⌘\`, or the chip beside Split.
The three docks fold and the split goes off; the file's hooks list
stays. Leaving puts back the layout focus folded. A panel opened under it,
from `⌘K`, the mark menu or a diagnostic, ends focus rather than being shut
again.

---

## 5. Bottom dock: `panels/dock.bs`

Fourteen panels, of eighteen in all; the workspace says which are open, and a
registered plugin's joins the bottom dock. Every tab carries a close mark, and
a dock emptied of tabs gives its room back to the stage.

```
┌────────────────────────────────────────────────────────────────────┐
│ Output Problems 3 Assets                                           │
│                                     [filter…] all warn error clear │
│ ────────────────────────────────────────────────────────────────── │
│ 0.2  s  project     scene key 'color' on 'Lid' has no handler      │ mono
└────────────────────────────────────────────────────────────────────┘
```

| Tab | Script | Content | Shot |
|---|---|---|---|
| Output | `panels/output.bs` | timestamp · tag · message, filtered by level and a query | `01` |
| Problems | `panels/problems.bs` | lint findings, errors first, each a clickable `file:line` | `10` |
| Assets | `panels/assets.bs` | cards over a `res://` breadcrumb; new folder, rename, delete | `11`, `25` |
| Timeline | `panels/timeline.bs` | keys, a scrubber, then one lane per track | `15` |
| State machine | `panels/statemachine.bs` | the animation state machine's states and transitions | — |
| Debugger | `panels/debugger.bs` | continue, over, into, out, the pause reason, frames and locals | `12` |
| Recordings | `panels/recordings.bs`, `panels/lanes.bs` | a row a recording, the verify switch, and the replay's lanes | `13` |
| Profiler | `panels/meter.bs` | the frame's cost, then per-script cost rows | `14` |
| Cost | `panels/meter.bs` | draws and triangles, then a bar per node | `31` |
| Library | `panels/library.bs` | material, lighting and template cards behind chips | `32` |
| Docs | `panels/docs.bs` | the script reference, module by module, from the running engine | — |
| Tiles | `panels/tiles.bs` | the tile set's palette, and the layers it paints into | — |
| Weights | `panels/weights.bs` | a polygon's bone weights | — |
| Bone map | `panels/bonemap.bs` | a rig's bones matched to a skeleton's | — |
| *plugin* | `panels/plugin.bs` | whatever the plugin's panel fills | `19` |

---

## 6. Right dock: `panels/inspector.bs`, `drawers.bs`

```
┌────────────────────────────────┐ 288
│ ◉  Spinner   MeshInstance3D    │ title, its type beside it
│ [ Search                     ] │
│ ▾ TRANSFORM ─────────────── ×  │ caret, title, enabled, ×
│ Position   [x 0.00][y 0.00][z] │
│ ⚠ Scale    [x 0.00][y 0.00][z] │ a warning: a mark on the label
│ ▸ Advanced · 3                 │ a group, shut until it is opened
│ ▾ SCRIPT ───────────────────── │
│ Source     [ scripts/x.bs pick]│
│ Clockwise  [x]                 │
│ Speed      [ 3.50      ] reset │ off its default: a chip on the row
│                                │
│ [      ＋ Add component      ] │ accent
└────────────────────────────────┘
```

A row's control follows its property's type: number field, vector of number
fields, checkbox, dropdown, swatch, pills, node path, asset path with its own
rows under it, or a text field. A `list` or `map` is a head that adds and an
entry's row that takes it out; a `record` or `variant` is a head and its
fields. Component sections come from `scene::component_schema`, so a plugin's
component gets a section for free. The transform is one of them:
`[nodes.transform]` in the file, `node.transform.position` in a script, and a
node that names none has none. A row is labelled from its property name
(`half_extents` reads "Half extents"). Its label carries the schema's
`description` as a tooltip, then what its mark says: a warning, or that it
changes what the prefab sets.

Only the rows on screen are nodes. An edit writes one property, and the change
log rewrites that row.

The Import tab (`panels/import.bs`) shows the file the Assets tab has
selected.

---

## 7. The mark menu: `menu.bs`

The mark at the head of the bar is the shell's one menu, opened with a left
click. It holds the command palette, Settings, the theme, Export and About,
each a command of `commands.bs` beside its key's chord, then every panel
with a tick beside the open ones, two to a line. Its last row puts the current
workspace's panels back. A folded side dock is a rail of the same marks: the
one that opens it, then one per panel it holds. Shot `36-menu`, whose `menu`
state draws the rows as a sheet, since no offscreen run can click a popup open.

---

## 8. Overlays and windows

| Surface | Script | Shot | State |
|---|---|---|---|
| Command palette | `palette.bs` | `07` | ⌘K, or `palette` |
| Input overlay | `panels/inputview.bs`: key chips, click ripples, drawn cursor | `18` | `input` |
| Plugin window | `plugins.bs`, `panels/plugin.bs` | `19` | `test:plugin_docks` |
| Showcase driver | `showcase.bs`: scripted input for the manual's clips | — | `show:<name>` |
| Font sheet | `shell_checks::font_sheet`: every script the chain covers, the faces, the icon font | `28` | `font_sheet` |
| Theme window | `themewin.bs` on `window::sheet_form`, pages down the side, each role drawn as itself | `42`, `43` | the mark menu's Theme, or `theme:<page>:<role>` |
| Populate | `populate.bs` on `window::sheet_form`, one row per option of the layout picked | `44` | Populate… on a `multimesh3d` or `multimesh2d` section, or `populate` |
| Changed on disk | `disk::fill` on `window::sheet_form`, a row per file with its two answers, and both for all | — | a file changed under unsaved edits, or `test:disk_changed` |

A state is passed as `balaur edit --state <list>`. The theme window is the one
sheet whose rows change the chrome around them: an edit to a user theme is worn
at once, and a bundled theme is read-only.

```
┌ Theme ────────────────────────────────────────────────────── Save  Close ┐
│ Colours  34   │ [dusk ▾] [based on dark ▾]           Duplicate  Delete   │
│ Text     25   │ name         [dusk                                    ]   │
│ Controls 39   │ …/balaur/themes/dusk.toml · states only what differs      │
│ Boxes    31   │ chip         ( chip )                          [editing]  │
│ Layout   32   │   table      (role) (hover) + active + focus + touch      │
│ Kinds     3   │   fill       ■ [sunken                             ▾]    │
│ Problems  0   │   color      ■ [dim                                ▾] ×  │
│               │   size       [              11                      ]    │
│               │   add        [add a key…  ▾]                              │
│               │ dim on sunken: 6.22:1, reads at AA                        │
└──────────────────────────────────────────────────────────────────────────┘
```

Populate is Godot's Populate Surface over the selected multimesh. The rows
follow the layout: a surface asks which node, a grid its columns and rows,
a ring its radius. The last row says where the list will land.

```
┌ Populate ─────────────────────────────────────────────────────── Close ┐
│ Layout        [surface                                          ▾]   │
│ Surface       [/Main/World/Ground                               ▾]   │
│ Count         [                       50                         ]   │
│ Rotation      [                      1.00                        ]   │
│ Tilt          [                      0.00                        ]   │
│ Scale         [                      1.00                        ]   │
│ Random scale  [                      0.20                        ]   │
│ Seed          [                         1                        ]   │
│ Writes        the scene's [[assets]] block #blades                     │
│ [                          Populate                                ]   │
└────────────────────────────────────────────────────────────────────────┘
```

---

## 9. Folded, moved and resized

The same shell with a dock taken away, a panel moved, the scale raised or the
screen changed. `layout.bs` computes every rect from the room left, so none of
these has code of its own; the shots prove the rects still add up.

| Shot | State | What changes |
| --- | --- | --- |
| `26` | `shut:left,shut:right` | Both side docks fold to their handles in the top corners; the tool rail moves to the left edge and the bottom dock spans the full width. |
| `27` | `move:output:left,move:assets:right` | Output joins the left dock's tabs and Assets the right's (`docks::move_to`). See D25. |
| `29` | `scale:1.8` | 1067 design px, under `COMPACT_WIDTH`: the workspace and bar labels fold to icons, and the dock's filter and level pills drop out. |
| `30` | `scale:2.4` | 800 design px: the tool rail goes to two columns, the viewport keeps two of its chips, and the side docks hold their widths while the stage narrows. |

`38`–`41` set the framebuffer with `--size`, and the last three add `--touch`,
which raises the scale to the 44 px touch target. The classes are
[PLAN-responsive.md](PLAN-responsive.md)'s.

| Shot | Size | Classes | What changes |
| --- | --- | --- | --- |
| `38` | 1920 × 1080 | `pointer`, `wide`, `tall` | Nothing: the default shell, as `01`. |
| `39` | 834 × 1194, touch | `touch`, `medium`, `tall` | Workspace labels fold to icons, both side docks fold to handles, and the bottom dock runs the full width. |
| `40` | 390 × 844, touch | `touch`, `narrow`, `tall` | The bottom dock folds to a ▲ chip beside the zoom reading; the top bar ends after the fourth workspace. See D24. |
| `41` | 844 × 390, touch | `touch`, `medium`, `short` | The bottom dock folds away and the tool rail goes to two columns; the top bar is cut at the right edge. |

---

## 10. What the screenshots show is wrong

Open defects, reproducible from the state in the last column. A fixed one is
dropped; git holds it.

| # | Defect | Where | Seen in |
| --- | --- | --- | --- |
| D4 | **Long property names widen the inspector.** `Angular damping` and `Center of mass` widen the label column and the panel with it, though the values stay inside the window. | `panels/inspector.bs` | `02`, `20`, `34` |
| D10 | **The Script workspace's inspector is ~500 px of nothing** between Events and Add component. | `panels/inspector.bs` | `03`, `12` |
| D13 | **The palette card has no edge.** Card fill ≈ scrimmed background, the first-row highlight is narrower than the rows, and the list clips mid-row with no scroll cue. | `palette.bs` | `07` |
| D14 | **Script identity is stated four times:** the tree's `‹›` glyph, the hooks sidebar, the inspector's Events section and the events document tab. | across | `03`, `16` |
| D15 | **The dock tab row is 12 controls wide:** the tabs, a filter field, three level pills and clear, with no grouping. | `panels/dock.bs` | `01` |
| D24 | **The top bar clips instead of folding on a narrow screen.** At 390 px the bar ends after the fourth workspace: UI, play, pause, stop and the command pill are not drawn, so a phone cannot start the game from the bar. At 844 × 390 the last control is cut at the edge. | `panels/topbar.bs` | `40`, `41` |
| D25 | **A moved panel draws in the wrong dock.** Output moved to the left dock shows an empty body; Assets moved to the right draws its toolbar there and its cards in the bottom dock. | `docks::move_to`, `panels/dock.bs` | `27` |
| D26 | **Whatever draws at the top of the stage sits under the viewport's chip strip.** The font sheet's `FONT COVERAGE` and a playing game's `Score 0` overdraw `3D · Perspective` and `2D · Orthographic`. | `panels/chips.bs` | `09`, `28` |
| D27 | **Bone names overdraw each other.** A chain's labels sit at their joints at one size, so `Plume`, `Plume2`, `Plume3`, `Elbow` and `Hand` pile into one unreadable block. | `rig.bs` | `21` |
| D28 | **Deep tree rows clip names mid-word.** Past the fifth level the row runs out of width and cuts `Plume2` to `Plum` and `Hand` to `Ha`, with no ellipsis. | `panels/scene_tree.bs` | `21` |

### Where the measurements live

The token set is in `editor/themes/`: each theme a folder of a dark and a light
half, and `roles.toml` for the roles every theme shares. Type is four sizes,
`style::SMALL` 11, `MEDIUM` 12, `LARGE` 14 and `EXTRA_LARGE` 17, and a call
site names one of those or takes the size its `role` carries. The rest, dock
sizes and seams, live in the scene and the roles that draw them.
