# Plan: every setting the wrapped crates have

A component exposes every setting the type it wraps offers, as a key, a
reader, or a stated constraint in the code. What is left waits on changes
outside this repository.

## Steps

1. **The rapier settings in Ughuuu/rapier#4**, once a rapier release carries
   them. `roll_influence` and `skid_info` become public, `side_factor` and
   `fwd_factor` wheel settings, and the character controller's sweep
   iterations, depenetration budget, ground probe and platform friction
   settings. Where: `crates/balaur_physics/src/vehicle.rs` and
   `character.rs`, both dimensions. Done when each is a key on `wheel3d`,
   `vehicle3d` or both characters, with its reader, read back by `get`.
2. **The kiss3d fork merged.** Merge `balaur-wrapped-surface`
   (Ughuuu/kiss3d#1) into `balaur-hooks`. Done when `Cargo.toml` names
   `balaur-hooks` and the workspace builds against it.
3. **2D GI on the web.** `camera2d`'s `gi` pass renders into an `Rgba32Float`
   target that no browser run has drawn. Done when a web export of a scene
   with `gi` shows the lit frame in a browser.
