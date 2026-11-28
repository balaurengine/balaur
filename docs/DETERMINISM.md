# Determinism, recording and replay

Identical inputs produce bit-for-bit identical simulation on every platform.
How to keep that, check it, and debug it when it breaks. `ARCHITECTURE.md` says
why the engine is built this way.

It buys replays that store what was pressed, not what the world looked like,
so minutes of play is kilobytes. "It desynced sometimes" becomes "tick 4213,
node `n_ball_7`, component `body2d`". Lockstep multiplayer costs bandwidth per
player rather than per object.

## The two tick callbacks

**Simulation goes in `fixed_update`.**

| | `update(dt)` | `fixed_update(dt)` |
| --- | --- | --- |
| Runs | once per frame | 0–4 times per frame at 60 Hz, more at a faster tick |
| `dt` | measured frame time | always `1/tick_hz`, `1/60` by default |
| Before physics | no | yes |
| Reproducible | no | yes |

```bs
pub fn fixed_update(dt: float) {         // simulation
    if input::key_down(input::KEY_SPACE) {
        self.node.body3d.apply_impulse(vec3(0.0, 5.0, 0.0));
    }
}

pub fn update(dt: float) {               // presentation only
    self.bob = self.bob + dt;
}
```

A force applied in `fixed_update` lands on the step it was meant for, on every
machine; one applied in `update` lands before however many steps that machine
ran.

## Checking it

```bash
balaur run my-game --fixed-tick --trace-digest run-a.txt
```

One `<tick> <digest>` per line. Two runs that differ parted at the first
differing line, so `diff` them across machines. CI does this for five examples
(`scripts/determinism_trace.sh` names them) on Linux, macOS and Windows.

## Recording

```bash
balaur run my-game --fixed-tick --record session.blr
balaur replay session.blr --verify
balaur replay session.blr --entries-at 25
```

- The file is JSON Lines: a header, one line per tick (that tick's external
  input and the step it ran at), a trailer. It records **input, not state**.
- `--verify` re-checks every tick and stops at the first mismatch with a
  non-zero exit, naming the recorded and replayed digests. It also holds what
  each tick sent against what the recording's sent: each HTTP request with its
  method, URL and body, each socket opened and each frame sent on one, a body
  as its length and hash. The first tick they part on is named with both lists.
- A run started with `--scene` keeps that scene in the header, and its replay
  opens it in place of the project's `main_scene`.
- `--entries-at <tick>` dumps that tick's parts, as
  `n_ball/transform 1e773bf22b6856a5`. Dump on both machines and `diff`: the
  differing line names the node and the component. Labels come from the scene's
  `id`, so they survive rename and reparent.
- A command-line run digests every tick; the editor records without them and
  writes one at the end, since a play session answers "what happened" and only a
  verified run needs "where did it part". The Session dock's `verify` toggle
  turns per-tick digests on.
- The editor records every play session into its data directory, and the Session
  dock replays it with a timeline of input, requests, log lines and stops.
- A replay never touches the network: a recorded HTTP request replays its
  recorded reply.

## Rollback

Recording answers "what happened"; a snapshot puts it back. Capture the world at
a tick, keep the last few in a `CheckpointRing`, and when a late input arrives
restore and re-simulate. The re-run is deterministic, so it reaches the same
world.

```rust
let taken = balaur_core::snapshot::capture(&app.engine);
// ... a late input shows up ...
balaur_core::snapshot::restore(&app.engine, &taken);
```

Each subsystem saves its own state (transforms and RNG in core, rapier's worlds
in physics, a script instance's plain fields in the host). Restore puts the node
*set* back too: core's `nodes`
source frees what was spawned and respawns what was freed, before any other
source writes. `crates/balaur_core/tests/suite/snapshot.rs` holds it.

## Local nodes

A subtree tagged `local` is this machine's alone: a camera, a HUD, a sign-in.
The digest hashes none of it, no snapshot frame holds it, and a restore never
frees, moves or writes it, so a multiplayer join keeps the joiner's own. A
snapshot looks a node up by id among the shared nodes only, since a joiner's
local node can carry an id the host minted for another. A plugin's own source
(physics, animation) still saves a local node's state.

## Scene switches

`scene.switch` frees the scene it leaves and builds the next, so only the
scenes standing are world state.

- **Replay:** a switch happens on the tick a script asked for it, so a replay
  frees and builds on the same ticks.
- **Snapshot:** each frame names the scene standing under each node, by the
  stable ids of the node and of the scene's root. A restore across a switch
  frees what it built and respawns what it freed, as it does any node.
- **Preload:** `scene.preload` fills caches and builds no node, so a run that
  preloaded and one that did not build the same world. It answers no status a
  script could branch on.

## What breaks determinism

| Hazard | Status |
| --- | --- |
| `random::float`, `random::range`, `random::int` | Handled — seeded stream, recorded in the replay header |
| `math::sin`, `cos`, `exp`, … | Handled — pure-Rust `libm` |
| `Quat::from_euler`, `Vec3::normalize`, … | Handled — `glamx` pinned to `libm` and `scalar-math` |
| Physics across platforms | Handled — rapier's `enhanced-determinism` |
| Variable `dt` | Handled if you simulate in `fixed_update` |
| Network replies | Handled — recorded and replayed, outbound suppressed |
| A file a request's `save_to` wrote | Handled — its bytes are recorded with the reply, and the replay writes them again as the reply lands |
| A worker's answer (`json::parse_async`, `ui::picture_async`, `image::encode`) | Handled — the tick each answer was taken on is recorded, and a replay waits for that worker on that tick |
| `audio::device_ready`, `audio::devices` | Handled — the machine is asked once a tick and the answer recorded; a replay answers from the recording |
| `engine::dark_mode()`, `focused()` and the rest of the device's facts | Handled — recorded each tick, and in the header as they stood when `init` read them |
| `cmd` in an `[input.actions]` binding | Handled — whether it is ⌘ or Control is in the header, and a replay's bindings read that, not the replaying machine's |
| A tick rate per platform | Handled — `[time] tick_hz` takes no override, and the header carries the rate |
| `task::seconds` | Handled — counted in whole fixed steps, `ceil(seconds * tick_hz)`, not summed as a float |
| `engine::unix_time()` | Handled — read once a tick and recorded, so a replay reads the time its recording did. A tool's own script, on a node outside `debugger::set_scope`'s subtree, reads the machine's clock, which is never recorded |
| `engine::time()`, `engine::delta()` | **Yours** — both accumulate real frame time. Use `engine::tick()` or `fixed_update`'s `dt` |
| A shader's `time()` | Handled — it reads `engine::time()`, so a run at `--fixed-tick` and a replay draw the same frames |
| Iterating a map's keys | Handled — a map keeps the order its keys were added |
| A recording started part-way through a frame | Handled — the game holds through the rest of that frame, as a replay holds the frame it loads in |
| A tween's handle | Handled — the editor's own tweens count apart from the game's, and the header carries where the game's start |
| A plugin's own fixed-step accumulator | Handled for the animation player's playhead and springs — each drops its remainder when `Engine::step_restarts` moves, and under a hold only when `Engine::hold_keeps_remainder` says to, as the app does. A tool's own clips and tweens, outside the held subtree, step through a hold on a remainder apart from the game's |
| Hot reload mid-session | Handled in the editor, which ends the session on a reload. Under `--record` the recording spans the change, with only the header's script fingerprint to say so |

Every `math` function is safe to call: `sqrt`, `abs`, `floor`, `ceil`, `round`,
`min` and `max` are exactly rounded by IEEE-754, and the rest go through `libm`,
which `crates/balaur_bs/tests/suite/determinism.rs` holds.
On the Rust side `scripts/house_lints.py` fails the build on a bare `.sin()`,
`f32::sin(x)`, `.powf()` and the rest of the inexact list.

`libm` costs too little to trade its accuracy for speed. Rapier's step calls no
`sin`, `cos`, `exp`, `log` or `pow`: rotations integrate as quaternions and
normalise, leaving `atan2` and `sqrt`, and `sqrt` is one exact instruction. A
script's `sin` is 8 ns of `libm` inside 75 ns of interpreter, and polynomial
approximations bought under 1% of a tick wherever they were measured.

## Rules of thumb

1. Simulation in `fixed_update`, presentation in `update`.
2. Never branch simulation on accumulated time; `engine::tick()` is the exact
   integer.
3. Iterate a `Vec`, or sort an object's keys first.
4. Record a session in CI and `--verify` it.

## Extending it, for plugin authors

```rust
// Passive state a frame fills in and nothing sends back, from the plugin's `declare`.
reg.add_replay_resource::<WidgetInputSnapshot>("ui");

// State a component does not report — `body3d` reports its kind, not velocity.
reg.add_digest_source("physics", |eng, out| { /* push labelled entries */ });

// Something a timeline should show. Free when nothing is recording.
replay::event(eng, "net.request", format!("{method} {url}"), None);

// A read in the frame a recording starts in, where `init` runs: the header's.
replay::amend_setup(eng, "audio_device", answers);
```

A subsystem that also *sends* uses `replay::ExternalIo` instead of a replay
resource: it owns the worker channel, and the only way out is `start`, which
does nothing while a replay is playing.
