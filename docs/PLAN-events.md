# Plan: events, and one way to hear each of them

Every engine event goes out through `events::announce` and is heard as
`on_<name>(payload, source)`, a binding row, a listener or an await;
`ARCHITECTURE.md` "The seam" records the rules. Two pieces are left.

## Steps

1. **A snapshot restore records what it changed.** `snapshot::restore` in
   `crates/balaur_core/src/snapshot.rs` respawns, frees and rewrites nodes
   without an entry in the change log (`crates/balaur_core/src/changes.rs`),
   so a `changed` listener misses a rollback or a restore. It records one
   entry per node and component it changed, and still announces no tree
   events. Done: a test restores a snapshot under a `changed` listener and
   hears the nodes it put back.
2. **The Problems list runs the event-name checks.** `editor/scripts/lint.bs`
   asks `script::check` a file at a time, which gives compile errors only.
   `balaur check` also warns about a handler, listener, row or `emit` naming
   an event nothing announces (`crates/balaur/src/event_names.rs`), and those
   warnings never reach the editor. Done: the Problems list shows the same
   name warnings as `balaur check` for the open project.
