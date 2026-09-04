> **Status:** the asset layer and the animation system shipped on 2026-08-31;
> ARCHITECTURE.md's "Assets" and "Animation" sections and
> [generated/](generated/) are the record. What was deferred, deliberately and
> with nothing precluded, is below.

# Plan: animation — blending, trees and state machines

The sampler stayed pure — `(clip, time) -> pose`, reachable with no `Engine` —
so a blender composes *samples* without the data model moving. Three pieces,
in order:

1. **Blend.** A pose is a `Vec<TrackValue>`; `blend(a, b, t)` lerps
   positions and scales and slerps rotations, track by track, over two
   poses of the same clip shape. The player gains a second slot and a
   cross-fade: `animation::play(node, "run", #{ fade: 0.2 })` samples both
   clips and blends by elapsed fade. Method tracks fire from the incoming
   clip only.
2. **Blend trees.** An `animation_tree` asset: a small graph of nodes —
   clip, blend by one parameter, blend by two — evaluated to one pose per
   tick from parameters a script sets (`animation::set_param(node, "speed",
   v)`). The tree is data in a TOML file, edited in the Animate persona as
   a list before it is a graph.
3. **State machines.** States name a tree or a clip; transitions carry a
   condition on parameters and a fade. Evaluated on the fixed step, so a
   replay reproduces every transition.

3D IK follows the 2D modifiers: `modifier3d` with `look_at` and
`two_bone_ik` on a `bone3d` chain, the same analytic solve in three
dimensions with a pole vector. Rapier's `Multibody::inverse_kinematics` is a
solver already in the tree, but it solves a reduced-coordinates joint chain
(`physics3d.solve_ik`), not a rig. Everything stays on `libm` and the fixed
step; the digest already covers bone transforms.

## Also deferred

- **The player is in neither the snapshot nor the digest.** `balaur_anim`
  registers no `snapshot` or `digest` source, so a rollback rewinds the
  transforms a clip wrote and not the playhead that wrote them, and the
  accumulator keeps its residual across a record restart. First item of
  `docs/PLAN-hardening.md` phase 1; nothing above should be built on top of
  a player that rollback cannot put back.
- **Skinning, rigs and retargeting** shipped since, in 2D and 3D; retargeting
  a clip from one rig to another did not.
- **Stable asset ids** (`uid://`, GUIDs) — paths until renames actually hurt.
- **`tween_method` / `tween_subtween`**, and the `loop_finished` /
  `step_finished` signals.
- **Asset hot reload through the file watcher.** `assets.reload` is the
  mechanism a watcher would call; the watcher is script-only today.
- **Animating a dynamic body fights physics**, as it does in Godot. Warn
  once, document, and let it be.
