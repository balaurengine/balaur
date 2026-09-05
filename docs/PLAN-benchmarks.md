> **Status:** mostly built, 2026-09-05. `examples/benchmark` runs all twelve
> physics cases and the scene-tree ones, headless, in the editor and on the
> web; `scripts/bench_compare.py` merges the Godot suites' results into
> `docs/BENCHMARKS.md`, and the website syncs it. What is left is the
> raw-rapier twin (§ "The raw-rapier twin") and the criterion node cases.

# Plan: benchmarks beside Godot

## What "beside Godot" means

Two reference suites already exist and publish numbers. Both are reproduced
here case for case, so a row in our table is the same scene, the same counts,
the same protocol, measured on the same machine on the same day.

1. **Physics: `Ughuuu/benchmarks-repo`** — the suite behind the godot-rapier
   v0.35 post. Six cases per dimension, sleeping off, 60 Hz, 60 warmup ticks
   (90 where a pile has to settle), 300 timed steps, `step_ms` = wall time
   between physics ticks in a headless `--fixed-fps 60` run, reported as
   p50/p95/p99/mean/max. Engines: Rapier, Box2D v3, Godot Physics in 2D;
   Rapier, Jolt, Box3D, Godot Physics in 3D. A checkout with results from
   this machine (Apple M1, Godot 4.7-stable) is at
   `~/Documents/appsinacup/benchmarks-repo`; its `results/` is what the post
   quotes.

   | case | bodies | measures |
   |---|---:|---|
   | `pyramid` | 10,000 (2D) / 3,795 (3D) | solver throughput, box–box |
   | `mixed_pile` | 5,000 | narrow phase: box, ball, capsule, convex |
   | `joint_grid` | 5,000 + ~9,950 joints | constraints, with cycles |
   | `smash` | 5,001 | a heavy projectile into a wall |
   | `query_storm` | 8,000 static + 2,000 moving, ~750–900 queries/tick | the acceleration structure |
   | `drop` | 8,000 | a falling, colliding, settling pile |

2. **Nodes: `godotengine/godot-benchmarks`, `scene_nodes/`** — 50,000
   `add_child`, 50,000 `remove_child` in order / reversed / shuffled, 50,000
   `get_node` on a random tree, and 1,000×1,000 `move_child`. Each is one
   timed function; the number is milliseconds for the whole loop. Published
   daily at benchmarks.godotengine.org from a 12th-gen i5 on Linux, and
   runnable locally with the Godot at `/Applications/Godot.app`.

3. **The engine's own overhead.** The same physics case on the rapier Balaur
   compiles (0.35, `enhanced-determinism`, `parallel`, no SIMD) with no
   engine around it: no nodes, no components, no write-back, no script. The
   ratio is what a node-based engine costs over the library, and the
   fingerprint of the final positions is the parity check — same rapier,
   same inputs, same answer.

## Constraints

- **Runs everywhere the engine runs.** The cases are a Balaur project,
  `examples/benchmark`, in Rune: headless for the numbers, in the editor by
  opening and playing it, and on the web as a pack on a `/benchmark` page.
  One source for the scenes; only the raw-rapier twin (3) is Rust, in
  `crates/balaur_bench`, because it exists to have no engine in it.
- **The numbers come from the engine's profiler, not from a clock the
  runner keeps.** `engine::timings()` is the profiler's data path — the
  same table the editor's Profiler dock draws — published after every frame
  with the frame, `fixed_steps`, each stage and each named span. A case reads
  it and nothing else: the `fixed_update` stage divided by `fixed_steps` is
  the step as Godot measures it (script `_physics_process` plus the server's
  step plus node sync); a new span `physics3d/step` / `physics2d/step` around
  rapier's own step separates the library from what the engine wraps it in,
  and shows up in the dock as its own row the day it lands. Rune's
  instruction profiler (`RuneHost::script_costs`) is the other half, for a
  case whose cost is script rather than engine; a `scripts.costs()` reader is
  the one addition it needs. Wall time is presentation: nothing in a case
  branches on it.
- **Same protocol, same JSON.** Warmup, timed window, percentiles, work
  units, extra metrics and the FNV fingerprint are computed as
  `bench_main.gd` and `stats.gd` do, and written in the same schema, so the
  Godot suite's own report tooling could read ours.
- **Where the mirror is not exact, the page says so.** Godot's seeded RNG is
  not reproducible here; `mixed_pile`, `query_storm` and `drop` use the
  engine's `rng` with the same ranges, so those worlds are statistically, not
  bit, identical. Godot's 2D is y-down in pixels: y is mirrored, gravity is
  980 and `length_unit` is 100, godot-rapier's 2D default. Balaur's rapier has
  no SIMD; the addon's does. Every one of these is a line on the page.

## The engine changes it needs

1. **A joint on a bodiless child node ties the nearest body above it**, as a
   collider on a child already belongs to that body. `joint3d`/`joint2d` is
   one per node and a lattice needs two per body; today the second one has
   nowhere to live. `ends()` resolves the holder through `nearest_body`, the
   `expects` advisory stays, and a test pins it.
2. **`physics3d/step` and `physics2d/step` spans** around `step_with_events`,
   through `timings::measure`, so a script can subtract the step from the
   stage — and the Profiler dock lists them with no change of its own.
3. **`balaur run <project> -- <args>`** hands trailing arguments to
   `engine::args()`, as `edit` already does for its own. Headless runs pick
   the case that way: `-- --case=3d/pyramid --steps=300`.

## The project: `examples/benchmark`

- `scenes/main.toml`: one node with `scripts/bench.rn`. Nothing else is
  authored; every case builds its world in script and frees it after.
- `scripts/bench.rn`: the runner. A menu (`ui`) listing the cases, a
  draw-shapes toggle (on with a window, off headless: shapes are the viewer's,
  not the measurement's), a run button, and a results table. The protocol:
  build a chunk of bodies per frame, warm up `max(60, settle_ticks)` ticks,
  time 300 steps
  reading `engine::timings()` each frame, then report p50/p95/p99/mean/max
  of `step` (fixed stage per step), `physics` (the span), and `frame`. Headless
  it prints one `BENCH <json>` line and quits.
- `scripts/cases3d.rn`, `scripts/cases2d.rn`: the six cases each,
  written against the same three helpers the GDScript has — `body(at,
  shape)`, `fixed(at, shape)`, `pin(a, b)` — so a case reads as the scene it
  describes. `query_storm` runs its rays, casts and point queries from
  `fixed_update`, the seam GDScript pays too.
- `scripts/nodes.rn`: the scene_nodes cases, each one frame of work whose
  cost is read from the stage it lands in: `add_child` and `get_node` from
  `scripts/update`; `queue_free` from the deferred-destruction stage, since a
  script cannot free a node synchronously. `move_child` has no counterpart:
  the tree has no sibling-reorder operation, which is a gap the page names.
- Extra metrics per case as the originals: `spread`/`non_finite` for
  `smash`, `displacement_max` for `joint_grid`, `queries`/`hits` for
  `query_storm`; the `mixed_pile` hull is an inline `[[assets]]` mesh in the
  scene.

## The raw-rapier twin: `crates/balaur_bench`

- `src/cases3.rs`, `src/cases2.rs`: the same six cases on
  `rapier3d::pipeline::PhysicsWorld` / `rapier2d`, sleeping off, the same
  thread count the plugin picks, the same `length_unit`.
- `src/bin/physics.rs`: `--dim 3d --case pyramid --steps 300 --warmup 60
  --out file.json`; one case per process, as `run_bench.sh` does, so a warm
  allocator does not flatter the next case. Same JSON, same fingerprint.
- `benches/engine.rs` gains the node cases in Rust (`add_children/50000`,
  `delete_children/{in_order,reverse,random}/50000`, `get_node/50000`) for
  the budget gate; the page quotes the Rune numbers, which carry the seam.

## The driver: `scripts/bench_compare.py`

Runs every case headless, one process each, reads the Godot suite's
`results/` (`--godot-results DIR`) and a godot-benchmarks JSON for the node
rows (`--godot-nodes FILE`), and writes `docs/BENCHMARKS.md`: machine, engine
commit, Godot version, date; a summary table of our tick beside the quickest
Godot engine's; one table per case with our step, rapier's step inside it, the
script seam, and every Godot engine at p50 and p99; the extra metrics and the
fingerprint; the caveats above. `--no-run` rewrites the report from the last
run's JSON. The file is committed from a run, like `CHANGELOG.md`, never
generated in CI: a shared runner's numbers are noise, and `budgets.toml`
already gates orders of magnitude.

Not built: cores used and peak resident memory, which the Godot driver takes
from `/usr/bin/time -l`.

## The website

- `sync-docs.sh` fetches `docs/BENCHMARKS.md` into `docs/benchmarks.md` with
  front matter, as it does the changelog; the docs sidebar lists it after
  Crates. The pictures the report points at — one screenshot per physics
  case, taken offscreen at the first timed tick with `--shot`, and a bar
  chart per dimension — are written by the driver straight into the site's
  `static/img/benchmarks/`, as `showcase.sh` does for the manual.
- `package_play.sh` packs `examples/benchmark`; `/benchmark` is the play
  page pointed at that pack, with the note that a browser build is
  single-threaded and vsynced, so its numbers describe the browser, not the
  machine.

## Phases

1. **Done.** The three engine changes, with tests. A fourth followed from
   the first measurement: `node_pose` propagated the whole scene tree on
   every body created, which made building a world quadratic; it composes
   from the node's own ancestors now.
2. **Done.** The project: runner, the twelve physics cases, the scene-tree
   cases, headless runs of each.
3. **Partly done.** The driver and `docs/BENCHMARKS.md` from a full run on
   this machine beside the Godot results. The raw-rapier twin is not built:
   the `physics3d/step` span already separates rapier from the engine
   in-process, so the twin's remaining value is checking that our rapier is
   configured as the addon's is.
4. **Partly done.** The node cases run in Rune; the criterion ones and the
   budget rows for them are not written.
5. **Done.** The website: `sync-docs.sh` fetches the report, the docs
   sidebar lists it, `package_play.sh` packs the project and `/benchmark`
   runs it in a browser.
6. Later, under `docs/PLAN-release.md`: the same run on a pinned runner per
   tag.

## Open questions

1. **`joint_grid` on a Godot engine that collapses** — the originals report
   `displacement_max` and mark the run unstable rather than fast. Ours does
   the same; a Balaur row with a large displacement is a solver question, not
   a benchmark one.
2. **What the web number means.** Rayon has no threads on
   `wasm32-unknown-unknown`, and the browser ticks at the display's rate; the
   page reports the step and says which build it came from.
3. **`move_child`.** Adding a sibling-reorder operation is an API question
   (`docs/NAMING.md` N9) before it is a benchmark one; the row stays empty
   until it exists.
4. **Freeing nodes is quadratic**, which the first run of the scene-tree
   cases showed: fifty thousand `queue_free` calls take about half a second
   against Godot's ten milliseconds, because `scene::free_subtree` unlinks
   each node from its parent with a `retain` over every sibling. Its own
   plan, not this one.
5. **Fixed by the first run:** `raycast` walked every collider along the
   ray with no distance pruning and sorted the lot — `intersect_ray` where
   `cast_ray_and_get_normal` was meant — which put the 2D query storm at
   12.45 ms; it is 5.6 ms now. The 2D step also never marked the broad phase
   fresh, so a query after any collider insertion rebuilt it a second time.
6. **The joint grid sags about twice as far as the addon's.** Same rapier
   version, same joint kind, same pitch; the addon's build has SIMD and ours
   does not, which is the first thing to rule out.
