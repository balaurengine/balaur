> **Status:** not started. Written down on 2026-09-02 so the order is decided
> before the first line: rollback first, because determinism makes it cheap
> here; replication second, factored so both ride the same identity, property
> access and clock; QUIC under both. `ARCHITECTURE.md` "Networking and state
> sync" has the reasoning; this is the work.

# Plan: multiplayer

What the engine has today, and what a game needs that it does not.

## 0. Where the tree is today

Built, and not built for networking:

| Have | Where |
| --- | --- |
| HTTP, websockets (TLS, `permessage-deflate`, headers), `[net]` defaults | `balaur_net` |
| Login, REST, rooms, server hooks | `balaur_gamend` |
| Every arrival enters the simulation once per tick, in order, recorded and replayable | `ExternalIo`, `NetSnapshot` |
| A fixed 60 Hz step and a digest per tick | `Stage::FixedUpdate`, `digest` |
| Whole-world snapshots: transforms, RNG, both physics worlds, script state | `snapshot` |
| Cross-machine identity that survives rename and reparent | `StableId` |
| A property schema for every component | `ComponentRegistry` |

Missing:

- Nothing sends a tick's inputs to another machine, and nothing re-simulates
  when a late one arrives.
- Snapshot restore writes over nodes that exist; it cannot respawn a freed
  node or free a spawned one.
- The script value model has no bytes, so a websocket drops binary frames.
- Every transport is TCP: one lost packet stalls everything behind it.
- No replication, no RPC, no authority, no interest management, no
  prediction, no delta encoding.
- No lobbies or matchmaking beyond a Gamend room.

## 1. Design

Four layers, each usable without the ones above it:

1. **Transport.** Reliable-ordered streams and unreliable datagrams behind
   one trait, so a game never names a socket type. Websocket today; QUIC
   later; the browser through a JS bridge, the way the emscripten HTTP shim
   already works.
2. **Session.** Who is in the game, at which tick, with which clock offset.
   Membership comes from a Gamend room or from a hand-written server; the
   session only needs a member list and a way to send to each.
3. **Rollback.** Inputs go on the wire, not state. Each peer keeps a ring of
   snapshots, predicts a missing input by repeating the last one, and on a
   late arrival restores the tick before it and re-simulates forward. The
   per-tick digest is the desync detector: peers exchange digests a few ticks
   behind and stop on the first mismatch, naming the tick and, through
   `replay --entries-at`, the node.
4. **Replication.** A server simulates and sends deltas; clients apply,
   predict their own node, and reconcile. Declared in the scene, not in code:

   ```toml
   [[nodes]]
   name = "Player"
   id = "n_player"
   replicate = { authority = "owner", components = ["transform", "body2d"], mode = "on_change" }
   ```

   Change detection is a generation counter per `(entity, component)` written
   by `components::patch`; the delta is the changed properties since the
   observer's last acked tick, quantised off the schema's declared ranges.
   RPCs address a node by `StableId`, never by path.

Rollback ships before replication. It is the smaller job on a deterministic
engine, it is the one nobody else can offer cheaply, and replication's
prediction and reconciliation reuse its snapshot ring.

## 2. Phases

1. **Bytes.** A `Value::Bytes` variant, `websocket::send` taking either
   text or bytes, binary frames delivered as bytes. Needed by everything
   below and useful alone.
2. **Rollback on one machine.** Snapshot restore learns to respawn and free
   nodes. A `Session` with local players only, an input journal per tick,
   and a test that drops a late input in, rolls back and reaches the same
   digest as the straight run.
3. **Rollback over the wire.** The websocket transport under the session;
   two engines on loopback play a scene in lockstep with rollback, and a
   test injects a desync and asserts both stop on the same tick.
4. **QUIC.** One transport crate (`wtransport` or `web-transport-quinn`)
   behind the same trait; datagrams for inputs, a stream for the handshake
   and digests. Native first; the browser bridge when web export lands.
5. **Replication.** The `replicate` component, change detection, delta
   encoding, an authority rule, RPC by stable id, and the client's predict
   and reconcile loop for its own node.
6. **Sessions from Gamend.** A room becomes a session; matchmaking and
   presence stay on the server side.

Each phase has its own tests before the next starts; a headless CI run of two
engines on loopback is the bar for 3 and 5.

## 3. Open questions

1. **Hidden information under lockstep.** Every client simulates everything,
   so fog of war cannot hide state from a modified client. Answer per game:
   lockstep for games that can live with it, replication for the rest.
2. **Script state in rollback.** A snapshot captures every script's plain
   fields, or what `save_state` / `load_state` return. A rollback restores
   them 60 times a second; a script that keeps a handle to something outside
   the simulation needs the explicit pair. Whether to require it or to warn
   is decided by the first game that hits it.
3. **Hot reload during a session.** Replaying journaled inputs against
   changed code cannot reproduce the recorded state. A reload truncates the
   journal and re-baselines from the current snapshot: rewind within a
   version, reload across versions, never rewind across a reload.
4. **Which QUIC crate.** Decided by whichever runs in the browser first.
