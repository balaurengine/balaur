> **Status:** not started. Written down on 2026-09-02 so the order is decided
> before the first line: rollback first, because determinism makes it cheap
> here; replication second, factored so both ride the same identity, property
> access and clock; QUIC under both, as WebTransport, so one transport runs
> native and in the browser. `ARCHITECTURE.md` "Networking and state sync"
> has the reasoning; this is the work.

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
- Every transport is TCP. No UDP, no QUIC, no WebTransport, no WebRTC: one
  lost packet stalls everything behind it.
- Nothing accepts a connection. Every socket is outbound, so two engines
  cannot meet without a server between them.
- No replication, no RPC, no authority, no interest management, no
  prediction, no delta encoding.
- No lobbies or matchmaking beyond a Gamend room.

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
| WebSocket (TCP, TLS, `permessage-deflate`) | Have, text only; binary frames come in phase 1. Stays for turn-based games, lobbies, chat, and as the fallback where UDP is blocked. Wrong for anything twitchy: one lost packet stalls every frame behind it. |
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

## 3. Phases

1. **Bytes.** A `Value::Bytes` variant, `websocket::send` taking either
   text or bytes, binary frames delivered as bytes. Needed by everything
   below and useful alone.
2. **Rollback on one machine.** Snapshot restore learns to respawn and free
   nodes. A `Session` with local players only, an input journal per tick,
   and a test that drops a late input in, rolls back and reaches the same
   digest as the straight run.
3. **Rollback over the wire.** The transport trait with the websocket under
   it and a loopback server in the test; two engines play a scene in
   lockstep with rollback, and a test injects a desync and asserts both stop
   on the same tick.
4. **WebTransport, native.** One crate (`wtransport` or `web-transport-quinn`)
   behind the same trait, client and server: datagrams for inputs, a stream
   for the handshake and digests. The loopback test of phase 3 runs again
   over it.
5. **WebTransport in the browser.** The emscripten shim over the browser's
   WebTransport API, once web export lands; `serverCertificateHashes` for
   the dev server.
6. **Replication.** The `replicate` component, change detection, delta
   encoding, an authority rule, RPC by stable id, and the client's predict
   and reconcile loop for its own node.
7. **Sessions from Gamend.** A room becomes a session, and the relay for
   browser peers; matchmaking and presence stay on the server side.
8. **WebRTC data channels.** Only when a game needs browser peer-to-peer
   without a relay. Same trait, same loopback tests.

Each phase has its own tests before the next starts; a headless CI run of two
engines on loopback is the bar for 3, 4 and 6.

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
4. **Which QUIC crate.** Both wrap `quinn`; they differ in API and
   maintainer. Decided at phase 4 by whichever makes the server side
   smaller.
5. **Relay or direct.** Whether native rollback peers connect to each other
   (NAT traversal, no server cost) or always meet at a relay (one code path,
   browsers included). One code path is the default until latency says
   otherwise.
