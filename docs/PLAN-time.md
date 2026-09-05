> **Status:** not started. Written 2026-09-05 from the Godot parity
> investigation: a game cannot pause, slow down, or draw smoothly between
> ticks on a display faster than the tick.

# Plan: pause, process mode, time scale and interpolation between ticks

## 0. Where the tree is today

- One accumulator drains into whole `FIXED_DT` steps at `TICK_HZ = 60`, at
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

## 1. Design

**Pause is the debugger's freeze, given to scripts.** `engine.set_paused(true)`
freezes the root's subtree the way a breakpoint does: no fixed step, no
`update` for scripts inside it, physics held, animation held, `task` waits
held. A node with `process = "always"` — a core key beside `visible` —
keeps its subtree ticking through a pause, which is what a pause menu is;
`process = "when_paused"` ticks only while paused; `process = "disabled"`
never ticks. Physics is one world, so a paused root holds every body even
under an `always` node, as Godot's does.

**A pause is not recorded.** The script that paused runs again on replay and
pauses again; nothing about it enters the input trace. A pause from outside
the simulation — the OS suspending the app — arrives as `on_focus_changed`
already, and what a game does with it is a script's.

**Time scale feeds the accumulator.** `engine.set_time_scale(s)` multiplies
the measured frame time before it is owed, so half speed takes half the
steps and double speed takes twice, still whole steps, still `FIXED_DT`
each. A replay and a session drive by tick and ignore it, which is the
constraint stated on the reader: a scale is a wall-clock matter.

**Interpolation is render-side state.** After each fixed step the engine
keeps the previous tick's pose beside the current one for every node that
opts in, and the render sync draws
`lerp(previous, current, accumulator / FIXED_DT)`. Nothing reads it back:
`node.position()` answers the tick, not the frame, so a script never sees a
blended pose and the digest never contains one. A rollback restores the tick
and discards the previous pose, which is exactly what a snapshot already
does with everything render-side. A body that teleports resets its
previous pose, so a respawn does not streak across the level.

**Who interpolates.** A node carrying a dynamic or kinematic body, and a
node whose script has a `fixed_update`, by default; anything else moves in
`update` per frame and needs nothing. `interpolate = true | false` on the
node overrides either way, and `[time] interpolate = false` in
`project.toml` turns it off for a project that wants the old frames.

**The tick rate is a project setting last.** `[time] tick_hz` replaces the
constant, the recording header carries it, a session refuses a peer whose
rate differs, and `MAX_SUBSTEPS` scales with it. Sixty stays the default and
the one rate the cross-OS digest job runs, so nothing below it moves until a
game has a reason.

## 2. The surface

| Need | Decision |
| --- | --- |
| Pause the game, keep the menu alive | Step 1: `engine.set_paused`, `engine.paused`, `process` on a node, `on_paused(bool)` on every script |
| A node that never ticks | Step 1: `process = "disabled"` |
| Slow motion, fast forward | Step 2: `engine.set_time_scale`, `engine.time_scale`; `engine.time` keeps counting scaled time, `engine.unix_time` does not |
| A hitch that should not run four steps at once | Have: `MAX_SUBSTEPS` drops the time; step 2 scales the cap with the time scale so fast forward is not silently capped |
| Smooth motion at 120 and 144 Hz | Step 3: interpolation for transforms and the cameras; `node.reset_interpolation()`; `physics3d.teleport` and `physics2d.teleport` reset on their own |
| A script reading the drawn pose | **Not planned**: the drawn pose is render-side; a script wanting it is a script wanting `update` |
| A tick rate other than sixty | Step 4: `[time] tick_hz`, in the recording header and the session handshake |
| Frame rate cap and vsync | Step 2: `[render] max_fps`, `vsync = true`; the loop already sleeps to a target |
| A timer node | Have: `task.seconds`, held by a pause and slowed by the scale |

## 3. Steps

1. Pause and `process`, reusing `frozen_root` with a second, script-owned
   root beside the debugger's.
2. Time scale, the substep cap, `max_fps` and `vsync`.
3. Interpolation, behind the project flag, with the body and `fixed_update`
   defaults. The showcase's physics clip is re-recorded at 120 Hz to show it.
4. The tick rate setting.

## 4. What CI can prove, and what it cannot

Headless proves a paused run and an unpaused one differ only in the ticks
that did not happen, that the same script pausing on tick 30 replays to the
same digest, and that a scaled run takes the step count the scale predicts.
The existing headless-versus-offscreen digest job proves interpolation
touches no state. CI cannot prove smoothness; an offscreen run at a
simulated 120 Hz can assert a body's drawn position moves every frame.

## 5. Open questions

1. **Two freeze roots or one.** The debugger's scope and a game's pause
   are the same mechanism with two owners; a breakpoint inside a paused
   game has to keep the editor drawing and the game held. One root with two
   reasons, or two roots the loop checks in order — decided at step 1.
2. **Interpolating a rotation across a wrap.** `Quat::slerp` on `libm` is
   fine; the 2D angle is a quaternion too, so nothing special is needed,
   but a node spinning faster than half a turn per tick draws the short way.
   Godot has the same limit.
