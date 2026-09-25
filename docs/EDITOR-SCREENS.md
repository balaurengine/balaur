# The editor's screens

Every surface the editor draws: an ASCII mockup, the code that draws it, and the
screenshot that proves what it looks like. The review sheet — read a mockup, open
the PNG, write the complaint in §8.

`scripts/uiaudit.sh` shoots whole shells, one a state. `scripts/views.sh` shoots
one **view** at a time — each dock panel, cut out of the shell to its own PNG in
`target/views/`, plus two contact sheets: `_sides.png` puts the side panels in a
row and `_bottoms.png` stacks the bottom ones, which is the shape each is seen
in. Design a panel against its own picture rather than hunting for it in a
screenshot of everything.

The shell is **Stage**: the scene runs edge to edge and every panel is a sheet at
a rect `editor/scripts/layout.rn` computes. The rects that matter are the table
in [PLAN-editor-redesign.md](PLAN-editor-redesign.md) §1; §1 below is the docked
shell it replaced, kept for its sizes. The workspace bar, command pill and document
tabs are one full-width bar (`chrome::top_bar`); the status strip is the bottom
dock's foot; a side dock minimises to a 32 px handle.

Regenerate the PNGs into `target/uiaudit/` with `scripts/uiaudit.sh`, or one with
`scripts/uiaudit.sh 03-script`. Captures are offscreen at 1920 × 1080 device px
(`OFFSCREEN_SIZE`), `ui_scale` 1.25, so the shell lays out at 1536 × 864 design
px, above the 1240 px compact threshold. `29`, `30` and `38`–`41` change the
scale or the size on purpose. The screenshots are regenerated; the prose is
not.

---

## 1. The skeleton

Fixed: nothing floats. A workspace re-fills four regions and states which panels
are open in each dock; a tab's close mark takes one away and the mark menu at
the head of the bar puts it back. Drawn by `editor.rn:draw_ui` in this order:
workspace bar, status bar, tree, inspector, dock, centre.

```
┌──────────────────────────────────────────────────────────────────────────────┐
│ ● balaur  (Scene Script Animate Physics Interface)   ▶ ⏸ ■   ⌕ command   ☾   │ 56
├───────────────┬──┬─────────────────────────────────────────┬─────────────────┤
│ NODE TREE  ＋ │  │ ◇ main.toml  ‹›script.rn  ⚯events  ◫Split│ ◉ NodeName      │
│               │t │─────────────────────────────────────────│   Type          │ 38
│  World        │o │                                         │ ▾ SECTION ──────│
│   ├ Ground    │o │                                         │  label  [value] │
│   ├ Ball    ‹›│l │        viewport  /  code  /  events      │  label  [value] │
│   └ Player  ‹›│  │                                         │                 │
│     ├ Sprite  │r │                                         │ ▾ EVENTS ───────│
│     └ Hitbox  │a │                                         │  ● hook() →     │
│               │i │                                         │                 │
│               │l │                                         │                 │
├───────────────┤46│                                         │                 │
│ SCENES        │  │                                         │                 │
│  ◇ main.toml  │  │                                         │                 │
├───────────────┴──┴─────────────────────────────────────────┤                 │
│ Output Problems Assets Timeline Debugger Session Profiler   │                 │ 34
│ ─────────────────────────────────────────────────────────  │                 │
│ 0.2 s  project  scene key 'color' has no handler            │ [＋ Add compo.] │ 150
├─────────────────────────────────────────────────────────────────────────────-┤
│ ● editing  16 nodes · 4 scripts  ● Rune VM warm  kiss3d · wgpu  Scene · Select│ 28
└──────────────────────────────────────────────────────────────────────────────┘
  262 fixed        46      central (fills)                    308 fixed
```

| Region | Size | Code | Resizable |
|---|---|---|---|
| workspace bar | 40 px | `chrome::top_bar` | no |
| status bar | 28 px | `chrome::status_bar` | no |
| tree + secondary | 262 px | `left::draw` | no |
| inspector | 308 px | `inspector::draw` | no |
| tool rail | 46 px | `center::rail` | no, hides with the viewport |
| document tabs | 38 px | `center::doc_tabs` | no |
| bottom dock | 150 / 212 px | `dock::draw` | no |
| centre | fills | `center::viewport` / `script_editor` / `events_view` | no |
| split code pane | 620 px | `center::split_code` | **yes**, the only one |

---

## 2. Workspaces

Five, `defs::workspaces()`. Switching resets the tool to Select, points the
document tab at that workspace's default and opens the panels `defs::workspace_docks`
names, in the docks it names them for. That is all a workspace does to the docks:
what it opens closes again from the tab, and what it leaves out opens from the
mark menu without changing workspace. Selection is workspace-independent.

| | Scene | Script | Animate | Physics | Interface |
|---|---|---|---|---|---|
| tool rail | select move rotate scale tiles zoom | *(none — rail hides)* | select move bone polygon key zoom | select move polygon zoom | select move zoom |
| secondary panel | Scenes | Rune modules | Clips | Collision | Interface |
| viewport chips | 3D·Perspective, Snap 8 px, Guides | — | Motion path, Snap 8 px | Show colliders, Sleep bodies | Safe area, 1920×1080 |
| inspector | transform, skeleton, polygon, components, script | attached script, language, hot reload | skeleton, polygon, animation, transform, animation/bone/polygon comps, script | body/collider comps, polygon, script | widget comps, interface, script |
| panels open | scene outline · output problems assets · inspector import | scene outline · output problems docs debugger · inspector | scene outline · timeline output library · inspector | scene · output problems profiler · inspector | scene outline · output problems assets · inspector import |
| screenshot | `01-scene-3d`, `02-scene-2d` | `03-script` | `04-animate` | `05-physics` | `06-interface` |

The Scene workspace in other states: `08` is the light theme, `09` a game playing
in the viewport with its HUD and a recording running, `22` the same logo drawn
plain and through a material's shader, `23` a 3D skin on three
bones, `34` a selected spot light with its cone, and `35` the pen tool at 45 %.
`21` is the Animate workspace with the bone tool.

---

## 3. Left column — `left::draw`

The tree's header carries the search field and a facet chip row (3D, 2D,
Physics, Draws, Interface, Script); a chosen chip flattens the tree to the
nodes carrying a component with that tag. A row shows a lock or a hidden mark
where it has one, and a selected row that is not the active one reads bold.

```
┌──────────────────────────────┐ 262
│ NODE TREE                 ＋ │ 36  heading 10 px caps + 21 px add
├──────────────────────────────┤
│ ▾ ◉ World                    │ 27  depth 0, bold
│ ├─ ▣ Ground                  │ 27  rails are mono ├─ └─ │ glyphs
│ ├─ ○ Ball                 ‹› │     trailing ‹› = has script
│ ├─ ◆ Spinner              ‹› │     selected = solid accent pill
│ ├─ ◐ Platform             ‹› │
│ ├─ ▾ ○ CrateA             ⧉  │     trailing ⧉ = prefab instance
│ │  ├─ ● Crate               │     an instance's children are dimmed
│ │  └─ ◆ Lid                 │
│   … scrolls …                │
├──────────────────────────────┤ 1 px rule
│ SCENES                       │ 11 top / 9 bottom
│ ┌──────────────────────────┐ │
│ │ ◇ crate.toml             │ │ 29  sunken pill, meta right-aligned
│ │ ◇ main.toml         open │ │ 29
│ └──────────────────────────┘ │
└──────────────────────────────┘
```

Secondary panel height is `clamp(50 + rows × 33, 83, 280)`; the tree takes the
rest. Its contents per workspace: scene files, `.rn` files with line counts, clips
with their library, bodies and colliders, widgets and `draw_ui` scripts.

---

## 4. Centre

### 4a. Viewport — `center::viewport` (`01`, `02`, `05`)

```
┌────────────────────────────────────────────────────────┐
│ ┌3D · Perspective┐ ┌Snap 8 px┐ ┌Guides┐                │ 26 px chips, 12 in
│                                                        │
│                    (kiss3d renders here,               │
│                     egui draws a transparent hole)     │
│                                                        │
│ ┌x → y ↑ z ↓┐                        ┌ − │ 100 % │ ＋ ┐│ 28 axis, 22 zoom
└────────────────────────────────────────────────────────┘
```

Chrome is one `ui::overlay` sized from `ui::central_rect()`. Selection, gizmos,
colliders, guides and the motion path are 3D lines from `gizmo`, `gizmo2d`,
`overlays`, `rig` and `polygon` — not egui.

### 4b. Code — `center::code_pane` (`03`, `24`)

```
┌────────────────────────────────────────────┬──────────────┐
│  1 // Spins in place; the `reverse` action │ HOOKS IN FILE│ 172
│  2                                         │ ┌──────────┐ │
│  6 pub fn exports() {                      │ │● exports │ │ 27
│  7     #{ speed: 2.0, clockwise: true }    │ ├──────────┤ │
│  8 }                                       │ │● init    │ │
│    gutter 34 px · mono 12.5 · lh 1.78      │ │● update  │ │
│    breakpoints, problems, warnings and the │ └──────────┘ │
│    stopped line all live in the gutter     │ ● unsaved·⌘S │
└────────────────────────────────────────────┴──────────────┘
```

One pane for `.rn` and `.wesl`; a shader swaps the hooks list for a `SHADER`
label and turns gutter clicks into value previews.

### 4c. Events — `events::view` (`16`, `33`)

One row per `[[nodes.bindings.rows]]` entry on the selected node: an event
dropdown, a `when` field, an action, a target picked from the scene's nodes or
its variables, a value, and a delete. *Add row* and *Convert to script* sit in
the header; the scene's variables and the node's own script hooks are listed
below the rows.

### 4d. Split — `center::split_code` + `viewport` (`17`)

Code as a resizable right panel, viewport in what is left. The only resizable
region in the shell.

### 4e. Focus — `shell::toggle_focus` (`37`)

The code pane with the window to itself: `⇧⌘\`, or the chip beside Split. The
three docks fold and the split goes off; the file's hooks list stays, and the
pair fill the shell's gutter edge to edge, as the top bar does. Leaving puts
back the layout focus folded. A panel opened under it — from `⌘K`, the mark
menu or a diagnostic — ends focus rather than being shut again.

---

## 5. Bottom dock — `dock::draw`

Seventeen panels exist; the workspace says which of them are open, and a
registered plugin's joins the bottom dock. 150 px; 212 px for timeline,
debugger, session, profiler and cost. Every tab carries a close mark, and a
dock emptied of tabs gives its column back to the scene.

```
┌────────────────────────────────────────────────────────────────────┐
│ Output Problems 3 Assets Timeline Debugger Session Profiler Counter│ 34
│                                     [filter…] all warn error clear │
│ ────────────────────────────────────────────────────────────────── │
│ 0.2  s  project     scene key 'color' on 'Lid' has no handler      │ 11.5 mono
└────────────────────────────────────────────────────────────────────┘
```

| Tab | Content | Height | Shot |
|---|---|---|---|
| Output | timestamp · tag · message, filtered by level and a query | 150 | `01` |
| Problems | lint findings, errors first, each a clickable `file:line` | 150 | `10` |
| Assets | 96 px cards over a `res://` breadcrumb; new folder, rename, delete | 150 | `11`, `25` |
| Timeline | `＋ Key` `− Key`, a scrubber, then one lane per track | 212 | `15` |
| Debugger | continue/over/into/out, the pause reason, frames and locals | 212 | `12` |
| Session | recordings with tick counts; play, keep, export, delete, verify | 212 | `13` |
| Profiler | `FRAME 1.17 ms of 16.7`, then per-script cost rows | 212 | `14` |
| Cost | `FRAME 17 draws · 3758 triangles`, then a bar per node | 212 | `31` |
| Library | material, lighting and template cards behind three chips | 150 | `32` |
| Docs | the script reference, module by module, from the running engine | 150 | — |
| Tiles | the tile set's palette, and the layers it paints into | 150 | — |
| *plugin* | whatever `register()` returned | 150 | `19` |

---

## 6. Inspector — `inspector::draw` (308 px)

```
┌────────────────────────────────┐
│ ◉  Spinner                     │ 34  heading 16
│    MeshInstance3D              │     mono 11 type
│ ▾ TRANSFORM ─────────────── ×  │ 10 caps + rule
│ Position   [x 0.00][y 0.00][z] │ 28  84 px label column
│ Rotation eu[x 0.00][y 0.00][z] │
│ ▾ SCRIPT ───────────────────── │
│ Script     [ scripts/x.rn open]│     sage chip
│ PROPERTIES                     │
│ Clockwise  (●———)              │ 42×24 toggle
│ Speed      [ 3.50            ] │
│ ▾ EVENTS ───────────────────── │
│ ● init()              → engine │ 30
│ ● update()            → engine │
│                                │
│ ─────────────────────────────  │
│ [      ＋ Add component      ] │ 34 accent
└────────────────────────────────┘
```

Six control shapes: numeric field, select, toggle, slider, script chip, asset row.
Component sections are generated from `scene::component_schema`, so a plugin's
component gets a section — and its label width — for free. The transform is one
of them: `[nodes.transform]` in the file, `node.transform.position` in a script,
and a node that names none has none. A row is labelled
from its property name (`half_extents` reads "Half extents") and its label
carries the schema's `description` as a tooltip.

---

### 5a. The mark menu — `menu::draw`

The mark at the head of the bar is the shell's one menu, opened with a left
click: the command palette, Settings and Export, then every panel there is with
a tick beside the open ones, two to a line, then the row that puts the current
workspace's panels back. A folded side dock is a rail of the same marks: the one
that opens it, then one per panel it holds. Shot `36-menu`, whose `menudemo`
state draws the rows as a sheet: no offscreen run can click a popup open.

---

## 7. Overlays and windows

| Surface | Code | Shot | State |
|---|---|---|---|
| Command palette | `palette::draw` — `ui::modal`, the one scrim | `07` | ⌘K, or `--state palette` |
| Input overlay | `inputview::draw` — key chips, click ripples, drawn cursor | `18` | `--state input` |
| Plugin window | `plugins::draw_windows` — `ui::window`, floating | `19` | `--state counterdemo` |
| Node context menu | `left::tree_row`'s `menu:` — add child, attach script, duplicate, delete | — | right-click |
| Showcase driver | `showcase::draw` — scripted input for the manual's clips | — | `--state show:<name>` |
| Font sheet | `selftest::font_sheet` — every script the chain covers, the three faces, the icon font | `28` | `--state fontdemo` |
| Theme window | `themewin::draw` — `window::sheet_form`, pages down the side, each role drawn as itself | `42`, `43` | the mark menu's Theme, or `--state theme:<page>:<role>` |

The theme window is the one sheet whose rows change the chrome around them: an
edit to a user theme is worn at once, and a bundled theme is read-only.

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

---

## 7b. Folded, moved and resized

The same shell with a dock taken away, a panel moved, the scale raised or the
screen changed. `layout.rn` computes every rect from the room left, so none of
these has code of its own; the shots are what proves the rects still add up.

| Shot | State | What changes |
| --- | --- | --- |
| `26` | `shut:left,shut:right` | Both side docks fold to their 32 px handles in the top corners; the tool rail moves to the left edge and the bottom dock spans the full width. |
| `27` | `move:output:left,move:assets:right` | Output joins the left dock's tabs and Assets the right's (`docks::move_to`); the bottom dock keeps Problems and Counter. See D25. |
| `29` | `scale:1.8` | 1067 design px, under `COMPACT_W`: the workspace and bar labels fold to icons, and the dock's filter and level pills drop out. |
| `30` | `scale:2.4` | 800 design px: the tool rail goes to two columns, the viewport keeps two of its chips, and the side docks hold their widths while the stage narrows to about 250 design px. |

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

## 8. What the screenshots show is wrong

Open defects, reproducible from the state in the last column. Fixed ones are
dropped as they are fixed; git holds them.

| # | Defect | Where | Seen in |
| --- | --- | --- | --- |
| D4 | *Improved, not fixed — values no longer clip off the window, the panel still widens.* **Long property names blow the inspector out of the window.** `Angular damping`, `Center of mass` widen the label column, the panel takes the full width, values clip off the right edge and the dock is overdrawn. | `inspector::row`'s label column | `02`, `20`, `34` |
| D10 | **The Script workspace's inspector is ~500 px of nothing** between Events and Add component. | `inspector::draw` | `03`, `12` |
| D13 | **The palette card has no edge.** Card fill ≈ scrimmed background, the first-row highlight is narrower than the rows, and the list clips mid-row with no scroll cue. | `palette::draw` | `07` |
| D14 | **Script identity is stated four times** — the tree's `‹›` glyph, the Rune modules list, the hooks sidebar, the inspector's Events section and the events document tab. Five, counting the tab. | across | `03`, `16` |
| D15 | **The dock tab row is 12 controls wide** — 8 tabs, a filter field, three level pills and clear — with no grouping. | `dock::tab_row` | `01` |
| D19 | **Plugin docks are unreachable.** The dock tab lists are fixed (`docks::state`), `registry.docks` is read for names only, nothing pushes a plugin's id, and `counter.rn` writes `S.dock`, a field that is gone — so the "one per registered plugin" tab above never draws. | `docks.rn`, `plugins.rn`, `editor/plugins/counter.rn` | `--state counterdemo` |
| D20 | **Preferences load only when Settings opens.** `init` never calls `settings::load(prefs)` or `apply`, so theme, `ui_scale`, `sessions/keep`, `verify` and the fault settings are defaults until the window is opened; `editor/appearance/compact` never applies because `editor.rn` overwrites `S.compact` from the window width every frame. | `editor.rn:init`, `settings.rn` | any |
| D22 | **Showcase clicks land off-target.** `showcase.rn` measures its spots off the pre-Stage shell (`ROW = 27` against the tree's 23 px rows, tabs at `y = 27`), so the drawn cursor misses the control the verb drives in every clip. | `showcase.rn` | `--state show:*` |
| D23 | **`settings?<query>` clears itself.** `open_search` sets the query but not the search field's buffer, which the next frame writes back as empty; a category click while a query is typed does the same. | `settings.rn`, `search.rn` | `--state settings?theme` |
| D24 | **The top bar clips instead of folding on a narrow screen.** At 390 px the bar ends after the fourth workspace: Interface, play, pause, stop and the command pill are not drawn, so a phone cannot start the game from the bar. At 844 × 390 the last control is cut at the edge. | `chrome::top_bar` | `40`, `41` |
| D25 | **A moved panel draws in the wrong dock.** Output moved to the left dock shows an empty body; Assets moved to the right draws its toolbar there and its cards in the bottom dock, under the Problems tab. | `docks::move_to`, `dock::draw` | `27` |
| D26 | **Whatever draws at the top of the stage sits under the viewport's chip strip.** The font sheet's `FONT COVERAGE` and a playing game's `Score 0` overdraw `3D · Perspective` and `2D · Orthographic`. | `center::viewport`'s chips | `09`, `28` |
| D27 | **Bone names overdraw each other.** A chain's labels sit at their joints at one size, so `Plume`, `Plume2`, `Plume3`, `Elbow` and `Hand` pile into one unreadable block. | `rig.rn` | `21` |
| D28 | **Deep tree rows clip names mid-word.** Past the fifth level the row runs out of width and cuts `Plume2` to `Plum` and `Hand` to `Ha`, with no ellipsis. | `left::tree_row` | `21` |

### Where the measurements live

The token set is the website's ink-and-blue palette, in `editor/themes/*.toml`:
four surfaces, one seam, three levels of text, one accent and one second
colour. Type is four sizes — `style::SM` 11, `MD` 12, `LG` 14, `XL` 17 — and a
call site names one of those or takes the size its `role` carries. Nothing in
the shell spells a size or a colour of its own.

The measurements — dock heights, the 84 px label column, the 999 px radii, the
1 px seams — live in the code that draws them. This file is the state of the
world; the plan is where it is going.
