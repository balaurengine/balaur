> **Status:** planned 2026-10-30, nothing built. Steps 9 to 11 of
> PLAN-foundations: after the derive is finished and the core is split.

# Plan: the ECS on hecs

What Bevy's `bevy_ecs` and `bevy_app` give, built into `balaur_core` on
hecs 0.11. Today a system is a `FnMut(&Engine, f32)` in one of eight stages,
run in registration order. Change tracking covers registry writes only;
events are named strings delivered a frame later; the one deferred command is
`Command::Free`; `Parent` and `Children` are kept by hand.

## 1. Why hecs can carry it

hecs hands a query a plain `&mut T` and has no hooks, so two rules do what
it does not.

- **One door for every write.** The `World` goes private behind `Engine`:
  spawn, insert, remove and despawn are `Engine` calls that run what hangs
  off them. On `editor-redesign`, Rust code wrote the world directly at
  `world_mut()` in 75 places in `balaur_render`, 49 in `balaur_core`, 21 in
  `balaur_physics`, 6 in `balaur_animation`, 4 in `balaur_input` and 4 in
  `balaur_ui`. Count again at step 1; the derive work moves some.
- **A tick on what is tracked.** A tracked component carries the tick it
  last changed, stamped from its `DerefMut`, as hecs's own `ChangeTracker`
  docs suggest. The derive writes it, so a component author marks a struct
  and nothing else.

## 2. What each feature is here

Names marked *(name)* wait on NAMING.md: a hook and a state already mean
something else in this engine.

- **Ticks.** One engine counter, stepped per system run, in the snapshot and
  restored with it. A reader keeps the tick it last ran at; "changed since"
  is a comparison. A `u64`, so it never wraps.
- **Change detection.** `changed` and `added` filters over a query, and the
  components removed this tick, recorded by the door. The change log reads
  ticks instead of being told by each writer.
- **Lifecycle calls** *(name)*. Bevy's component hooks: code run on add,
  insert, replace, remove and despawn, declared on the derive. They get a
  command queue and read access. They never call a script: anything a script
  should hear goes through `events::announce`'s queue.
- **Observers.** A Rust function run at once when an event is announced on
  one node or on any, carried up the tree when the event says so. Script
  listeners keep their delivery at the top of the next `Update`
  (PLAN-events).
- **Messages.** A typed Rust event, written by one system and read by others
  with a cursor each, kept two frames, beside the string events scripts use.
- **Commands.** Spawn, insert, remove, despawn and a closure over `Engine`,
  queued and applied in order at the end of each stage.
- **Relationships.** A pair of components the lifecycle calls keep
  consistent: set the one side, the other follows, and despawning a target
  can take its sources with it. The scene tree is the first: `Parent` and
  `Children` keep their names and their order, which the digest walks.
- **Required components.** A component that brings others with their
  defaults, declared on the derive, so a scene file, a script and Rust all
  get them.
- **Schedules.** Sets, `before`, `after` and run conditions on a system,
  sorted once when the app builds; a cycle is an error naming its systems.
  Systems with no order between them run in registration order. The eight
  stages stay the frame's spine.
- **App modes** *(name)*. Bevy's `States`: a value with enter and exit
  schedules, sub-modes that exist while a parent mode holds a value, and
  computed modes. In the digest and the snapshot.
- **Typed systems.** A Rust function whose parameters say what it reads and
  writes: a query with filters, a resource, `ResMut`, commands, a message
  reader or writer, a local. Borrowed through the engine's `RefCell`s, and a
  clash panics naming the system and the parameter. A plain
  `FnMut(&Engine, f32)` stays a system.
- **Parallel work inside a system.** Native only, over hecs's
  `iter_batched`: each entity's work alone, and any sum combined in batch
  order so the result is the same on every run. Serial on the web.

## 3. What every step keeps

- **The tick reproduces.** `scripts/determinism_trace.sh compare` stays
  clean, and a digest that moves says why in the commit.
- **A rollback restores what is new**: ticks, relationship pairs, queued
  messages and commands, either drained at the tick's edge or in the
  snapshot.
- **Scripts address components by registry index.** Nothing typed crosses
  the script seam.
- **Hot reload, the editor's undo and `balaur check`** keep working.
- **Costs are measured** against the numbers in PLAN-foundations §1: write
  and spawn time, memory per node (0.35 KB on the site's compare page), web
  bytes.

## 4. Steps

1. **The door.** `Engine` gains the structural calls and the world goes
   private to the kernel. A `world-write` house lint fails a `world_mut()`
   outside it, written first and reporting until the migration ends.
2. **The migration**, a crate a commit, render last as the largest.
3. **Ticks and change detection**, with the change log moved onto them.
4. **Lifecycle calls**, and `after_write`'s added and changed cases moved
   onto them.
5. **Commands.**
6. **Messages.**
7. **Observers**, inside `events::announce`.
8. **Relationships**, then the scene tree on them. The digest walks children
   in the same order, so no recording moves; a test proves it.
9. **Required components.**
10. **Schedules**: sets, order, run conditions, cycle errors.
11. **App modes.**
12. **Typed systems**, and the systems that read better with them moved over.
13. **Parallel work inside a system**, for what measures as hot: the render
    mirror's sync is the first candidate.

Each step brings its tests: behaviour a caller sees, a snapshot that
round-trips, and a determinism trace.

## 5. Done when

The scene tree runs on relationships and the change log on ticks. The render
mirror syncs only what changed, and no Rust writes the world around the door.
The costs in §3 are measured and written into ARCHITECTURE.
