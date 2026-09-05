<!-- Written by scripts/bench_compare.py from a real run. -->

# Benchmarks

Balaur `0108981c0` on Apple M1, 8 cores, Darwin 25.6.0, 2026-09-05.

Every case is the scene the reference suite builds, with the same counts, the same 60 Hz tick and the same window: a settle, then 300 timed steps, reported as the median and the 99th percentile of one tick.

**step p50** is a whole physics tick: script `fixed_update`, the solver, and writing every simulated pose back to the scene tree — what the Godot suite calls `step_ms`. **rapier's own step** is the solver alone, and the gap between the two is what the engine adds. **script seam** is what the case's own script cost inside that tick.

Across the 12 physics cases the engine itself costs 1% to 13% of a tick; the rest is rapier, and in the query cases the script's own calls.

Median tick, lower better:

| case | bodies | Balaur | quickest in Godot | that engine's tick, over ours |
| --- | ---: | ---: | --- | ---: |
| `3d/pyramid` | 3795 | 10.43 ms | Box3D 13.52 ms | 1.30x |
| `3d/mixed_pile` | 5000 | 5.04 ms | Box3D 9.72 ms | 1.93x |
| `3d/joint_grid` | 5000 | 3.08 ms | Rapier 3D 8.92 ms | 2.90x |
| `3d/smash` | 5001 | 7.99 ms | Box3D 9.62 ms | 1.20x |
| `3d/query_storm` | 2000 | 5.50 ms | Rapier 3D 5.93 ms | 1.08x |
| `3d/drop` | 8000 | 3.06 ms | Box3D 11.34 ms | 3.70x |
| `2d/pyramid` | 10000 | 7.10 ms | Rapier 2D 20.03 ms | 2.82x |
| `2d/mixed_pile` | 5000 | 2.86 ms | Box2D v3 6.71 ms | 2.35x |
| `2d/joint_grid` | 5000 | 2.17 ms | Box2D v3 4.93 ms | 2.27x |
| `2d/smash` | 5001 | 3.50 ms | Box2D v3 11.57 ms | 3.31x |
| `2d/query_storm` | 2000 | 12.45 ms | Box2D v3 4.56 ms | 0.37x |
| `2d/drop` | 8000 | 6.07 ms | Rapier 2D 16.54 ms | 2.73x |

The one case Balaur loses is `2d/query_storm`, and the table below says why: nine hundred queries a tick cost 11 ms crossing the script seam and 1 ms inside rapier. A game asking that many questions per tick from script pays for the questions, not for the broad phase.

The Godot columns are 4.7-stable (official) with the addons the reference suite commits, run on this same machine.

## 3D

### `pyramid` (3d)

3795 bodies. 300 timed steps after 60.

| engine | step p50 | step p99 | rapier's own step | script seam |
| --- | ---: | ---: | ---: | ---: |
| **Balaur** | 10.43 | 15.28 | 10.30 | 0.002 |
| Box3D | 13.52 | 33.12 | — | — |
| Rapier 3D | 14.62 | 34.17 | — | — |
| Jolt | 26.92 | 106.63 | — | — |
| Godot Physics 3D | 48.03 | 133.30 | — | — |

### `mixed_pile` (3d)

5000 bodies, 5 static. 300 timed steps after 60.

| engine | step p50 | step p99 | rapier's own step | script seam |
| --- | ---: | ---: | ---: | ---: |
| **Balaur** | 5.04 | 7.41 | 4.87 | 0.003 |
| Box3D | 9.72 | 17.16 | — | — |
| Rapier 3D | 11.28 | 34.73 | — | — |
| Jolt | 13.70 | 37.14 | — | — |
| Godot Physics 3D | 84.90 | 149.55 | — | — |

### `joint_grid` (3d)

5000 bodies, 9950 joints. 300 timed steps after 60.

| engine | step p50 | step p99 | rapier's own step | script seam |
| --- | ---: | ---: | ---: | ---: |
| **Balaur** | 3.08 | 4.40 | 2.82 | 0.002 |
| Rapier 3D | 8.92 | 14.13 | — | — |
| Box3D | 10.27 | 17.44 | — | — |
| Godot Physics 3D | 26.08 | 52.31 | — | — |

Balaur: displacement_max 49.58. Fingerprint `cc96e435`.

### `smash` (3d)

5001 bodies. 300 timed steps after 60.

| engine | step p50 | step p99 | rapier's own step | script seam |
| --- | ---: | ---: | ---: | ---: |
| **Balaur** | 7.99 | 10.68 | 7.81 | 0.003 |
| Box3D | 9.62 | 14.37 | — | — |
| Jolt | 12.12 | 29.06 | — | — |
| Rapier 3D | 19.14 | 25.05 | — | — |
| Godot Physics 3D | 99.98 | 201.82 | — | — |

Balaur: non_finite 0, spread 430.5. Fingerprint `8a46de70`.

### `query_storm` (3d)

2000 bodies, 8000 static. 300 timed steps after 60.

| engine | step p50 | step p99 | rapier's own step | script seam |
| --- | ---: | ---: | ---: | ---: |
| **Balaur** | 5.50 | 6.03 | 1.12 | 4.09 |
| Rapier 3D | 5.93 | 6.80 | — | — |
| Box3D | 6.67 | 8.86 | — | — |
| Jolt | 26.45 | 34.11 | — | — |
| Godot Physics 3D | 73.26 | 86.42 | — | — |

Balaur: hits 201175, queries 270000. Fingerprint `dc3e3b83`.

### `drop` (3d)

8000 bodies, 5 static. 300 timed steps after 60.

| engine | step p50 | step p99 | rapier's own step | script seam |
| --- | ---: | ---: | ---: | ---: |
| **Balaur** | 3.06 | 4.24 | 2.77 | 0.002 |
| Box3D | 11.34 | 41.58 | — | — |
| Rapier 3D | 12.26 | 18.59 | — | — |
| Jolt | 22.14 | 37.79 | — | — |

## 2D

### `pyramid` (2d)

10000 bodies. 300 timed steps after 60.

| engine | step p50 | step p99 | rapier's own step | script seam |
| --- | ---: | ---: | ---: | ---: |
| **Balaur** | 7.10 | 8.53 | 6.71 | 0.002 |
| Rapier 2D | 20.03 | 28.12 | — | — |
| Box2D v3 | 21.33 | 52.38 | — | — |
| Godot Physics 2D | 147.26 | 442.54 | — | — |

### `mixed_pile` (2d)

5000 bodies, 3 static. 300 timed steps after 60.

| engine | step p50 | step p99 | rapier's own step | script seam |
| --- | ---: | ---: | ---: | ---: |
| **Balaur** | 2.86 | 4.69 | 2.60 | 0.002 |
| Box2D v3 | 6.71 | 11.73 | — | — |
| Rapier 2D | 9.25 | 15.28 | — | — |
| Godot Physics 2D | 71.12 | 136.33 | — | — |

### `joint_grid` (2d)

5000 bodies, 9950 joints. 300 timed steps after 60.

| engine | step p50 | step p99 | rapier's own step | script seam |
| --- | ---: | ---: | ---: | ---: |
| **Balaur** | 2.17 | 2.75 | 1.88 | 0.002 |
| Box2D v3 | 4.93 | 8.75 | — | — |
| Rapier 2D | 10.16 | 19.44 | — | — |

Balaur: displacement_max 190.9. Fingerprint `f779b873`.

### `smash` (2d)

5001 bodies. 300 timed steps after 60.

| engine | step p50 | step p99 | rapier's own step | script seam |
| --- | ---: | ---: | ---: | ---: |
| **Balaur** | 3.50 | 4.39 | 3.29 | 0.002 |
| Box2D v3 | 11.57 | 28.63 | — | — |
| Rapier 2D | 11.86 | 21.93 | — | — |
| Godot Physics 2D | 55.09 | 104.59 | — | — |

Balaur: non_finite 0, spread 2999. Fingerprint `7c549757`.

### `query_storm` (2d)

2000 bodies, 8000 static. 300 timed steps after 60.

| engine | step p50 | step p99 | rapier's own step | script seam |
| --- | ---: | ---: | ---: | ---: |
| Box2D v3 | 4.56 | 5.49 | — | — |
| Rapier 2D | 6.25 | 7.70 | — | — |
| **Balaur** | 12.45 | 15.57 | 1.14 | 11.03 |
| Godot Physics 2D | 57.24 | 67.48 | — | — |

Balaur: hits 280111, queries 324000. Fingerprint `5ff47d2c`.

### `drop` (2d)

8000 bodies, 3 static. 300 timed steps after 60.

| engine | step p50 | step p99 | rapier's own step | script seam |
| --- | ---: | ---: | ---: | ---: |
| **Balaur** | 6.07 | 9.75 | 5.75 | 0.003 |
| Rapier 2D | 16.54 | 31.08 | — | — |
| Box2D v3 | 22.66 | 46.56 | — | — |
| Godot Physics 2D | 103.34 | 286.81 | — | — |

## Nodes

### Scene tree

Milliseconds for the whole loop, lower better. Godot's column is its own published run on another machine, so it says which operations are in a different class, not how the two machines compare.

| operation | Balaur p50 | Balaur min | Godot release |
| --- | ---: | ---: | ---: |
| `add_children` | 46.18 | 45.85 | 28.19 |
| `delete_children_in_order` | 543.27 | 542.12 | 9.82 |
| `delete_children_reverse` | 463.37 | 461.49 | 6.10 |
| `delete_children_random` | 513.89 | 512.12 | 19.63 |
| `get_node` | 66.98 | 66.35 | 29.60 |

`move_child` has no row: the scene tree has no sibling-reorder operation to measure.

Destroying is two orders of magnitude off, and it is the scene tree, not the script: `scene::free_subtree` unlinks each node from its parent by scanning every sibling, so freeing a flat container of fifty thousand is quadratic. Adding and looking up are within a small factor of Godot's on a faster machine.

## What is not the same

- Balaur's rapier is built without SIMD; the godot-rapier addon's is built with it. Same version, same `enhanced-determinism`, same threaded solver.

- The worlds are statistically alike, not bit-identical: the scattered cases draw from each engine's own random stream, so a fingerprint compares two Balaur runs, never Balaur against Godot.

- The 2D cases are the GDScript's coordinates with y negated, because Godot's 2D plane points down and ours points up. Distances, contacts and gravity are unchanged, and `length_unit` is 100 as godot-rapier's 2D default is.

- `mixed_pile`'s convex hull is one fixed size rather than a random one: a hull collider is built from a mesh asset and has no scale.

- Godot's own suite runs its scene-tree cases on its CI machine, not this one.

## Running it

```bash
cargo build --release -p balaur_cli --bin balaur
python3 scripts/bench_compare.py --godot-results <benchmarks-repo>/results
```

One case on its own, or in the editor:

```bash
target/release/balaur run examples/benchmark --headless --fixed-tick -- --case=3d/pyramid
target/release/balaur edit examples/benchmark
```

