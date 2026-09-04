# Plugins: one trait, one order, one switch

Status: **shipped.** All four phases are in — a plugin is one declaration in
`balaur`'s `modules!` table, ordered by requirement then name, switchable from
`[plugins]` in `project.toml`, and `balaur_plugin::Builtin` gives an
`App`-shaped plugin a manifest. ARCHITECTURE.md's plugin section is the
record. What is left is below.

## Open

- Around thirty registration helpers in `balaur_physics`, `balaur_render`,
  `balaur_anim` and `balaur_ui` still take `&mut App` rather than
  `&mut Registry`, which is the only reason `Registry::app()` is still reached.
  Converting them is what closes the gap between what a Rust plugin can do and
  what a C one can.
- `[plugins]` values are booleans, not tables of per-plugin config: audio,
  http and websocket each read their own file through `ProjectFiles` already,
  and a second place to configure them would be a second place to look.
