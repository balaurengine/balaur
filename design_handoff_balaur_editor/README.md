# Handoff: balaur editor — persona-based editor shell

## Overview

`balaur` is a node-based 2D/3D game engine written in Rust. This handoff covers the **editor shell**: the full-window chrome around the viewport — a persona switcher, node tree, tool rail, document tabs, viewport overlays, a Rune script editor, a bottom dock (output / assets / timeline), an inspector, a status bar, and a command palette.

The organizing idea is borrowed from Affinity's *personas*: one window, five task modes. Switching persona does **not** change the layout skeleton — it re-fills four regions (tool rail, left secondary panel, viewport overlays, inspector sections) and may re-order the bottom dock tabs. Nothing docks, floats or re-arranges. The user always knows where things are.

Target: **egui**, rendered inside a kiss3d window. The design was authored against egui's real constraints — see *Building this in egui* below.

## About the design files

`Editor.dc.html` in this bundle is a **design reference created in HTML** — a prototype showing intended look, layout and behavior. It is not production code and none of it should be ported literally. The task is to **recreate this design in egui** using idiomatic `egui`/`eframe` panel APIs and a custom `Visuals` theme, driven by the measurements and tokens below.

Open it in any browser to explore: click personas, click nodes in the tree, press ⌘K / Ctrl-K for the palette, hit the moon/sun button to switch theme.

## Fidelity

**High fidelity.** Colors, type sizes, row heights, panel widths and radii are all final and exact. Recreate them faithfully. Two deliberate exceptions:

- The viewport contents (grid, ground plate, crate, selected sprite, gizmo handles) are **placeholder stand-ins** for the real render target. Only the *overlay chrome* drawn on top of the viewport — chips, zoom pill, axis pill, selection label, gizmo handle styling — is specified design.
- Icons in the prototype are hand-drawn 24×24 stroked paths standing in for a real icon set. Use Lucide at stroke width 2.75 (the bound design system's icon rule); the prototype's shapes name the intent, not the final artwork.

---

## Layout skeleton

Full window, no floating panels, no user docking. Top to bottom:

```
┌──────────────────────────────────────────────────────────────────────┐
│ persona bar                                            56 px, fixed │
├────────────┬──────────────────────────────────────┬──────────────────┤
│            │ ┌────┬─────────────────────────────┐ │                  │
│ node tree  │ │tool│ document tabs      38 px    │ │  inspector       │
│            │ │rail├─────────────────────────────┤ │                  │
│  262 px    │ │46px│ viewport  /  script editor  │ │   308 px         │
│  fixed     │ │    │           (fills)           │ │   fixed          │
│            │ └────┴─────────────────────────────┘ │                  │
│ ───────────│ ┌─────────────────────────────────┐  │                  │
│ secondary  │ │ bottom dock   150 px (212 when  │  │                  │
│ panel      │ │               timeline)         │  │                  │
├────────────┴─┴─────────────────────────────────┴──┴──────────────────┤
│ status bar                                             28 px, fixed  │
└──────────────────────────────────────────────────────────────────────┘
```

Every seam between regions is a **1 px** line in `--e-line`. Panel widths are fixed in the prototype; making the three splits drag-resizable is a natural egui addition (`SidePanel::resizable(true)`).

### egui mapping

| Region | egui |
| --- | --- |
| persona bar | `TopBottomPanel::top("persona").exact_height(56.0)` |
| status bar | `TopBottomPanel::bottom("status").exact_height(28.0)` |
| node tree + secondary | `SidePanel::left("tree").exact_width(262.0)` |
| inspector | `SidePanel::right("inspector").exact_width(308.0)` |
| tool rail | `SidePanel::left("tools").exact_width(46.0)` (inside the central area) |
| bottom dock | `TopBottomPanel::bottom("dock").exact_height(150.0/212.0)` (inside the central area, declared *after* the outer panels) |
| document tabs | `TopBottomPanel::top("tabs").exact_height(38.0)` inside `CentralPanel` |
| viewport / script | `CentralPanel` |

Declaration order matters in egui: outer chrome (persona, status, tree, inspector) first, then the inner tool rail and dock, then the central panel.

---

## Personas

Five, in this order, always visible in the persona bar. Below 1240 px window width the labels hide and the pills go icon-only (tooltip carries the name).

| Persona | Icon intent | Tool rail | Secondary panel | Viewport overlay | Inspector sections | Default dock |
| --- | --- | --- | --- | --- | --- | --- |
| **Scene** | cursor | Select, Translate, Rotate, Scale, Draw shape, Paint tiles, Zoom | *Layers* — Foreground 3, Actors 6, Backdrop 2 | chips: `2D · Orthographic` (on), `Snap 8 px`, `Guides` | Transform, Rendering, Script | Output |
| **Script** | code brackets | Select, Edit script, Bind event, Watch value, Zoom | *Rune modules* — player.rn 24 L, world.rn 61 L, hearts.rn 18 L | — (viewport is replaced by the editor) | Attached script, Exported vars | Output |
| **Animate** | clock | Select, Translate, Rig bone, Key frame, Zoom | *Clips* — idle 1.2s, run 0.8s, hurt 0.4s | chips: `Onion skin` (on), `Snap 8 px` | Animation, Transform, Script | **Timeline** |
| **Physics** | collider box | Select, Collider, Polygon, Body type, Zoom | *Collision layers* — 1 world on, 2 actors on, 3 pickups off | chips: `Show colliders` (on), `Sleep bodies`; **dashed sage capsule + center-of-mass dot drawn over the selection** | Body, Collision, Script | Output |
| **Interface** | layout blocks | Select, Container, Anchors, Draw shape, Zoom | *Screens* — HUD live, Pause, Title | chips: `Safe area` (on), `1920 × 1080`; **dashed terracotta safe-area rect inset 34 px with a label** | Anchors, Container, Script | Output |

Rules:
- Switching persona resets the active tool to Select.
- Selecting **Script** switches the document area to the script editor and activates the `*.rn` document tab; leaving Script returns to the scene document.
- Selecting **Animate** switches the bottom dock to Timeline and moves Timeline to the first dock tab.
- The node **selection is persona-independent** — it never resets.

---

## Region specifications

### Persona bar — 56 px

Background `--e-panel`, 1 px bottom border `--e-line`, horizontal padding 12 px, item gap 16 px.

1. **Brand**, flex-none: 27 px terracotta circle (`--e-accent-fill`) with a lowercase `b` in the heading face at 15 px, `#f9f4ed`; then `balaur` in the heading face at 17 px, letter-spacing −0.01em. 9 px gap.
2. **Persona group**: a 999 px-radius container, `--e-sunken` fill, 1 px `--e-line-soft` border, 3 px padding, 2 px gap. Each pill is 32 px tall, padding 0 15px (0 9px when compact), heading face 13 px, icon 14 px, 7 px gap. Active pill: `--e-accent-fill` fill, `#f9f4ed` text. Inactive: transparent fill, `--e-dim` text.
3. Flexible spacer.
4. **Transport group**: same container treatment; three 32 px circular buttons — play/pause (toggles; active state uses the accent fill), pause, stop. 14 px icons.
5. **Command pill**: 32 px tall, `--e-sunken` fill, 1 px `--e-line-soft`, padding `0 6px 0 12px`, 8 px gap: 14 px search icon, `Run a command` at 12 px `--e-faint`, then a `⌘K` chip (999 px, `--e-panel` fill, `--e-dim`, 11 px, weight 600, padding 3px 8px). Hover: border → `--e-accent-fill`, text → `--e-dim`. Label hides when compact.
6. **Theme button**: 32 px circle, same fill/border as the command pill, 15 px moon (in dark) / sun (in light) icon. Hover: accent border and icon.

### Node tree — 262 px

- Header row 36 px: `NODE TREE` in the heading face, 10 px, uppercase, letter-spacing 0.1em, `--e-dim`; right-aligned 21 px circular add button, `--e-accent-soft` fill, `--e-accent` icon.
- Rows: **27 px** tall, 999 px radius, 1 px vertical gap, indent `10 + depth × 15` px, right padding 9 px, 7 px gap. 13 px type icon, name (ellipsised), then a 12 px sage script glyph shown only when the node has a script attached. Depth-0 row is weight 700.
- Selected row: `--e-accent-fill` fill, `#f9f4ed` text and icon.
- Scrolls independently.

The scene in the prototype (indent shown by nesting):

```
World                Node2D              world.rn
  Camera             Camera2D            follow.rn
  Ground             TileMap
  Player             KinematicBody2D     player.rn      ← selected by default
    Sprite           Sprite2D
    Hitbox           CollisionShape2D
    Animator         AnimationPlayer     anim_fsm.rn
  Crate              RigidBody2D
  Lantern            PointLight2D
  HUD                CanvasLayer
    HeartRow         HBox                hearts.rn
```

- **Secondary panel** below, separated by a 1 px `--e-line-soft` rule: a section label (same 10 px uppercase heading treatment, 11 px top / 9 px bottom padding) and 29 px pill rows, `--e-sunken` fill, 4 px gap, 11 px padding: 12 px icon, name, right-aligned meta in the mono face at 11 px `--e-faint`. Contents per persona in the table above.

### Tool rail — 46 px

Vertically stacked 32 px circular buttons, 3 px gap, 8 px top/bottom padding, 16 px icons. Active: `--e-accent-soft` fill, `--e-accent` icon. Inactive: transparent, `--e-dim`. Tooltip = tool name.

### Document tabs — 38 px

`--e-panel` fill, 1 px bottom `--e-line-soft`, 12 px horizontal padding, 6 px gap. Three tabs: `level_01.balaur` (node icon), `<selected node's script>.rn` (code icon), `events` (link icon). Each 27 px tall, 999 px radius, padding 0 13px, 12 px type, 11 px icon, label ellipsised at 132 px max. Active: `--e-accent-soft` fill, `--e-accent` text. Right side: a mono 11 px `--e-faint` hint — `level_01 · 11 nodes` or `Rune · player.rn` — hidden when compact.

### Viewport

- Ground: `--e-bg`, overlaid with a two-level grid — 28 px minor lines in `--e-grid`, 140 px major lines in `--e-grid-major`. One brighter horizontal and one vertical line mark the world origin.
- Selection is drawn as a fill/outline rect plus a 1.5 px `--e-accent-fill` marquee offset 7 px outward at `--radius-md`, with four 9 px circular handles (`--e-bg` fill, 1.5 px accent border) at its corners.
- Selection label: a 22 px pill above the marquee, `--e-accent-fill` fill, `#f9f4ed`, mono 11 px, padding 0 10px — the node name.
- **No other floating object labels.** Object identity comes from the tree and the selection pill.
- Overlay chips, top-left, 12 px inset, 6 px gap, wrapping: 26 px tall, 999 px radius, padding 0 12px, 1 px `--e-line-soft` border, 11 px weight 600. On = `--e-accent-soft` / `--e-accent`; off = `--e-panel` / `--e-dim`.
- Zoom pill, bottom-right: 999 px container, `--e-panel` fill, 1 px `--e-line-soft`, 3 px padding — `−`, the zoom value (mono 11 px, 52 px min width, centered), `+`. Steps of 25 %, clamped 25–400 %.
- Axis pill, bottom-left: 28 px tall, same container treatment, mono 11 px — `x →` in accent, `y ↓` in sage. This is the 2D orientation cue; in 3D it becomes the axis gizmo.

### Script editor (Script persona / `.rn` tab)

- Gutter + code on `--e-code-bg`. Mono 12.5 px, line-height 1.78, 12 px vertical padding. Line numbers: 24 px wide, right-aligned, `--e-faint`, non-selectable, 12 px gap to the code. Code padding 16 px horizontal.
- Highlighted lines (the prototype marks the jump branch) get an `--e-accent-soft` row fill spanning the full width.
- Syntax roles, all tokens: keyword `--k-key` weight 600, string `--k-str`, number `--k-num`, comment `--k-com`, function/identifier `--k-fn`, type/builtin `--k-type`, punctuation `--k-punc`. Rune keywords: `use pub fn let const if else while for in match return struct impl true false`. Builtins highlighted as types: `emit input balaur math Vec2`. Any capitalized identifier is a type. An identifier directly after `fn` is weight 600.
- **Right sidebar, 172 px**: `--e-panel`, 1 px left `--e-line-soft`, 12 px padding — `HOOKS IN FILE` label, then 27 px `--e-sunken` pills with a 6 px status dot and the hook name in mono 11 px: `ready`, `update` (accent dots), `on_hit` (sage), `emit jumped` (faint). This is the file's engine-facing surface at a glance.

### Bottom dock — 150 px (212 px for Timeline)

Tab row 34 px: 24 px pills, padding 0 12px, 11.5 px weight 600. Active: `--e-accent-fill` / `#f9f4ed`. Inactive: `--e-sunken` / `--e-dim`. Right-aligned mono 11 px hint (`balaur-core 0.4.2` / `res://` / `0.00 – 0.80 s · 24 fps`), hidden when compact. Content area separated by a 1 px `--e-line-soft` rule.

**Output** — mono 11.5 px rows, 2 px gap: 56 px timestamp column (`--e-faint`), 64 px tag column (weight 600, colored per severity — sage ok, accent warning, `--e-dim` info), then the message. Two sets: idle (build/rune/assets lines, one warning) and playing (VM boot, scene instanced, gameplay events with timestamps).

**Assets** — 96 px cards, 12 px gap: a 66 px tinted tile at `--radius-md` with a 20 px icon, then the filename in mono 11 px `--e-dim`, ellipsised. Textures and tilemaps tint accent/sage; scripts, audio and folders sit on `--e-sunken`.

**Timeline** — 186 px track-name column (1 px right rule, 8 px padding, 30 px pill rows with a 7 px dot and 12 px name), then the lane area: vertical rules every 48 px, one 30 px lane per track with a 2 px `--e-line-soft` baseline and 11 px circular keyframes (1.5 px colored border; filled before the playhead, `--e-bg`-filled after). Playhead: a 2 px `--e-accent-fill` vertical line with an 18 px time pill at the top. Tracks: `position`, `sprite.frame`, `hitbox.enabled`, `emit("step")`.

### Inspector — 308 px

- Header: 30 px `--e-accent-soft` circle with a 15 px `--e-accent` type icon; node name in the heading face 16 px; node type below in mono 11 px `--e-faint`.
- Sections: a 10 px uppercase heading-face label with a 1 px `--e-line-soft` rule filling the remaining width; 12 px between sections, 8 px inside.
- Rows: 28 px min height, an 84 px label column at 12 px `--e-dim`, then the control area (8 px gap). Five control shapes:
  - **Numeric field** — 28 px, 999 px radius, `--e-sunken` fill, 1 px `--e-line-soft`, padding 0 11px: optional axis letter (10 px, weight 700, `--e-accent`) then the value in mono 11.5 px. Vector rows put two side by side (`x`, `y`) at equal width.
  - **Select** — same shell, 12 px label plus an 11 px chevron in `--e-faint`.
  - **Toggle** — 42 × 24 px track, 3 px padding, 18 px knob. On: `--e-accent-fill` track, `#f9f4ed` knob, knob right. Off: `--e-sunken` track, `--e-faint` knob, knob left.
  - **Slider** — 5 px `--e-sunken` rail, accent fill, 13 px knob (`--e-bg` fill, 2 px accent border), then a 34 px right-aligned mono 11.5 px readout.
  - **Script chip** — 28 px pill, `--e-sage-soft` fill, 1 px `--e-sage`: the filename in mono 11.5 px sage, then a 20 px `open`/`add` button (999 px, `--e-panel` fill, `--e-dim`, 10 px weight 700). `open` jumps to the Script persona with that file loaded; `add` creates and attaches a new `.rn`.
- **Events** section, always last: 30 px `--e-sunken` pills with a 6 px dot, the signal signature in mono 11.5 px, and the bound target right-aligned at 11 px `--e-faint` — `jumped(pos) → world.rn`, `died() → GameOver`, `body_entered → Hitbox`.
- Footer, above a 1 px `--e-line-soft` rule: a full-width 34 px `--e-accent-fill` button, `#f9f4ed`, heading face 13 px — **Add component** (opens the palette scoped to components).

Section contents per persona are in the persona table; exact field values are in `Editor.dc.html` (`personaSections`).

### Status bar — 28 px

`--e-panel`, 1 px top `--e-line`, mono 11 px `--e-dim`, 12 px padding, 16 px gap. Left: `editing` / `playing · 60 fps` with a status dot (sage when playing, `--e-faint` idle); `11 nodes · 3 scripts`; `Rune VM warm` with a sage dot; `kiss3d · vulkan`. Right, flush: `<Persona> persona · <Active tool>`.

### Command palette

The **only** overlay in the design — an egui `Window`/`Area`, modal, centered horizontally, 11 vh from the top, 560 px wide (max 92 vw). Scrim `rgba(23,25,28,0.42)`. Card: `--e-panel`, 1 px `--e-line`, `--radius-lg`, clipped.

- Search row: 12 px vertical / 16 px horizontal padding, 1 px bottom `--e-line-soft` — 16 px accent search icon, a borderless 15 px input (placeholder `Nodes, commands, personas…`), an `esc` chip (`--e-sunken` fill, `--e-faint`, 11 px weight 600).
- Results: max height 326 px, scrolling, 8 px padding. Rows 40 px, 999 px radius, 11 px gap — 15 px accent icon, the label at 13.5 px, a right-aligned mono 11 px shortcut in `--e-faint`. First result is pre-highlighted with an `--e-sunken` fill; hover fills the same.
- Substring filter on the label, case-insensitive.
- Commands: switch to each persona (⌥1–⌥5), `Add child node…` (⇧A), `Attach Rune script to selection` (⌥S), `Run scene` (F5), `Reload Rune VM` (⇧R), `Toggle dark chrome` (⌥L), `Bake collision shapes`, `Open animation timeline` (⌥3).
- Opens on ⌘K / Ctrl-K and on the toolbar pill; closes on Esc, on scrim click, and on running a command. Query resets on every open.

---

## Interactions & behavior

Implemented in the prototype and expected in the editor:

| Trigger | Effect |
| --- | --- |
| Persona pill | Sets persona; resets tool to Select; re-fills rail/secondary/overlays/inspector; may switch document and dock (see persona rules) |
| Node tree row | Sets selection; inspector header, sections and the `.rn` document tab all follow |
| Tool rail button | Sets active tool; status bar right side updates |
| Document tab | Switches viewport ↔ script editor; picking the script tab enters the Script persona |
| Dock tab | Switches Output / Assets / Timeline; dock grows to 212 px for Timeline |
| Play | Toggles running; dock switches to Output and shows the runtime log; status bar shows `playing · 60 fps` |
| Zoom ± | 25 % steps, clamped 25–400 % |
| Theme button | Swaps the entire token set (see below); no layout change |
| ⌘K / Ctrl-K, command pill, Add component | Opens the palette |
| Esc / scrim / run | Closes the palette |
| Window < 1240 px | Persona labels, command-pill label and all right-aligned mono hints hide |
| Script chip `open` | Enters Script persona with that file |

No animation is specified beyond instant state changes and hover tints — appropriate for egui, which repaints per frame.

## State

```rust
struct EditorUi {
    theme: Theme,          // Dark | Light — default Dark
    persona: Persona,      // Scene | Script | Animate | Physics | Interface — default Scene
    selection: NodeId,     // default: Player
    tool: ToolId,          // default: Select
    document: DocumentTab, // Scene | Script | Events
    dock: DockTab,         // Output | Assets | Timeline
    palette: Option<String>, // Some(query) when open
    playing: bool,
    zoom: u32,             // percent, 25..=400, step 25
    compact: bool,         // derived from window width < 1240
}
```

All of it is view state — nothing here needs to survive a restart except `theme` and possibly the last persona.

## Design tokens

Two complete sets. Every surface reads from these; nothing hard-codes a color outside them. `#f9f4ed` is the fixed "on-accent" text color in both themes.

| Token | Role | Dark (default) | Light |
| --- | --- | --- | --- |
| `--e-bg` | window ground, viewport | `#17191c` | `#eef0f1` |
| `--e-panel` | panels, toolbars, dock | `#20242a` | `#e2e5e7` |
| `--e-sunken` | inset fields, pills, groups | `#101215` | `#d6dadc` |
| `--e-raised` | hover fill on transparent buttons | `#2b3037` | `#f9fafb` |
| `--e-line` | region seams, palette border | `#343a42` | `#c2c8cb` |
| `--e-line-soft` | inner rules, field borders | `#2b3037` | `#d8dcdf` |
| `--e-text` | primary text | `#eef1f4` | `#1d2124` |
| `--e-dim` | secondary text, inactive icons | `#b0b8c0` | `#586065` |
| `--e-faint` | tertiary text, meta, line numbers | `#767e88` | `#8f979c` |
| `--e-accent` | accent text/icons on tint | `#f0a273` | `#a85a2c` |
| `--e-accent-fill` | accent solid fills | `#d5814e` | `#c06a34` |
| `--e-accent-soft` | accent tint fills | `#3d2415` | `#f3e0d2` |
| `--e-sage` | second voice: scripts, physics, ok | `#8fbcae` | `#5f7f74` |
| `--e-sage-soft` | sage tint fills | `#22352f` | `#dde8e4` |
| `--e-grid` | viewport minor grid, 28 px | `#1e2126` | `#e3e6e8` |
| `--e-grid-major` | viewport major grid, 140 px | `#2b3037` | `#cfd4d7` |
| `--e-code-bg` | script editor ground | `#131518` | `#f9fafb` |

Syntax:

| Token | Role | Dark | Light |
| --- | --- | --- | --- |
| `--k-key` | keywords | `#f0a273` | `#8a4718` |
| `--k-str` | strings | `#8fbcae` | `#3f6459` |
| `--k-num` | numbers | `#ffc7a8` | `#a85a2c` |
| `--k-com` | comments | `#767e88` | `#8f979c` |
| `--k-fn` | identifiers, fn names | `#eef1f4` | `#22272b` |
| `--k-type` | types, builtins | `#b6d8cc` | `#5f7f74` |
| `--k-punc` | punctuation | `#98a1aa` | `#767e83` |

### Type

| Role | Family | Size |
| --- | --- | --- |
| Brand, persona pills, node/section headings, primary button | Display/heading face (`Caprasimo` in the prototype; substitute a rounded display face that ships as TTF for egui, or fall back to the UI face at weight 700) | 17 / 13 / 16 / 10 px |
| UI text | `Figtree` (any humanist sans) | 11–13.5 px |
| All values, code, paths, log, meta | `JetBrains Mono` | 10–12.5 px |

Section labels are 10 px, uppercase, letter-spacing 0.1em.

### Spacing, radius

4 px base: 4 / 8 / 12 / 16 / 24 px are the only gaps used. Radii: **999 px for nearly everything** (rows, pills, fields, buttons, chips, toggles, handles), `--radius-md` ≈ 12 px for tiles and rects in the viewport, `--radius-lg` ≈ 16 px for the palette card. **No shadows anywhere** — depth comes only from the panel/sunken/raised fill steps and 1 px lines.

### Fixed heights (memorize these — they define the density)

56 persona bar · 38 document tabs · 34 dock tabs · 28 status bar · 27 tree rows · 27 document tabs pills · 32 toolbar buttons and rail buttons · 29 secondary rows · 30 event/track rows · 28 inspector fields · 24 dock tab pills / 24 toggle · 40 palette rows · 26 viewport chips.

## Building this in egui

The design was constrained on purpose to what egui does natively:

- **No shadows, no blur, no gradients.** Every surface is a flat fill. Set `Shadow::NONE` throughout `Visuals`.
- **No overlapping floating panels.** The command palette is the single overlay, and it is a modal window with a scrim.
- **Fixed row heights everywhere** — map directly to `Frame` + `ui.set_min_height` or `ui.allocate_ui_with_layout`.
- **All rounding is either fully-round or a uniform corner radius**, both expressible with `Rounding`. For 999 px pills, use `Rounding::same(h / 2.0)`.
- **1 px separators only** — `Separator` or a `Frame` stroke; no double borders.
- Icons: load Lucide SVGs as textures, or embed an icon font. Keep them at 11 / 12 / 14 / 15 / 16 / 20 px per the specs above.

Suggested approach: build one `Theme` struct holding the token table, derive a full `egui::Visuals` from it (widget fills from `--e-sunken`/`--e-raised`, strokes from `--e-line-soft`, text from `--e-text`/`--e-dim`), and write small helpers — `pill_button`, `field_row`, `section_header`, `toggle`, `slider_row` — that every panel composes from. The design has only ~8 distinct widget shapes; the helpers are the whole implementation.

Two things to decide that the design deliberately leaves open:

1. **3D.** The skeleton is identical; the Scene persona gains axis/orbit tools, the axis pill becomes a gizmo, and the grid becomes a ground plane. Nothing else moves.
2. **Panel resizing.** The prototype pins the three widths. Making them draggable is expected; keep the minimums at roughly 220 / 40 / 280 px so no row's fixed content clips.

## Assets

None to hand over. The prototype uses no images. Icons are placeholders for Lucide (stroke width 2.75). Fonts are Google Fonts (Caprasimo, Figtree, JetBrains Mono) — vendor real font files for egui.

## Files

- `screenshots/` — one capture per persona (`01-scene`, `02-script`, `03-animate`, `04-physics`, `05-interface`), plus `06-command-palette` and `07-light-theme`. Captured at a narrow window, so they also show the compact behavior; the prototype is the authority on spacing at full width.
- `Editor.dc.html` — the full design prototype. Self-contained apart from the stylesheet below; open in a browser and interact with it.
- `styles.css` — the bound *Organic* design system's token sheet, which the prototype's palette is derived from. Reference only.

The prototype's logic is at the bottom of `Editor.dc.html`: `NODES`, `PERSONAS`, `TOOLS` and `CODE` are the sample data; `personaSections()` holds the per-persona inspector contents; `renderVals()` holds every derived color and state rule described above.
