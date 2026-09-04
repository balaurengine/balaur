> **Status:** steps 1-7 done — bytes on the wire, identity and lifecycle for
> run-time nodes, rollback on one machine, the transport trait with a
> websocket under it, two engines playing in lockstep over loopback, one
> crate per protocol with QUIC under the trait, and a recorded session tested
> against a link that drops and delays.
>
> Written down on 2026-09-02 so the order was decided before the first line:
> node identity and lifecycle first, because rollback and replication both
> stand on it and it is the piece that can invalidate the determinism work;
> rollback next, because determinism makes it cheap here; replication after a
> session exists, factored so both ride the same identity, property access
> and clock; QUIC under both, as WebTransport, so one transport runs native
> and in the browser. `ARCHITECTURE.md` "Networking and state sync" has the
> reasoning; §3 is the work, in order.

# Plan: multiplayer

What the engine has today, and what a game needs that it does not.

## 0. Where the tree is today

Built, and not built for networking:

| Have | Where |
| --- | --- |
| HTTP, websockets (TLS, `permessage-deflate`, headers), per-protocol defaults | `balaur_http`, `balaur_websocket` |
| Login, REST, rooms, server hooks | `balaur_gamend` |
| Every arrival enters the simulation once per tick, in order, recorded and replayable | `ExternalIo`, `NetSnapshot` |
| A fixed 60 Hz step and a digest per tick | `Stage::FixedUpdate`, `digest` |
| Whole-world snapshots: transforms, RNG, both physics worlds, script state | `snapshot` |
| Cross-machine identity that survives rename and reparent | `StableId` |
| A property schema for every component | `ComponentRegistry` |
| Bytes across the script boundary, and binary websocket frames | `Value::Bytes`, `balaur_websocket` |
| An id for every run-time spawn, and a snapshot that restores the node set | `ids`, `snapshot`'s `nodes` source |
| Rollback in one process: a session, an input journal, prediction and re-simulation | `rollback` |
| A transport trait with reliable and unreliable delivery, over a websocket | `core::transport`, `net::transport` |
| Accepting connections, and a session that plays over them | `websocket::listener`, `core::netsession` |
| One crate per protocol, and real QUIC datagrams under the trait | `balaur_http`, `balaur_websocket`, `balaur_webtransport` |
| A recordable session, fault injection, and per-link measurement | `netsession`, `transport::Faulty` |

Missing:

- No WebRTC, so two browsers cannot reach each other without a relay.
- The browser has no WebTransport backend yet, so a web build still falls
  back to websockets.
- No replication, no RPC, no authority, no delta encoding. Inputs are
  predicted; state is not: no client-side prediction of the local player
  against a server, no reconciliation of the correction that comes back, no
  interpolation of the players a client does not own, no lag compensation,
  no interest management.
- Nothing measures the link. No round-trip time, loss or bytes a second per
  peer, and no way to give a transport latency, loss or reordering in a
  test, so every number so far is a loopback number.
- No lobbies or matchmaking beyond a Gamend room.
- A networked session is not in the recording. Inputs reach the journal from
  a transport rather than through a replay source, so a session replays only
  if the journal is recorded — which nothing does yet.
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

### Hiding latency

Rollback and replication hide the same 60-100 ms in different places, and a
game picks one. Under rollback every peer simulates everything and a wrong
guess is paid for by re-running a tick. Under replication the server is the
only simulation that counts, so the client covers the round trip itself:

- **Client-side prediction.** The client applies its own input to its own
  node on the tick it is pressed, without waiting for the server, and keeps
  that input pending, keyed by the tick it ran on. This is the `rollback`
  journal again with one player in it.
- **Server reconciliation.** A delta carries the last input tick the server
  consumed. The client drops what is acked, restores its own node to the
  server's state at that tick, and replays the inputs still pending. Nodes
  it does not own are never re-simulated: they are the server's word.
  Agreeing costs nothing, and `is_resimulating` keeps the replay off the
  network the way it already does for rollback.
- **Correction smoothing.** A correction that moves a node visibly decays
  out of the render transform over a few frames instead of snapping it. It
  is a rendering offset and never feeds back into simulation state, which is
  the rule the three run modes rest on.
- **Interpolation of everyone else.** Unowned nodes are drawn a fixed delay
  behind the newest state received — a send interval or two — and
  interpolated between the two states that bracket the render time, so a
  late or lost delta reads as motion rather than a stall. Extrapolating past
  the newest state is a per-node choice, off by default.
- **Lag compensation.** A server testing a hit rewinds the world to the tick
  the shooter saw. The snapshot ring is that history already; what is
  missing is the per-client view time and a bound on how far back a rewind
  may go, since the client is the one claiming what it saw.
- **Clock and jitter.** All of it needs the client's tick to lead the
  server's by about the one-way delay plus jitter. The offset rides the
  channel the digests already use, and the client trims its own tick by a
  fraction of a step rather than jumping.

None of this is authority. The server simulates, the client guesses ahead,
and where they disagree the server wins — which is what makes the model
resist a modified client, and why prediction is a step of its own rather
than a line in the replication step.

| Technique | Decision |
| --- | --- |
| Rollback with input prediction | Have — steps 3 and 5 |
| Client-side prediction of the local node | Step 10 |
| Server reconciliation of the local node | Step 10 |
| Correction smoothing | Step 10 |
| Interpolation of unowned nodes | Step 10 |
| Extrapolation (dead reckoning) past the newest state | Step 10, per node, off by default |
| Clock offset and jitter estimate | Step 10 |
| Lag compensation by server rewind | Step 11 |
| Interest management | Step 12 |
| Delta encoding against an acked baseline, quantised off the schema | Step 9 |
| Join in progress from a baseline snapshot | Step 9 |
| Client-authoritative hit registration | Not planned — the server decides a hit |
| Deterministic peer-to-peer with no server | Have — step 5 |

## 2. Transports

Every protocol a multiplayer engine could sit on, and where each stands here.

| Protocol | Decision |
| --- | --- |
| HTTP | Have. REST, Gamend, downloads. Not a game transport. |
| WebSocket (TCP, TLS, `permessage-deflate`) | Have, text only; binary frames come in step 1. Stays for turn-based games, lobbies, chat, and as the fallback where UDP is blocked. Wrong for anything twitchy: one lost packet stalls every frame behind it. |
| Raw UDP | Not exposed to scripts, now or later. No browser has it, it brings no encryption, congestion control or NAT story of its own, and every engine that ships it ends up writing a reliability layer on top. QUIC datagrams are that layer, standardised. |
| QUIC over WebTransport | The target, and the only transport under rollback and replication. Reliable-ordered streams and unreliable datagrams in one encrypted connection: datagrams carry inputs and unreliable deltas, streams carry the handshake, digests, RPCs and reliable state. Native through `quinn`, wrapped by `web-transport-quinn` (§4.4); in the browser through the WebTransport API, which Chrome, Firefox and Safari all ship. Client-server only: a browser cannot listen, and the server is an HTTP/3 endpoint with a certificate. |
| WebRTC data channels | Fallback, for one case: browser peer-to-peer without a relay. Needs a signalling channel (a Gamend room) and STUN/TURN; the unreliable-unordered mode gives UDP-like datagrams. Candidates are `matchbox` (native and browser, `matchbox_server` for signalling) or `webrtc` (webrtc-rs). Behind the same trait; not built until a game needs it. |
| ENet, laminar, custom UDP protocols | Not planned. They solve what QUIC solves, and none of them runs in a browser. |

Two constraints shape the browser half:

- The emscripten build is thread-free and has no wasm-bindgen, so
  `web-transport-wasm` (web-sys) does not apply. The browser transport is a C
  shim over the browser API, the way the emscripten shim wraps fetch and
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
4. **The transport trait, websocket under it.** *Done.* `Transport` in
   `balaur_core` carries one reliable ordered channel and unreliable
   datagrams, polled per tick rather than awaited, so a link records and
   replays like every other source. `WebsocketTransport` in `balaur_websocket`
   implements it: both deliveries share one socket and carry a one-byte tag,
   and a datagram is sent reliably — degraded, never wrong. Opening goes
   through `ExternalIo::start`, so a replay or a re-simulated tick opens no
   socket. The QUIC crate is decided (§4.4). Open: one reliable channel
   rather than many, so a big reliable message would block small ones behind
   it; nothing sends one yet.
5. **Rollback over the wire.** *Done.* `WebsocketListener` accepts
   connections, which the engine had never done, and hands back the same
   `WebsocketTransport` the dialling side produces. `NetSession` puts a
   rollback session behind peers: each sends its own input for the tick it is
   about to run as a datagram, predicts the rest, and rolls back when a real
   one contradicts a prediction. Digests for a tick a few behind travel
   reliably and are compared; a mismatch names the tick and latches. Two
   engines in one process agree on every settled tick, and a peer whose
   simulation genuinely differs is caught by both ends on the same tick.
   Two bugs fell out: the websocket worker never unmasked client-to-server
   frames, because only a client had ever run it, and the snapshot ring
   appended re-simulated ticks instead of replacing them, so a
   rollback-heavy session forgot ticks it still needed.
6. **One crate per protocol, and WebTransport native.** *Done.* `balaur_net` became
   three plugin crates: `balaur_http`, `balaur_websocket` and
   `balaur_webtransport`. Nothing shared is left behind, because the shared
   part is already in core — `ExternalIo` for the delivery contract,
   `Transport` for the seam, `Engine::next_token` for await ids — so each is
   a self-contained plugin registering its own replay source, the way
   `balaur_physics` and `balaur_input` already do. What that buys is the
   reason for doing it: a QUIC stack is a large dependency, and a game that
   only wants `http::request` should not compile one.

   `balaur_webtransport` puts `quinn` behind the same trait, through
   `web-transport-quinn` (§4.4), client and server: datagrams for inputs, a
   stream for the handshake and digests. The crate is async and the engine is
   polled, so a link owns a worker thread with a current-thread runtime and
   speaks the same two channels the websocket worker does — no runtime
   anywhere near the frame loop. QUIC is always TLS: a server generates a
   short-lived self-signed certificate by default and the client pins it by
   hash, so two engines on one machine need no setup, and a shipped server
   passes a real certificate and key instead. The reliable channel is one
   bidirectional stream, and a stream is bytes rather than messages, so a
   reliable payload travels behind a four byte length; a datagram needs no
   framing, which is half the reason to use one — and the reads are their own
   tasks rather than arms of a `select!`, because `read_exact` is not
   cancel-safe and a cancelled one loses the bytes it had already taken.
   On by default, like audio and Gamend: the split is what makes turning it
   off possible, which is a different question from what a batteries-included
   facade should ship. Step 5's lockstep test runs again over it, two engines
   agreeing tick for tick with the inputs on real datagrams.

   Nothing keeps the old name for the sake of the old name. `net` described a
   crate that held two unrelated protocols, and now that it holds none, the
   word stops meaning anything. Four consequences, each chosen for
   consistency rather than for compatibility:
   - The one replay source named `net` becomes `http` and `websocket`. An
     old recording's `net` source goes unclaimed, so it replays without its
     network traffic. Recordings from before the split are not carried
     forward.
   - `[net]` in `project.toml` becomes `[http]` and `[websocket]`, and the
     keys drop the prefix the table now carries: `http_timeout` is
     `[http] timeout`, `websocket_compression` is `[websocket] compression`.
     A protocol's settings live under its own name, and a key never repeats
     its table.
   - `shim/emscripten_net.c` splits along the same line, fetch in one and
     websockets in the other, each compiled by its own crate's `build.rs`.
   - The `net` feature is removed rather than aliased. Three features named
     for what they are — `http`, `websocket`, `webtransport` — and each crate
     re-exported under its own name, so a game reaches for `balaur::http`
     rather than for a bundle that happens to contain it.
7. **Recording a session, and measuring the link.** *Done.* Peer payloads go
   through `PeerTraffic`, an `ExternalIo` behind a `session` replay source,
   so a recorded session replays with no peer on the other end and a desync
   is reproducible from a file. `transport::Faulty` wraps any transport and
   adds the three faults loopback does not have — delay, jitter and datagram
   loss — counted in polls rather than wall time, so the same seed holds the
   same payloads for the same ticks. `NetSession::stats` reports round-trip
   time, loss from sequence gaps, and bytes each way, published as
   `SessionStats` the way `engine.timings()` is.

   Turning the faults on immediately found a real gap: inputs were sent once,
   unreliably, and never repeated, so a dropped one was lost for good and the
   tick it belonged to diverged permanently. Every datagram now carries the
   last twelve ticks of that player's input, so one packet arriving repairs
   every gap behind it. At one datagram in twenty lost, twelve in a row is
   about one run in 2^52.
8. **Sessions from Gamend.** A room becomes a session, and the relay for
   browser peers; matchmaking and presence stay on the server side. First
   point at which a real game ships on this stack, which is why it comes
   before replication rather than after.
9. **Replication.** The `replicate` component, change detection, delta
   encoding against the observer's last acked tick, quantisation off the
   schema, an authority rule, RPC by stable id, and join in progress from a
   baseline snapshot. Inherits a proven transport, a proven session and
   working spawn and free.
10. **Client prediction and server reconciliation.** The client runs its own
    input on the tick it is pressed and holds it pending until a delta acks
    the tick it ran on; the correction rewinds that node alone, replays what
    is still pending, and decays out of the render transform rather than
    snapping. Unowned nodes are drawn a send interval or two behind and
    interpolated. The clock offset rides the digest channel, and the client
    trims its tick by a fraction of a step to hold its lead. §1 "Hiding
    latency" has the shape.
11. **Lag compensation.** The server rewinds to the tick the shooter saw
    before testing a hit, off the snapshot ring it already keeps, bounded by
    how far back a rewind may go and by what a client may claim it saw.
12. **Interest management and a bandwidth budget.** A client is sent what it
    can see: a per-observer filter over the replicated set, a send rate
    decoupled from the tick rate, and a cap that drops the least urgent
    deltas rather than growing a queue.
13. **WebTransport in the browser.** The emscripten shim over the browser's
    WebTransport API, with `serverCertificateHashes` for the dev server.
    Blocked on web export, so it lands whenever that does.
14. **WebRTC data channels.** Only when a game needs browser peer-to-peer
    without a relay. Same trait, same loopback tests.

Each step has its own tests before the next starts; a headless CI run of two
engines on loopback is the bar for 5, 6, 9 and 10.

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
6. **Which nodes a client predicts.** Whether prediction follows from
   `authority = "owner"` or is its own key on `replicate`. A game that
   predicts a vehicle it is riding but not driving wants the second; every
   game wants the first to mean something sensible without saying so.
7. **Where the send rate is set.** A 60 Hz tick does not want 60 sends a
   second. Whether the rate is a `[webtransport]` default, per node, or per observer
   decides whether interpolation delay can be a fixed number of frames.
