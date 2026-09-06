# Determinism, recording and replay

Identical inputs produce bit-for-bit identical simulation on every platform.
How to keep that, check it, and debug it when it breaks. `ARCHITECTURE.md` says
why the engine is built this way.

What it buys: replays that store what was pressed, not what the world looked
like (minutes of play is kilobytes); "it desynced sometimes" becomes "tick 4213,
node `n_ball_7`, component `body2d`"; and lockstep multiplayer that costs
bandwidth per player rather than per object.

## The two tick callbacks

**Simulation goes in `fixed_update`.**

| | `update(dt)` | `fixed_update(dt)` |
| --- | --- | --- |
| Runs | once per frame | 0–4 times per frame |
| `dt` | measured frame time | always `1/60` |
| Before physics | no | yes |
| Reproducible | no | yes |

```rune
pub fn fixed_update(this, dt) {          // simulation
    if input::is_down(input::KEY_SPACE) {
        physics3d::apply_impulse(this.node, 0.0, 5.0, 0.0);
    }
}

pub fn update(this, dt) {                // presentation only
    this.bob = this.bob + dt;
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
differing line — `diff` them across machines. CI does this for every example on
Linux, macOS and Windows.

## Recording

```bash
balaur run my-game --fixed-tick --record session.blr
balaur replay session.blr --verify
balaur replay session.blr --entries-at 25
```

- The file is JSON Lines: a header, one line per tick (that tick's external
  input and the step it ran at), a trailer. It records **input, not state**.
- `--verify` re-checks every tick and stops at the first mismatch with a
  non-zero exit, naming the recorded and replayed digests.
- `--entries-at <tick>` dumps that tick's parts —
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
a tick, keep the last few in a `SnapshotRing`, and when a late input arrives
restore and re-simulate — deterministically, so the re-run reaches the same
world.

```rust
let taken = balaur_core::snapshot::capture(&app.engine);
// ... a late input shows up ...
balaur_core::snapshot::restore(&app.engine, &taken);
```

Each subsystem saves its own state (transforms and RNG in core, rapier's worlds
in physics, script instances through `save_state`/`load_state`). Give a script
those two methods when only part of its state matters; leave them out and its
plain fields are captured. Restore puts the node *set* back too: core's `nodes`
source frees what was spawned and respawns what was freed, before any other
source writes. `crates/balaur_core/tests/snapshot.rs` holds it.

## What breaks determinism

| Hazard | Status |
| --- | --- |
| `rng::random`, `rng::range`, `rng::int` | Handled — seeded stream, recorded in the replay header |
| `math::sin`, `cos`, `exp`, … | Handled — pure-Rust `libm` |
| `Quat::from_euler`, `Vec3::normalize`, … | Handled — `glamx` pinned to `libm` and `scalar-math` |
| Physics across platforms | Handled — rapier's `enhanced-determinism` |
| Variable `dt` | Handled if you simulate in `fixed_update` |
| Network replies | Handled — recorded and replayed, outbound suppressed |
| `x.powf(y)`, `x.powi(n)` | Handled — our Rune fork routes both through `libm` |
| `engine::time()`, `engine::delta()` | **Yours** — both accumulate real frame time. Use `engine::tick()` or `fixed_update`'s `dt` |
| Iterating an object's keys | **Yours** — `#{}` is hash-ordered. Iterate a `Vec`, or `sort()` the keys |
| Hot reload mid-session | Handled in the editor, which ends the session on a reload. Under `--record` the recording spans the change, with only the header's script fingerprint to say so |

Every float method Rune exposes is safe to call: `sqrt`, `abs`, `floor`,
`ceil`, `round`, `min` and `max` are exactly rounded by IEEE-754, and `powf` and
`powi` are asserted against `libm` in `crates/balaur_script_rune/tests/pow.rs`.
On the Rust side `scripts/house_lints.py` fails the build on a bare `.sin()`,
`f32::sin(x)`, `.powf()` and the rest of the inexact list.

## Rules of thumb

1. Simulation in `fixed_update`, presentation in `update`.
2. Never branch simulation on accumulated time — `engine::tick()` is the exact
   integer.
3. Iterate a `Vec`, or sort an object's keys first.
4. Record a session in CI and `--verify` it.

## Extending it, for plugin authors

```rust
// Passive state the OS fills in, from the plugin's `declare`.
reg.add_replay_resource::<InputSnapshot>("input");

// State a component does not report — `body3d` reports its kind, not velocity.
reg.add_digest_source("physics", |eng, out| { /* push labelled entries */ });

// Something a timeline should show. Free when nothing is recording.
replay::event(eng, "net.request", format!("{method} {url}"), None);
```

A subsystem that also *sends* uses `replay::ExternalIo` instead of a replay
resource: it owns the worker channel, and the only way out is `start`, which
does nothing while a replay is playing.
