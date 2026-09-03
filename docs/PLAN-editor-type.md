# Plan: type and icons

> **Status:** built and verified 2026-09-03. All ten scripts render — the
> `.ttc` system collections do parse, which was the open risk — and the
> coverage sheet is `28-fonts` in the audit. The look now lives in
> `editor/themes/{dark,light}.toml`: colour tokens plus a `roles` table a
> widget takes with `role:`. About a quarter of the call-site overrides are
> gone; the tail is counted in §6.
>
> Superseded status: chosen and built, not yet verified. Text option A and
> Phosphor were picked on 2026-09-03; the faces are vendored in
> `editor/fonts/`, the loader builds explicit chains with per-platform system
> fallbacks, `editor/scripts/icons.rn` holds every codepoint, and `fontdemo`
> renders a coverage sheet. None of it has been seen on screen: the workspace
> does not compile while `balaur_script_rune` is being refactored elsewhere.
> First run must check the tofu rows in `28-fonts` and that `.ttc` system
> collections parse at all — that is the one real risk in the loader.
>
> Originally a specification, nothing chosen. Written 2026-09-03 because the
> shell does not match its own mockup and the reason turned out to be that it
> ships no fonts at all. Belongs with
> [PLAN-editor-redesign.md](PLAN-editor-redesign.md) §4.3, which asked for an
> icon font and stopped there.

## 0. Where we are, measured

`editor/fonts/` does not exist. `crates/balaur_ui/src/theme.rs:126` scans
`<project>/fonts/*.ttf`, finds nothing, and every named family falls through
to egui's built-ins:

| family | intended | resolves to today |
|---|---|---|
| `heading` | a display face | egui's default proportional |
| `ui` | a humanist sans | the same face |
| `mono` | JetBrains Mono | egui's default monospace |

So `heading` and `ui` are the same font at the same weight. `strong: true`
fakes a heavier stroke on a Light face; that is the whole type hierarchy.

**Script coverage, tested** — node names renamed and captured
(`target/uiaudit/i18n.png`):

| script | sample | today |
|---|---|---|
| Latin | `Plataforma` | renders |
| Cyrillic | `Вращатель` | renders |
| Greek | — | renders (same face) |
| Japanese | `ボール` | **tofu** |
| Chinese | `平台` | **tofu** |
| Arabic | `لوحة` | **tofu** |
| Hebrew, Devanagari, Thai | — | **tofu** |

The engine already localises: `strings/<locale>.toml`, `strings::set_locale`,
`[locale]` in `project.toml`, a fallback locale. A game can ship Japanese
strings today and the widget layer will draw them as boxes.

macOS has the fonts to fix this already installed — `Hiragino Sans GB`,
`AppleSDGothicNeo`, `STHeiti`, `GeezaPro`, `Kohinoor*`, `Arial Unicode` — and
the loader appends only `Apple Symbols`, which carries icons and no CJK.

## 1. The architecture: a chain, not a font

egui resolves **per glyph** down a family's chain. Coverage is therefore a
list, not a file, and the decision splits in two:

- **What we ship** decides how the editor *looks*. Latin, Greek and Cyrillic,
  a few hundred KB, under our control.
- **What we chain to** decides what the editor can *display*. Every other
  script, from the operating system, costing nothing to ship.

Never ship CJK. A single weight of Noto Sans CJK is 16 MB or more, and every
desktop already has one.

    heading: [display] → [ui face] → [system UI stack] → egui default
    ui:      [ui face] → [system UI stack] → egui default
    mono:    [mono face] → [system mono stack] → egui default

| platform | system UI stack | system mono |
|---|---|---|
| macOS | PingFang SC, Hiragino Sans, AppleSDGothicNeo, GeezaPro, Kohinoor*, Apple Symbols | Menlo |
| Windows | Segoe UI, Yu Gothic UI, Malgun Gothic, Microsoft YaHei, Segoe UI Symbol | Consolas |
| Linux | fontconfig query, then Noto Sans CJK / DejaVu Sans | DejaVu Sans Mono |

Three rules that follow:

1. A project's own `<project>/fonts/*.ttf` goes at the **front** of the chain,
   so a Japanese game can ship the face it wants without patching the editor.
2. The chain a file joins must be explicit. Today the loader guesses from the
   filename — `caprasimo` or `display` means heading, anything else means UI —
   so dropping in `Alegreya-Bold.ttf` would silently land in the UI chain.
   Name the files `heading-*.ttf`, `ui-*.ttf`, `mono-*.ttf`, or read a
   `[fonts]` table from `project.toml`.
3. A missing glyph must be visible in CI. A `fontdemo` state renders one row
   per script and the audit captures it; tofu in the screenshot is the test.

## 2. Text — the options

All five are OFL or equivalent and free to vendor. Sizes are per weight, to
confirm when vendoring; shipping two weights of the UI face and one of the
rest is the realistic budget.

### A. Alegreya + Source Sans 3 + JetBrains Mono

What the mockup was actually running, and what the balaur website already
uses. Alegreya is a **serif** — in the mockup it carried only the brand, the
node name and the section headings, which is what made those read as
headings against the sans rows.

- Coverage: Latin, Greek, Cyrillic across all three. No Arabic/CJK — chained.
- Weights: Alegreya 500/700, Source Sans 3 400/600, JetBrains Mono 400.
- **For:** one identity across site and editor, and a display face that is
  genuinely a different voice rather than a heavier weight.
- **Against:** a serif in a dark tool UI is a strong opinion. It works in
  small doses and fails if it leaks into rows.

### B. Figtree + a rounded display + JetBrains Mono

What `design_handoff_balaur_editor/README.md` specified before the palette
moved. All-sans, conventional, safe.

- Coverage: Latin, Cyrillic (Figtree has no Greek).
- **For:** reads as a tool; nothing to get wrong.
- **Against:** the heading face is a weight, not a voice; loses the website tie.

### C. IBM Plex Sans + Plex Mono + Plex Serif

A superfamily built for exactly this problem: one designer's Latin, Cyrillic,
Greek, Arabic, Devanagari, Hebrew and Thai, plus separate JP/KR/SC/TC
families that can be chained instead of the system's.

- **For:** the widest first-party coverage of any option; the serif and mono
  are siblings of the sans, so the three voices are related by design.
  If balaur ever wants a *consistent* look across languages rather than
  whatever the OS supplies, this is the only option that gets there.
- **Against:** Plex is strongly associated with IBM; heavier vendoring if the
  non-Latin families come along.

### D. Inter + Inter Display + JetBrains Mono

The default answer for a modern tool UI.

- Coverage: Latin, Greek, Cyrillic. Inter Display is the same skeleton
  optically sized, not a different voice.
- **For:** neutral, extremely legible at 11–13 px, huge weight range.
- **Against:** it is the house style of every other tool; no identity.

### E. Ship nothing, chain to the system

Zero download, perfect coverage, and the editor looks native on each OS.

- **For:** the only option with no licence, size or CI cost at all.
- **Against:** three different-looking editors, no control over the mockup's
  hierarchy, and it is what we have now — which is the complaint.

## 3. Icons — the options

The shell's icons are Unicode symbols in strings today (`defs::tool_icon`,
`palette::commands`, the dock tabs), resolved through whatever the fallback
chain has. That is why the Translate tool is an empty box and `⌥` prints as
`~`. Any icon font keeps the same model — a string in, a glyph out — so the
migration is a codepoint table, not an API change.

| | Icons | Licence | Model | Notes |
|---|---|---|---|---|
| **Phosphor** | ~9 000, six weights | MIT | official TTF, PUA | Weights let the icon stroke match the type weight. The largest official icon font. |
| **Lucide** | ~1 500 | ISC | SVG-first; community TTF builds | What the design handoff specified (stroke 2.75). Consistent 2 px stroke, no weight axis. |
| **Material Symbols** | ~3 600, three styles | Apache 2.0 | official **variable** TTF | One file, axes for weight/fill/optical size. Heaviest single file; Google's visual language. |
| **Nerd Fonts** | thousands, merged sets | per set (MIT/OFL) | patches a base font, PUA | This is the "pack a lot into Unicode" idea: it patches a *mono* face so the log and code get icons too. Overkill for a UI, and the icon styles are not a coherent set. |
| **Stay on Unicode** | what the OS has | — | — | Audit every glyph, keep only what the chain resolves. No download; different on every platform, which is how we got here. |

### The spec, whichever is chosen

- **One file, one weight/stroke.** The shell has one icon voice. A second
  weight is a decision, not a convenience.
- **`editor/scripts/icons.rn` is the only place a codepoint appears.** It maps
  a semantic name to a glyph: `icons::tool("move")`, `icons::panel("assets")`,
  `icons::key("alt")`. Swapping the font is then one file.
- **Sizes: 11 / 12 / 14 / 16 design px**, matching the existing call sites. No
  other size without a reason.
- **Colour comes from the row**, never from the icon: `k.dim` inactive,
  `k.accent` active, `k.on_accent` on a filled row.
- **Modifier keys are type, not icons** — `⌘ ⌥ ⇧ ⌃` must come from the *mono*
  chain, which needs a face that has them. This is a separate hole from the
  icon set and the palette shows it today.
- **A missing glyph is a test failure.** `fontdemo` renders every icon
  `icons.rn` names; a box in the screenshot fails the audit.

## 4. Chosen

**Text A** and **Phosphor**, on 2026-09-03. Vendored, 1.8 MB total:
`heading-Alegreya-Bold.ttf` (a wght 700 static cut from Google Fonts'
variable Alegreya, since upstream ships no statics), `ui-SourceSans3-Regular`
and `-Semibold`, `mono-JetBrainsMono-Regular`, `icons-Phosphor`.

Noto was asked about and not chosen. It is the right thing to *fall back to*
and the right thing for a **game** to bundle, but the wrong thing for the
editor's own voice: Noto Sans is designed to be invisible so that no script
looks out of place beside another, and choosing it would not avoid the chain
anyway — Noto is per-script files, so Noto CJK is still 16 MB+ that every
desktop already has. On Linux the system fallback usually *is* Noto.

### The recommendation as written

**Text: A**, with a rule. Alegreya for the brand, the inspector's node name
and section labels only — never a row, never a value. Source Sans 3 for every
UI string, JetBrains Mono for values, paths, code and the log. It is what the
mockup was running, it is the website's identity, and the serif is what gives
the shell a voice at all.

If a single-vendor look across every language matters more than that identity,
**C** is the only option that reaches it, and the choice should be made now
rather than after the codepoint tables are written.

**Icons: Phosphor**, at the weight that matches the UI face. Lucide is the
handoff's choice and would also be right; Phosphor wins on having an official
font build and a weight axis to tune against the type. Either way §3's spec
holds, and `icons.rn` makes the other one a one-file change.

**The chain (§1) happens regardless of both.** It is what fixes the tofu, and
it is worth doing before either font lands.

## 5. What changes

| Where | Change |
|---|---|
| `crates/balaur_ui/src/theme.rs` | explicit family naming; per-platform system fallback stacks; project fonts to the front of the chain |
| `editor/fonts/` | the vendored faces, named for the chain they join |
| `editor/scripts/icons.rn` | new: semantic name → codepoint, the only file with a glyph in it |
| `editor/scripts/defs.rn`, `palette.rn`, `docks.rn` | call `icons::*` instead of spelling a glyph |
| `editor/scripts/selftest.rn` | `fontdemo`: a coverage sheet, captured by `scripts/uiaudit.sh` |
| `THIRD-PARTY-NOTICES.md` | the vendored fonts' licences |

## 6a. The quiet pass

Affinity's rule is that the document is the only thing with colour. Applied,
almost entirely in the theme file:

- accent is now the tree selection (solid), the active tab and persona
  (`accent_soft`), and the one call to action. Solid `accent_fill` appears
  once on screen instead of five times.
- fields lost their stroke: a recessed fill is the affordance.
- sheets: radius 11 → 8, stroke `line` → `line_soft`.
- rows 25 → 23, list rows 27 → 25, tabs 24 → 22, fields 24 → 22, the persona
  bar 44 → 40, document tabs 36 → 34.

Two script edits were needed — the persona pill and the tab-row height — and
everything else was TOML, which is the argument for the theme asset making
its own case.

## 6. The theme asset, and what is left

The look is data: `editor/themes/dark.toml` and `light.toml`, `type =
"editor_theme"`, a `[colors]` table and a `[roles.*]` table. A role is the
option map a widget would otherwise have been given — `field`, `tab`,
`row_on`, `chip`, `button`, `heading`, `mono` — with colour entries written as
token names and resolved when the theme is set. `theme.rn` only loads one.

`Opts::with_roles` merges a role under the caller's options, so a call site
says only what it changes. `ui::set_theme` takes the nested table; the widget
layer's own `themes/*.toml` (`type = "widget_theme"`) is unchanged and still
governs a *game's* widgets.

**Still spelled at call sites**, counted after the pass:

| key | left |
|---|---|
| `height` | 127 |
| `size` | 79 |
| `fill` | 78 |
| `strong` | 49 |
| `stroke` | 23 |
| `radius` | 22 |
| `font` | 7 |

Most of the `height` and `fill` remainder is layout rather than look — a row
sized to its container, a fill chosen from live state. The rest wants more
roles, not more overrides: every one of those is a look that should have a
name. Adding a role is a line of TOML, so this is a chore, not a design
question.

## 7. Fallback coverage, measured

`28-fonts` after the chain landed:

| script | before | after |
|---|---|---|
| Latin, Greek, Cyrillic | renders | renders |
| Japanese, Korean, Chinese | tofu | renders |
| Arabic, Hebrew, Devanagari | tofu | renders |
| Thai | tofu | renders (needed `ThonburiUI.ttc` adding) |

Nothing was bundled for any of them. Windows and Linux stacks are listed in
§1 and have not been run.
