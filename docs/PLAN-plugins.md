# Plugins: one trait, one order, one switch

Status: **shipped.** All four phases are in — a plugin is one declaration in
`balaur`'s `modules!` table, implements one trait, is ordered by requirement
then name, and is switchable from `[plugins]` in `project.toml`.
ARCHITECTURE.md's plugin section is the record. What is left is below.

## Open

- Tiers above 1 of `docs/PLAN-c-api.md` — components, systems and calling back
  into script across the C boundary. `Registry` is now the whole surface a
  plugin registers through and `app()` is gone, so that list is exactly what a
  C extension is still short of, with nothing left to reach around it.
- A plugin can now be configured in two places — its own file through
  `ProjectFiles`, or a `[plugins]` table through `Registry::config` — and
  nothing in tree reads the second yet. Which one each plugin should prefer is
  worth settling before one grows both.
