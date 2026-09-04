> **Status:** all thirteen phases shipped on 2026-09-03 in
> `crates/balaur_physics`; see [generated/](generated/) for what the code
> does. What the build left open is below.

# Plan: the rest of Rapier — what is still open

1. **Script hooks on a parallel build.** rapier calls hooks from worker
   threads under `parallel`, so a hook that talks to Rune cannot run there.
   One design lifts the exclusion at the price of one step of latency: the
   `Sync` hook reads a `DetHashMap<(handle, handle), Decision>` filled on the
   main thread, and after each step the plugin asks the script about the
   pairs the hook saw for the first time, so the answer applies from the next
   step. A new pair defaults to *allow* for one step. Whether that latency is
   acceptable is a question for the first game that needs both; until then
   the two are exclusive and say so.
2. **Solver tuning in a recording's header.** Both halves are built — a
   `[physics]` table read once after the project loads, and
   `physics.set_tuning` for a game that tunes at run time. What is open is the
   recording: until its header carries the tuning, a replay trusts that the
   manifest has not changed since.
3. **Layer names.** `layers = ["0", "3"]` is honest and unreadable. Godot
   names layers in project settings and the inspector relabels the
   checkboxes. That needs schema options resolved at inspector time rather
   than at registration, which no other component wants; it can wait until
   someone has shipped a game with 30 layers.
4. **`f64` for the whole engine.** Phase 13 converts at the physics seam. If
   a game needs `f64` in `Transform` too — a solar system, not a large map —
   that is glamx's `D*` types through every crate, and a plan of its own.

One question is advice rather than code: in 2D a `heightfield` is a polyline
over a height array, which is a side-scroller's ground. Whether that beats
authoring a `polygon` is for whoever builds the second 2D example — both are
built.
