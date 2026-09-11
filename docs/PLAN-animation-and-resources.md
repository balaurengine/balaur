> **Status:** the asset layer and the animation system shipped on 2026-08-31;
> ARCHITECTURE.md's "Assets" and "Animation" sections and
> [generated/](generated/) are the record. What was deferred, deliberately and
> with nothing precluded, is below.

# Plan: animation — blending, trees and state machines

The sampler stayed pure — `(clip, time) -> pose`, reachable with no `Engine` —
so a blender composes *samples* without the data model moving. Three pieces,
in order:

1. **Blend — built, 2026-09-11.** `sampler::blend(from, from_pose, to,
   to_pose, weight)` pairs two clips' tracks by target and property, lerps
   positions, scales, tints and component numbers, slerps rotations, and
   switches a name or a flag at the halfway point; a track only one clip
   keys takes that clip's value. The player holds the outgoing clip as a
   `Fade`, still advancing, until `fade` seconds have run:
   `animation::play(node, "run", #{ fade: 0.2 })`. Method tracks fire from
   the incoming clip only. The fade is in the rollback snapshot and the
   digest.
2. **Blend trees.** An `animation_tree` asset: a small graph of nodes —
   clip, blend by one parameter, blend by two — evaluated to one pose per
   tick from parameters a script sets (`animation::set_param(node, "speed",
   v)`). The tree is data in a TOML file, edited in the Animate persona as
   a list before it is a graph.
3. **State machines — built over clips, 2026-09-11.** A `state_machine`
   asset maps states to clips of the player's library and carries
   transitions with a fade, an `advance` (`disabled`, `enabled` for travel
   only, `auto`), a `switch` (`immediate`, `sync`, `at_end`) and a
   condition, Godot's `AnimationNodeStateMachine` field for field
   (`crates/balaur_anim/src/machine.rs`). The `state_machine` component runs
   one against a player; `animation::travel` walks the fewest transitions
   to a state and cuts to one none reaches, `jump` cuts, `set_condition`
   feeds `auto`. Evaluated on the fixed step after the players advance, and
   in the snapshot and the digest, so a replay reproduces every transition.
   A state naming a blend tree waits on 2.

3D IK follows the 2D modifiers: `modifier3d` with `look_at` and
`two_bone_ik` on a `bone3d` chain, the same analytic solve in three
dimensions with a pole vector. Rapier's `Multibody::inverse_kinematics` is a
solver already in the tree, but it solves a reduced-coordinates joint chain
(`physics3d.solve_ik`), not a rig. Everything stays on `libm` and the fixed
step; the digest already covers bone transforms.

## Rig tooling — built, 2026-09-06

The engine halves of what `docs/PLAN-editor.md` §6 "Rigging panels" draws,
written down on 2026-09-05 and built the next day. Each landed as a data
change with a headless test before it was a panel.

- **Chain solvers and jiggle.** `modifier2d` gained `fabrik` and `ccdik`
  over a chain of any length (Godot's SkeletonModification2DFABRIK and
  CCDIK; `iterations`, a `tolerance` and a per-bone `angle_limit`) and
  `jiggle` (a spring per bone toward the pose, stepped on the fixed tick
  so it replays), and **`modifier3d`** is the same five kinds over
  `bone3d`. FABRIK and the spring move points in `Vec3` and 2D passes them
  with `z = 0`, so there is one reaching algorithm rather than two that
  drift apart; only the step that turns a solved point back into a
  rotation is written twice. Every transcendental is `libm`'s.

  Two things the spring needed that the plan did not say. It keeps the
  pose the clip wrote separately from the pose it wrote itself, or it
  springs toward its own last answer — a pair that agree at any angle,
  including upside down. And its point is a direction to aim along, not a
  joint position: holding it on the bone's own circle adds a second fixed
  point, exactly opposite the pose, where the pull is along the radius the
  projection cancels.
- **Retargeting through a bone map.** `bone_map` and `skeleton_profile`
  assets, with a humanoid profile built in rather than shipped as a file
  so `retarget` works in a project that has written no assets of its own.
  `animation.play(node, clip, { retarget = "maps/hero.toml" })` renames
  each track's `target` through the map before the pose is written, reads
  a rotation key as a turn away from the profile's rest and applies it to
  this rig's, and scales a position key by how much longer this rig's bone
  rests. A track the map says nothing about keeps the path it was authored
  with, so a clip that half matches plays the half that does.
- **Deform tracks.** A `polygon/deform` track keys a list of `[dx, dy]` per
  vertex. The `channels` change it was behind is a *wide* key beside the
  `Vec4` one rather than a widening of every key, so the transform hot path
  still allocates nothing; the offsets land on the node as a
  `balaur_core::mesh::Deform`, which the renderer adds to the authored
  positions before it skins. The vertex buffer is rewritten in place on the
  frames a deform actually moved something, rather than the node being
  rebuilt.
- **Physical bones.** `physics2d.ragdoll(root, opts)` and the 3D twin walk a
  rig into a body and a capsule per bone, hinged to its parent's, and leave
  a `ragdoll` component whose `blend` moves each bone from the pose the clip
  wrote toward the pose its body ended up in — 0 simulates unseen, 1 goes
  limp, and `physics.ragdoll_blend` tweens between them. The bodies are
  ordinary nodes with ordinary components, under a container at the scene
  root because physics reads and writes a body's transform as a world one.

## Also deferred

- ~~**The player is in neither the snapshot nor the digest.**~~ Built:
  `balaur_anim::snapshot` registers both, plus a replay setup for the
  fixed-step residual. Every playhead, every running tween with its
  generated clip, and every jiggle spring rides a rollback; the digest
  hashes the playhead and the spring but not the pose, which the transform
  walk already covers.
- ~~**Skinning, rigs and retargeting**~~ — all three shipped, in 2D and 3D;
  retargeting last, as "Rig tooling" above.
- ~~**Stable asset ids**~~ — built 2026-09-05 as `id://` over
  `assets/index.toml`, with `assets.rename` for the paths
  (`docs/PLAN-scenes-and-assets.md`).
- **`tween_method` / `tween_subtween`**, and the `loop_finished` /
  `step_finished` signals.
- ~~**Asset hot reload through the file watcher.**~~ Built: the watcher
  reloads every `.toml` asset and moves the generation for every binary.
- **Animating a dynamic body fights physics**, as it does in Godot. Warn
  once, document, and let it be.
