# The editor's screens

Every surface the editor draws, as an ASCII mockup next to the screenshot that
proves what it actually looks like. This is the review sheet: read a mockup,
open the PNG beside it, write the complaint under the screen.

The shell is **Stage** as of 2026-09-03: the scene runs edge to edge and every
panel is a sheet floating on it, at a rect `editor/scripts/layout.rn` computes.
The skeleton in §1 below is the docked shell it replaced, kept because the
defect table still refers to it; the rects that matter now are the table in
[PLAN-editor-redesign.md](PLAN-editor-redesign.md) §1. Re-capture before
reading further — the screenshots are regenerated, the prose is not.

Regenerate the PNGs into `target/uiaudit/` with:

    scripts/uiaudit.sh

The names below are that script's names. `scripts/uiaudit.sh 03-script` takes
one. Captures are offscreen at 1600 × 1000 device px, `ui_scale` 1.25, so the
shell lays out at 1280 × 800 design px — one notch above the 1240 px compact
threshold, which is the widest thing most people will ever run it at.

---

## 1. The skeleton

Fixed. Nothing docks, floats or re-arranges; a persona re-fills four regions
and may re-order the dock. Declared in `editor/scripts/editor.rn:draw_ui` in
this order: persona bar, status bar, tree, inspector, dock, centre. Under Stage
every one of these becomes a sheet at a rect the editor computes; the sizes
below are what the redesign starts from.

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
| persona bar | 56 px | `chrome::persona_bar` | no |
| status bar | 28 px | `chrome::status_bar` | no |
| tree + secondary | 262 px | `left::draw` | no |
| inspector | 308 px | `inspector::draw` | no |
| tool rail | 46 px | `center::rail` | no, hides with the viewport |
| document tabs | 38 px | `center::doc_tabs` | no |
| bottom dock | 150 / 212 px | `dock::draw` | no |
| centre | fills | `center::viewport` / `script_editor` / `events_view` | no |
| split code pane | 620 px | `center::split_code` | **yes**, the only one |

---

## 2. Personas

Five, `defs::personas()`. Switching resets the tool to Select, points the
document tab at scene or script, and points the dock at output or timeline.
Selection is persona-independent.

| | Scene | Script | Animate | Physics | Interface |
|---|---|---|---|---|---|
| tool rail | select move rotate scale zoom | *(none — rail hides)* | select move bone polygon key zoom | select move polygon zoom | select move zoom |
| secondary panel | Scenes | Rune modules | Clips | Collision | Interface |
| viewport chips | 3D·Perspective, Snap 8 px, Guides | — | Motion path, Snap 8 px | Show colliders, Sleep bodies | Safe area, 1920×1080 |
| inspector | transform, skeleton, polygon, components, script | attached script, language, hot reload | skeleton, polygon, animation, transform, bone/polygon comps, script | body/collider comps, polygon, script | widget comps, interface, script |
| default dock | output | output | timeline | output | output |
| screenshot | `01-scene-3d`, `02-scene-2d` | `03-script` | `04-animate` | `05-physics` | `06-interface` |

---

## 3. Left column — `left::draw`

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

The secondary panel's height is `clamp(50 + rows × 33, 83, 280)`; the tree
takes the rest. Contents per persona: scene files, `.rn` files with line
counts, clips with their library, bodies/colliders with kind and dimension,
widgets and `draw_ui` scripts.

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

The chrome is one `ui::overlay` sized from `ui::central_rect()`. Selection,
gizmos, colliders, guides and the motion path are 3D lines from `gizmo`,
`gizmo2d`, `overlays`, `rig` and `polygon` — not egui.

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

Same pane serves `.rn` and `.wesl`; a shader swaps the hooks list for a
`SHADER` label and turns gutter clicks into value previews.

### 4c. Events — `center::events_view` (`16`)

One flat row per hook across the whole document: `● Node.hook()` left,
`scripts/file.rn:12` right. No grouping, no click target.

### 4d. Split — `center::split_code` + `viewport` (`17`)

Code as a resizable right panel, viewport in what is left. **Currently
broken** — see §7.

---

## 5. Bottom dock — `dock::draw`

Seven built-in tabs plus one per registered plugin. 150 px, or 212 px for
timeline, debugger, session and profiler.

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
| *plugin* | whatever `register()` returned | 150 | `19` |

---

## 6. Inspector — `inspector::draw` (308 px)

```
┌────────────────────────────────┐
│ ◉  Spinner                     │ 34  heading 16
│    MeshInstance3D              │     mono 11 type
│ ▾ TRANSFORM ────────────────── │ 10 caps + rule
│ Position   [x 0.00][y 0.00][z] │ 28  84 px label column
│ Rotation   [x 0.00][y 0.00][z] │
│ ▾ SCRIPT ───────────────────── │
│ Script     [ scripts/x.rn open]│     sage chip
│ PROPERTIES                     │
│ clockwise  (●———)              │ 42×24 toggle
│ speed      [ 3.50            ] │
│ ▾ EVENTS ───────────────────── │
│ ● init()              → engine │ 30
│ ● update()            → engine │
│                                │
│ ─────────────────────────────  │
│ [      ＋ Add component      ] │ 34 accent
└────────────────────────────────┘
```

Six control shapes: numeric field, select, toggle, slider, script chip, asset
row. Component sections are generated from `scene::component_schema`, so a
plugin's component gets a section for free — and so does its label width.

---

## 7. Overlays and windows

| Surface | Code | Shot | State |
|---|---|---|---|
| Command palette | `palette::draw` — `ui::modal`, the one scrim | `07` | ⌘K, or `--state palette` |
| Input overlay | `inputview::draw` — key chips, click ripples, drawn cursor | `18` | `--state input` |
| Plugin window | `plugins::draw_windows` — `ui::window`, floating | `19` | `--state counterdemo` |
| Node context menu | `left::tree_row`'s `menu:` — add child, attach script, duplicate, delete | — | right-click |
| Showcase driver | `showcase::draw` — scripted input for the manual's clips | — | `--state show:<name>` |

---

## 8. What the screenshots show is wrong

Ordered by how much of the shell it spoils. Each is reproducible from the
state in the last column.

**D1, D2, D3 and D5 are fixed** (2026-09-03), and they were one bug:
`center::draw` wrote `S.viewport_live = split || !document` straight to the
field, which in Rune 0.14 also overwrites the local on the left of a
short-circuit — so `split` became true on every frame, in every persona. The
centre carved a code pane it was never asked for, and every rect derived from
`ui::central_rect()` was wrong: the viewport, the overlay chrome and the
widget layer alike. The pitfall is written up in `AGENTS.md`; `layoutdemo`
asserts the centre now, and the screenshots below are from after the fix.

| # | Defect | Where | Seen in |
|---|---|---|---|
| D1 | *Fixed.* **Game widgets escaped the viewport.** The widget layer paints over the document tabs, the tool rail, the axis pill and the dock tab row; in the Interface persona the HUD and the dashed safe-area rect run across the inspector. | `editor.rn:update` `ui::set_widget_layer`, `gizmo::viewport_rect` | `06`, `14`, `18` |
| D2 | *Fixed.* **Split was unusable.** The code column collapses to ~25 px — gutter only, no code — whatever `split_w` says. | `center::split_code` | `17` |
| D3 | *Fixed.* **A ghost code gutter sat between viewport and inspector.** A ~25 px `code_bg` strip with the selected script's line numbers is drawn in every persona whenever the selection has a script. It also clips the document-tab hint to `no…`. | centre layout | `01`, `08`, `13`, `20` |
| D4 | *Improved, not fixed — values no longer clip off the window, the panel still widens.* **Long property names blow the inspector out of the window.** `angular_damping`, `center_of_mass` widen the label column, the panel takes the full width, values clip off the right edge and the dock is overdrawn. | `inspector::row`'s label column | `02`, `20` |
| D5 | *Fixed in the viewport; still overflows in a split.* **The zoom pill drew on top of the inspector**, over the `SCRIPT` heading. The overlay is wider than the viewport it belongs to. | `center::viewport` overlay rect | `01`, `15`, `17` |
| D6 | **Glyphs are missing from the shipped font.** `⌥` renders as `~` in every palette shortcut; the Translate tool's `✥` renders as an empty box in every rail. | `defs::tool_icon`, `palette::commands` | `07`, `10`, `20` |
| D7 | **The dock's right-hand hint lies.** Session, Profiler and plugin tabs all fall through to the animation branch and print `no clip`. | `dock::tab_row` | `10`, `13`, `14`, `19` |
| D8 | **Plugin windows are unthemed.** `ui::window` draws egui's stock frame — pale title bar, centred serif title, native close button — against the editor's dark chrome, and it opens over the persona bar. | `plugins::draw_windows` | `19` |
| D9 | **Tall docks are mostly empty.** Debugger, Profiler and Problems reserve 212 / 150 px and fill one row; ~180 px of dead panel. | `dock::draw` height | `10`, `12`, `14` |
| D10 | **The Script persona's inspector is ~500 px of nothing** between Events and Add component. | `inspector::draw` | `03`, `12` |
| D11 | **The timeline has no time axis.** No ruler, no ticks, no playhead line down the lanes; keys are `●`/`○` text glyphs, and track pills are ragged widths. | `dock::timeline` | `15` |
| D12 | **Asset cards are unreadable.** Near-black tiles on a near-black panel, no thumbnails, filenames flush against the dock's bottom edge. | `dock::assets` | `11`, `25` |
| D13 | **The palette card has no edge.** Card fill ≈ scrimmed background, the first-row highlight is narrower than the rows, and the list clips mid-row with no scroll cue. | `palette::draw` | `07` |
| D14 | **Script identity is stated four times** — the tree's `‹›` glyph, the Rune modules list, the hooks sidebar, the inspector's Events section and the events document tab. Five, counting the tab. | across | `03`, `16` |
| D15 | **The dock tab row is 12 controls wide** — 8 tabs, a filter field, three level pills and clear — with no grouping. | `dock::tab_row` | `01` |
| D16 | **The tree's connector rails are mono text.** `├─`/`└─`/`│ ` glyphs drift out of alignment with the 27 px rows and the depth indent. | `left::tree_row` | `01`, `03` |
| D17 | **The log's columns do not line up.** The severity mark, the timestamp, the `s`, and the tag each start where the last one ended. | `dock::output` | `01` |

That those four were one bug is the argument for the plan's §2. They were not
four places drawing badly; they were one wrong rect, read by four consumers
that each derived their own. Stage makes that class of bug the normal case,
which is why one module owning every rect comes before anything else.

### Where the measurements live

The shell was built from a prototype whose terracotta-and-sage tokens are
gone: the token set is the website's ink-and-blue palette, in
`editor/scripts/theme.rn`. The measurements it fixed — the dock heights, the
84 px label column, the 999 px radii, the 1 px seams — live in the code that
draws them, and D3, D4 and D5 are places that code has drifted off them.
This file is the state of the world; the plan is where it is going.
