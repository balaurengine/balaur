> **Status:** steps 1-3 done (bytes on the wire; identity and lifecycle for
> run-time nodes; rollback on one machine). Written down on 2026-09-02 so the
> order was decided before the first line: node identity and lifecycle first, because rollback
> and replication both stand on it and it is the piece that can invalidate
> the determinism work; rollback next, because determinism makes it cheap
> here; replication after a session exists, factored so both ride the same
> identity, property access and clock; QUIC under both, as WebTransport, so
> one transport runs native and in the browser. `ARCHITECTURE.md`
> "Networking and state sync" has the reasoning; §3 is the work, in order.

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
| Bytes across the script boundary, and binary websocket frames | `Value::Bytes`, `balaur_net` |
| An id for every run-time spawn, and a snapshot that restores the node set | `ids`, `snapshot`'s `nodes` source |
| Rollback in one process: a session, an input journal, prediction and re-simulation | `rollback` |

Missing:

- Nothing sends a tick's inputs to another machine. Rollback works within one
  process; no transport carries a journal between two.
- Every transport is TCP. No UDP, no QUIC, no WebTransport, no WebRTC: one
  lost packet stalls everything behind it.
- Nothing accepts a connection. Every socket is outbound, so two engines
  cannot meet without a server between them.
- No replication, no RPC, no authority, no interest management, no
  prediction, no delta encoding.
- No lobbies or matchmaking beyond a Gamend room.
- A run-time `scene.instantiate` still reuses the file's ids, so two
  instances of one scene collide.

## 1. Design

Four layers, each usable without the ones above it:

1. **Transport.** Reliable-ordered streams and unreliable datagrams behind
   one trait, so a game never names a socket type. Websocket today;
   WebTransport (QUIC) next, native and in the browser; WebRTC data channels
   only for browser peer-to-peer. §2 has the decision per protocol.
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

## 2. Transports

Every protocol a multiplayer engine could sit on, and where each stands here.

| Protocol | Decision |
| --- | --- |
| HTTP | Have. REST, Gamend, downloads. Not a game transport. |
| WebSocket (TCP, TLS, `permessage-deflate`) | Have, text only; binary frames come in step 1. Stays for turn-based games, lobbies, chat, and as the fallback where UDP is blocked. Wrong for anything twitchy: one lost packet stalls every frame behind it. |
| Raw UDP | Not exposed to scripts, now or later. No browser has it, it brings no encryption, congestion control or NAT story of its own, and every engine that ships it ends up writing a reliability layer on top. QUIC datagrams are that layer, standardised. |
| QUIC over WebTransport | The target, and the only transport under rollback and replication. Reliable-ordered streams and unreliable datagrams in one encrypted connection: datagrams carry inputs and unreliable deltas, streams carry the handshake, digests, RPCs and reliable state. Native through `wtransport` or `web-transport-quinn`; in the browser through the WebTransport API, which Chrome, Firefox and Safari all ship. Client-server only: a browser cannot listen, and the server is an HTTP/3 endpoint with a certificate. |
| WebRTC data channels | Fallback, for one case: browser peer-to-peer without a relay. Needs a signalling channel (a Gamend room) and STUN/TURN; the unreliable-unordered mode gives UDP-like datagrams. Candidates are `matchbox` (native and browser, `matchbox_server` for signalling) or `webrtc` (webrtc-rs). Behind the same trait; not built until a game needs it. |
| ENet, laminar, custom UDP protocols | Not planned. They solve what QUIC solves, and none of them runs in a browser. |

Two constraints shape the browser half:

- The emscripten build is thread-free and has no wasm-bindgen, so
  `web-transport-wasm` (web-sys) does not apply. The browser transport is a C
  shim over the browser API, the way `shim/emscripten_net.c` wraps fetch and
  websockets today.
- A browser accepts a self-signed certificate through
  `serverCertificateHashes` only while it is valid for at most two weeks.
  Enough for local play and CI; a shipped server needs a real certificate.

Two browsers therefore never connect to each other over WebTransport, since
neither can listen. Rollback between them goes through a relay: a Gamend
room, or a small server on the same transport that forwards inputs. Native
peers may connect directly. The session layer does not care which.

## 3. Steps

Ordered so the riskiest piece is proved first and each step leaves something
that works.

1. **Bytes.** *Done.* A `Value::Bytes` variant, `websocket::send` taking
   either text or bytes, and a binary frame delivered as
   `{ kind = "binary", bytes }`. The C extension ABI gained a bytes tag
   without a layout change, so existing extensions still load.
2. **Identity and lifecycle for run-time nodes.** *Done.* `ids::mint` gives
   every run-time spawn an `<authority>:<counter>` id, and the counter is
   snapshotted, so a re-simulated spawn mints the id the first run did. A
   `nodes` snapshot source records each node's id, name, parent, components
   and script and restores the set: freeing what was spawned since,
   respawning what was freed. Transforms and script state are keyed by id
   rather than entity index, because a respawned node is a new entity. A
   spawn and a free across a snapshot boundary roll back to a bit-identical
   digest. Still open: a run-time `scene.instantiate` reuses the file's ids.
3. **Rollback on one machine.** *Done.* `rollback::Session` owns the tick,
   a snapshot ring and the journal. It predicts a missing input by repeating
   that player's last one, and when the real one arrives and disagrees it
   restores the tick before it and re-runs everything since; a late input
   matching the prediction costs nothing. The clock is snapshotted, so a
   re-run of tick 3 is tick 3 again. `ExternalIo::start` refuses to spawn
   work while `rollback::is_resimulating`, so a second run of a tick does
   not send its requests twice. Scripts join in through a `rollback` module:
   `input(player)` and `is_resimulating()`. A late input rolls back to the
   digest of the run that had it on time, and a script's own fields come
   back with it. Open: an input older than the ring cannot be answered, so
   `Session::stale_inputs` counts it and the caller has to resync.
4. **The transport trait, websocket under it.** Reliable-ordered streams and
   unreliable datagrams in the trait from the first line, shaped by QUIC and
   not by what a websocket happens to offer; the websocket implements
   datagrams as reliable sends, degraded but correct. Spike both QUIC crates
   before writing it so the trait fits the winner rather than being
   retrofitted to it.
5. **Rollback over the wire.** Two engines on loopback play a scene in
   lockstep with rollback, and a test injects a desync and asserts both stop
   on the same tick.
6. **WebTransport, native.** The chosen crate behind the same trait, client
   and server: datagrams for inputs, a stream for the handshake and digests.
   The loopback tests of step 5 run again over it.
7. **Sessions from Gamend.** A room becomes a session, and the relay for
   browser peers; matchmaking and presence stay on the server side. First
   point at which a real game ships on this stack, which is why it comes
   before replication rather than after.
8. **Replication.** The `replicate` component, change detection, delta
   encoding, an authority rule, RPC by stable id, and the client's predict
   and reconcile loop for its own node. Inherits a proven transport, a
   proven session and working spawn and free.
9. **WebTransport in the browser.** The emscripten shim over the browser's
   WebTransport API, with `serverCertificateHashes` for the dev server.
   Blocked on web export, so it lands whenever that does.
10. **WebRTC data channels.** Only when a game needs browser peer-to-peer
    without a relay. Same trait, same loopback tests.

Each step has its own tests before the next starts; a headless CI run of two
engines on loopback is the bar for 5, 6 and 8.

Two notes on running this:

- **The risk is concentrated in step 2.** If a physics restore plus a respawn
  does not reach a bit-identical digest, every step after it is blocked and
  the determinism work does not hold up. That is why its test is a digest
  comparison and why it does not wait for rollback to expose it.
- **Steps 1 and 4 do not depend on step 2.** Two people split there: one
  takes identity and lifecycle, the other takes bytes and the transport
  trait.

## 4. Open questions

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
4. **Which QUIC crate.** *Decided: `web-transport-quinn`.* Both wrap `quinn`
   and expose the same four stream calls, so the trait fits either. It wins
   on the server side: `open_bi` hands back the stream pair in one step where
   `wtransport` returns an `OpeningBiStream` to await again, it ships
   `ServerBuilder`/`ClientBuilder` with defaults, and it has
   `send_datagram_wait` and `datagram_send_buffer_space`, which a session
   pushing a datagram every tick needs and `wtransport` does not offer.
   `wtransport` keeps a `quic_connection()` escape hatch if that is ever
   worth switching for.
5. **Relay or direct.** Whether native rollback peers connect to each other
   (NAT traversal, no server cost) or always meet at a relay (one code path,
   browsers included). One code path is the default until latency says
   otherwise.
