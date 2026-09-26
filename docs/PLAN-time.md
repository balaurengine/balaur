> **Status:** built, bar one item. All four steps below landed: pause and
> `process`, the time scale and `max_fps`, interpolation behind
> `[time] interpolate`, and `[time] tick_hz`. What is left is the multiplayer
> handshake refusing a peer at another rate, which waits on a handshake to
> put it in — see [PLAN-multiplayer.md](PLAN-multiplayer.md).

# Plan: pause, process mode, time scale and interpolation between ticks

## 0. What it is now

- `engine.set_paused(true)` holds every `pausable` subtree, and
  `process = "always" | "when_paused" | "disabled" | "pausable" | "inherit"`
  is a key on a node like `visible`, inherited by its subtree
  (`crates/balaur_core/src/process.rs`).
- The fixed stage keeps running through a pause. Each subsystem skips the
  nodes the pause holds — scripts, animation players, tweens, state machines,
  the `timer` component — so an `always` subtree ticks inside a held game.
  Physics is one world and is held whole, as Godot's is.
- `on_paused_changed(bool)` reaches every script, the ones the pause just stopped
  included: `ScriptHost::announce` is `call_all_with` without the pause filter.
- `engine.set_time_scale(s)` multiplies measured frame time before it is owed,
  and the substep cap scales with it so fast forward is not silently capped.
  A run driving at a fixed step ignores it.
- `[window] max_fps` caps the loop, in `App::run` and in the windowed backend.
  `[window] vsync` was already there.
- `[time] interpolate` turns on drawing between steps. A node that opts in
  keeps the poses the last two steps left it in, and the scene sync draws
  `lerp(previous, current, accumulator / step)`. A moving body and a script
  declaring `fixed_update` opt in on their own; `interpolate` on the node
  overrides either way; `node.reset_interpolation()` and both `teleport`s
  clear it; a rollback clears it with the transforms it restores.
- `[time] tick_hz` replaces the constant. `balaur_core::fixed_dt()` is what
  every subsystem reads, a recording's header carries the rate, and
  `MAX_SUBSTEPS` scales with it. `DEFAULT_FIXED_DT` is the default, not the rate.

## 1. Where the tree was before this

- One accumulator drains into whole `DEFAULT_FIXED_DT` steps at `DEFAULT_TICK_HZ = 60`, at
  most `MAX_SUBSTEPS = 4` per frame; time past that is dropped
  (`crates/balaur_core/src/app.rs:781-800`).
- A debugger pause holds the simulation for a subtree: `Engine::frozen_root`
  skips `Stage::FixedUpdate` and every script call inside the scope, and
  the frame loop keeps drawing (`engine.rs:198-220`).
- `physics.set_paused` holds both physics worlds; a paused replay holds
  its own clock.
- The renderer reads `GlobalTransform` as of the last tick
  (`kiss3d_backend.rs:583`), so between two ticks a body drawn at 144 Hz
  stands still for every second frame.
- `task.frames` and `task.seconds` count fixed steps (`timers.rs`), so a
  wait replays exactly.
- `App::set_fixed_dt` and `--fixed-tick` drive a run at exactly one step per
  frame; a rollback session steps at the fixed step and takes no `dt`.
- The recording header carries the RNG seed, the bindings and the platform
  facts; not the tick rate, because there is one.

## 2. Design

**Pause is the debugger's freeze given to scripts, asked per node.**
`engine.set_paused(true)` holds what a breakpoint holds: no `update` or
`fixed_update` for the scripts inside it, physics held, animation held, `task`
waits held. It is not a second frozen root, because `always` has to let one
subtree run while its sibling is held and a root cannot say that. The fixed
stage keeps running and every per-node subsystem asks `process::ticks`
instead. A node with `process = "always"` — a core key beside `visible` —
keeps its subtree ticking through a pause, which is what a pause menu is;
`process = "when_paused"` ticks only while paused; `process = "disabled"`
never ticks; `process = "pausable"` is the default and cancels an inherited
`always`. Physics is one world, so a paused root holds every body even under
an `always` node, as Godot's does.

**A pause is not recorded.** The script that paused runs again on replay and
pauses again; nothing about it enters the input trace. A pause from outside
the simulation — the OS suspending the app — arrives as `on_focused_changed`
already, and what a game does with it is a script's.

**Time scale feeds the accumulator.** `engine.set_time_scale(s)` multiplies
the measured frame time before it is owed, so half speed takes half the
steps and double speed takes twice, still whole steps, still one fixed
step each. A replay and a session drive by tick and ignore it, which is the
constraint stated on the reader: a scale is a wall-clock matter.

**Interpolation is render-side state.** After each fixed step the engine
keeps the previous tick's pose beside the current one for every node that
opts in, and the render sync draws
`lerp(previous, current, accumulator / the step)`. Nothing reads it back:
`node.transform.position` answers the tick, not the frame, so a script never
sees a blended pose and the digest never contains one. A rollback restores the tick
and discards the previous pose, which is exactly what a snapshot already
does with everything render-side. A body that teleports resets its
previous pose, so a respawn does not streak across the level.

**Who interpolates.** A node carrying a dynamic or kinematic body, and a
node whose script has a `fixed_update`, by default; anything else moves in
`update` per frame and needs nothing. `interpolate = true | false` on the
node overrides either way, and `[time] interpolate` in `project.toml` is what
turns any of it on. Off is the default: the blend is one step behind real
time, and a run taking exactly one step per frame would pay that latency for
no smoothness at all.

**The tick rate is a project setting last.** `[time] tick_hz` replaces the
constant, the recording header carries it, and `MAX_SUBSTEPS` scales with it.
Refusing a peer at another rate waits on a session handshake to carry it. Sixty stays the default and
the one rate the cross-OS digest job runs, so nothing below it moves until a
game has a reason.

## 3. The surface

| Need | Decision |
| --- | --- |
| Pause the game, keep the menu alive | Step 1: `engine.set_paused`, `engine.paused`, `process` on a node, `on_paused_changed(bool)` on every script |
| A node that never ticks | Step 1: `process = "disabled"` |
| Slow motion, fast forward | Step 2: `engine.set_time_scale`, `engine.time_scale`; `engine.time` keeps counting scaled time, `engine.unix_time` does not |
| A hitch that should not run four steps at once | Have: `MAX_SUBSTEPS` drops the time; step 2 scales the cap with the time scale so fast forward is not silently capped |
| Smooth motion at 120 and 144 Hz | Step 3: interpolation for transforms and the cameras; `node.reset_interpolation()`; `physics3d.teleport` and `physics2d.teleport` reset on their own |
| A script reading the drawn pose | **Not planned**: the drawn pose is render-side; a script wanting it is a script wanting `update` |
| A tick rate other than sixty | `[time] tick_hz`, carried in the recording header. The session handshake that would refuse a peer at another rate does not exist yet; two peers at different rates desync on the first digest exchange, which is reported rather than refused |
| Frame rate cap and vsync | `[window] max_fps` beside the `[window] vsync` that was already there; `App::frame_budget` is what both loops sleep to |
| A timer node | Have: the `timer` component and `task.seconds`, both held by a pause; the component asks its node's `process`, a token has no node to ask |
| Sound that stops when the game does | **Not planned here**: playback is the backend's, not a per-frame advance, and which sounds a pause holds is a question the `audio` plan owns |

## 4. Steps

1. **Done.** Pause and `process`. Not a second frozen root: the fixed stage
   keeps running and each per-node subsystem asks `process::ticks`, because a
   frozen root cannot hold one subtree and let its sibling run.
2. **Done.** Time scale, the substep cap, `max_fps` and `vsync`.
3. **Done.** Interpolation, behind `[time] interpolate`, with the body and
   `fixed_update` defaults.
4. **Done.** The tick rate setting, in the recording header. The session
   handshake has no message to carry it yet.

## 5. What CI can prove, and what it cannot

Headless proves a paused run and an unpaused one differ only in the ticks
that did not happen, that the same script pausing on tick 30 replays to the
same digest, and that a scaled run takes the step count the scale predicts.
The existing headless-versus-offscreen digest job proves interpolation
touches no state. CI cannot prove smoothness; an offscreen run at a
simulated 120 Hz can assert a body's drawn position moves every frame.

## 6. Open questions

1. **Two freeze roots or one.** Settled: neither. A debugger's freeze still
   stops the whole fixed stage, and a game's pause is a per-node question the
   subsystems ask, because `always` has to let one subtree run while its
   sibling is held and a root cannot express that. A breakpoint inside a
   paused game is the two answering in order.
2. **Interpolating a rotation across a wrap.** Settled: `Quat::slerp`, and a
   node spinning faster than half a turn per step draws the short way. Godot
   has the same limit.
3. **A `task.seconds` wait inside an `always` subtree.** A token carries no
   node, so a pause holds every wait, `always` subtrees included. A pause
   menu that fades with `task.seconds` has to fade on `update` instead. The
   fix is a token that remembers who minted it, and it is not built.
