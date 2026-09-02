# Determinism, recording and replay

Balaur promises that identical inputs produce bit-for-bit identical
simulations on every platform. This page is how you use that promise: how to
write a game that keeps it, how to check that it holds, and what to do when
it does not.

`ARCHITECTURE.md` covers *why* the engine is built this way. This is the
practical side.

## Why you would want it

- **Replays that are actually small.** A recorded session stores what the
  player pressed, not what the world looked like. Minutes of play is
  kilobytes.
- **Bugs you can reproduce.** "It desynced sometimes" becomes "tick 4213,
  node `n_ball_7`, component `body2d`".
- **Multiplayer that sends inputs instead of state.** Deterministic lockstep
  costs bandwidth proportional to the number of players, not the size of the
  world — an RTS with 2000 units costs what pong costs.

## The two tick callbacks

The single most important rule: **simulation goes in `fixed_update`.**

```luau
local Player = {}

-- Runs on the fixed 60 Hz step. Zero or more times per frame, always the
-- same dt, immediately before physics. Simulation lives here.
function Player:fixed_update(dt)
    if input.is_down(input.KEY_SPACE) then
        physics.apply_impulse(self.node, 0, 5, 0)
    end
end

-- Runs once per frame with the real frame time. Presentation only:
-- anything a dropped frame may safely skip.
function Player:update(dt)
    self.bob = self.bob + dt
end

return Player
```

| | `update(dt)` | `fixed_update(dt)` |
| --- | --- | --- |
| Runs | once per frame | 0–4 times per frame |
| `dt` | measured frame time | always `1/60` |
| Before physics | no | yes |
| Reproducible | no | yes |

A force applied in `fixed_update` lands on the step it was meant for, on
every machine. A force applied in `update` lands before however many physics
steps that machine happened to run.

## Checking that it holds

Pin the simulation step and write a hash of the world each tick:

```bash
balaur run my-game --fixed-tick --trace-digest run-a.txt
```

Each line is `<tick> <digest>`. Two runs that produce different files parted
at the first differing line. Run it on two machines and `diff` the results —
that is the whole cross-platform check, and it is what CI does for every
example on Linux, macOS and Windows.

## Recording a session

```bash
balaur run my-game --fixed-tick --record session.blr
```

The file is JSON Lines: a header, then one line per tick carrying that tick's
external input, the step it ran at, and the digest it produced. It records
**input, not state** — the world is rebuilt by re-running your game against
the same input.

Play it back, re-checking every tick:

```bash
balaur replay session.blr --verify
```

```
60 ticks replayed, every digest matched
```

If a tick disagrees it stops there and exits non-zero:

```
Error: tick 25: recorded a3391e01013eaf31 but replayed a3391e01013eaf30
run `balaur replay <file> --entries-at 25` on both machines and diff
```

To find out *what* differed rather than just when, dump that tick's parts:

```bash
balaur replay session.blr --entries-at 25
```

```
n_platform/transform 0801bbde6c5fc306
n_platform/body 88bae8672f02ed9f
n_ball/transform 1e773bf22b6856a5
```

Each machine dumps from its own recording; `diff` the two and the line that
differs names the node and the component. Node names come from the scene
file's `id`, so they survive rename and reparent — two machines always agree
on what to call the thing that broke.

Replaying never touches the network. A recorded session that made an HTTP
request replays the recorded reply; it does not make the request again.

## Rollback

Recording answers "what happened"; a snapshot answers "put it back". Capture
the world at a tick, keep the last few in a `SnapshotRing`, and when a late
input arrives restore the tick before it and re-simulate forward. Because the
simulation is deterministic, re-simulating the same inputs from the restored
state reaches the same world the first run did -- the digest is how you check
that, and `crates/balaur_core/tests/snapshot.rs` is the test that holds it.

```rust
let taken = balaur_core::snapshot::capture(&app.engine);
// ... ticks pass, a late input shows up ...
balaur_core::snapshot::restore(&app.engine, &taken);
```

Each subsystem saves and restores its own state (transforms and RNG in core,
rapier's worlds in physics, script instances through `save_state` /
`load_state`). Give a script those two methods when only part of its state
matters; leave them out and its plain fields are captured for it. A snapshot
does not respawn or free nodes: restore writes over what exists.

## What breaks determinism

Most of these the engine already handles. The ones marked **your problem**
are the ones to watch.

| Hazard | Status |
| --- | --- |
| `math.random` | Handled — seeded engine stream, recorded in the replay header |
| `math.sin`, `cos`, `exp`, … | Handled — pure-Rust `libm`, identical everywhere |
| Physics across platforms | Handled — rapier's `enhanced-determinism` |
| Variable `dt` | Handled if you simulate in `fixed_update` |
| Network replies | Handled — recorded and replayed, outbound suppressed |
| `x ^ y` | **Your problem** — the operator calls the platform `pow`. Use `math.pow` |
| `os.clock()`, `os.time()` | **Your problem** — wall clock is not reproducible. Use `engine.tick()` |
| `pairs()` over table or userdata keys | **Your problem** — iteration order depends on pointer values. Use arrays, or string/number keys |
| Hot reload mid-session | **Your problem** — changing code changes behaviour. Turn it off for a run you intend to verify |

## Rules of thumb

1. Simulation in `fixed_update`, presentation in `update`.
2. Never branch simulation on wall-clock time.
3. Iterate arrays, or tables keyed by strings or numbers — never by table
   identity.
4. Use `math.pow`, not `^`.
5. Record a session in CI and `--verify` it. A determinism bug found the day
   it lands costs an afternoon; found six months later it costs a rewrite.

## Extending it

Two hooks, for plugin authors.

**A subsystem that receives things from outside** registers a replay source so
its arrivals land in recordings:

```rust
// Passive state the OS fills in — one line.
app.add_replay_resource::<InputSnapshot>("input");
```

If it also *sends* — a socket, a request — use `replay::ExternalIo` instead.
It owns the worker channel and the recording, and the only way to reach the
outside is through `start`, which does nothing while a replay is playing. You
cannot forget the check, because there is no other way to get the sender.

**A subsystem that computes state a component does not report** adds it to
the digest, so a divergence in it is caught:

```rust
// `body3d` reports its kind, not its velocity — and velocity diverges first.
app.add_digest_source("physics", |eng, out| { /* push labelled entries */ });
```
