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

## Rig tooling

The engine halves of what `docs/PLAN-editor.md` §6 "Rigging panels" draws,
written down on 2026-09-05. Each is a data change with a headless test
before it is a panel.

- **Chain solvers and jiggle.** `modifier2d` and, with 3D IK above,
  `modifier3d` gain kinds beside `look_at` and `two_bone_ik`: `fabrik` and
  `ccdik` over a chain of any length (Godot's SkeletonModification2DFABRIK
  and CCDIK; iterations and a per-bone angle limit), and `jiggle` (a
  spring per bone toward its rest, stepped on the fixed tick so it
  replays). Planned; every transcendental on `libm`.
- **Retargeting through a bone map.** A `bone_map` asset: canonical bone
  names (a `skeleton_profile` shipped for a humanoid, or one the project
  writes) to a rig's node paths. `animation.play(node, clip, { retarget =
  "maps/hero.toml" })` renames each track's `target` through the map
  before sampling, and rest-pose differences are corrected per bone by
  the rig's `Bone` rest against the profile's. Planned; the editor panel
  follows the asset.
- **Deform tracks.** A `polygon/deform` track keys a list of `[dx, dy]`
  per vertex, added to `positions` before skinning; the sampler already
  lerps any width, and `skin_positions` is where the offset lands. Planned,
  behind a `channels` change: a track today holds four numbers a key.
- **Physical bones.** A body and a joint per bone, built from a rig by one
  call (`physics2d.ragdoll(root)`, and the 3D twin) and blended back with
  the clip by a weight. `docs/PLAN-physics.md`'s, listed here so the panel
  and the call are one item.

## Also deferred

- **The player is in neither the snapshot nor the digest.** `balaur_anim`
  registers no `snapshot` or `digest` source, so a rollback rewinds the
  transforms a clip wrote and not the playhead that wrote them, and the
  accumulator keeps its residual across a record restart. First item of
  `docs/PLAN-hardening.md` phase 1; nothing above should be built on top of
  a player that rollback cannot put back.
- **Skinning, rigs and retargeting** shipped since, in 2D and 3D; retargeting
  a clip from one rig to another did not — "Rig tooling" above.
- ~~**Stable asset ids**~~ — built 2026-09-05 as `id://` over
  `assets/index.toml`, with `assets.rename` for the paths
  (`docs/PLAN-scenes-and-assets.md`).
- **`tween_method` / `tween_subtween`**, and the `loop_finished` /
  `step_finished` signals.
- ~~**Asset hot reload through the file watcher.**~~ Built: the watcher
  reloads every `.toml` asset and moves the generation for every binary.
- **Animating a dynamic body fights physics**, as it does in Godot. Warn
  once, document, and let it be.
