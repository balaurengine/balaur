# Plan: what rapier still has that no script reaches

The physics work left in `crates/balaur_physics` beyond the per-component
settings, which [PLAN-wrapped-surface.md](PLAN-wrapped-surface.md) holds.

## Steps

1. **Named collision layers.** Layer names set once in the project settings,
   and the inspector's layer checkboxes labelled with them. The inspector has
   to resolve a row's options when it draws, not when the schema registers.
   Done when a project that names a layer sees that name on every collider's
   layer row.
2. **The event collector under `parallel`.** `shared/events.rs`'s `Collector`
   takes a `Mutex` per event, since handlers run on rapier's threads. Measure
   with `cargo bench -p balaur_bench --bench engine -- physics_step` on a scene
   that hears contact forces on everything. If the lock shows, collect into
   per-thread buffers merged after the step; `take()` already sorts by a total
   key, so the order holds.
3. **The query pipeline's gaps.** `physics3d` and `physics2d` reach none of
   `cast_shape_nonlinear`, `project_point_and_get_feature` or
   `ShapeCastOptions.target_distance`. `shapecast` sweeps an unrotated shape,
   since `query.rs` builds its start pose from a translation only. Done when a
   script reaches each, in both dimensions.

## Open questions

1. **Script hooks on a threaded solver.** rapier calls hooks from its worker
   threads, so no script contact filter runs there. A `Sync` hook could read a
   `DetHashMap<(handle, handle), Decision>` the main thread fills after each
   step, answering a new pair one step late. Whether that latency is
   acceptable waits for the first game that needs both.
