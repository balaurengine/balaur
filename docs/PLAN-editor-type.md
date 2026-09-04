# Plan: type and icons

> **Status:** built and verified 2026-09-03. The font chain, the icon set and
> the theme asset all shipped — `editor/themes/{dark,light}.toml` carry colour
> tokens plus a `roles` table a widget takes with `role:`, and all ten scripts
> in the `28-fonts` audit render. What is left is a chore and a gap in
> coverage, both below.

## The roles chore

The look is data, and a call site should say only what it changes. Counted
after the pass, these keys are still spelled at call sites:

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

## Coverage not measured

The fallback chain was measured on macOS: Japanese, Korean, Chinese, Arabic,
Hebrew, Devanagari and Thai all render, with nothing bundled for any of them.
The Windows and Linux stacks are chosen but have never been run.
