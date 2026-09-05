> **Status (2026-09-04):** steps 1-7 done — bytes on the wire, identity and
> lifecycle for run-time nodes, rollback on one machine and over a socket, the
> transport trait with a websocket and real QUIC datagrams under it, one crate
> per protocol, and a recorded session tested against a link that drops and
> delays. Fault injection is switchable from the editor's settings screen
> (`netcode/faults`) and applies to every session link. `ARCHITECTURE.md`
> "Networking and state sync" is the record; §3 is what is left, in order.
>
> **Next:** step 9, replication, before step 8. The plan orders Gamend first
> because it is where a real game ships, and that reason stands for ship
> order — but 8 needs the hosted backend and cannot be proven on loopback,
> while 9 is testable against the fault-injecting transport step 7 built for
> exactly this. Settle open question 6 (which nodes a client predicts) before
> starting 9: it decides how change detection is scoped, and deciding is
> cheaper than refactoring. Steps 13 and 14 stay blocked and deferred
> respectively.
>
> **2026-09-05:** the Photon parity investigation split step 8 in two —
> `docs/PLAN-sessions.md` is the engine half (a session a script can open,
> `peer` / `host` / `server` roles, server-ordered inputs, late join,
> reconnect, spectators, host migration, bincode on the wire, the editor's
> Network dock) and `docs/PLAN-gamend.md` is the server half (a game server
> per lobby, lobby tokens, rejoin, the match record, typed bindings). Voice
> got `docs/PLAN-voice.md`. §0 and §1 below gained the rows that came out of
> it.
>
> Two housekeeping items wait on other work: `docs/generated/crates.md`
> still lists `balaur_net` and regenerates once the whole workspace builds,
> and the editor's manifest loader (`model.rn`, now reading
> `manifest.application`) has not been rendered since the `[application]`
> migration because the editor build sits behind crates mid-edit elsewhere.

# Plan: multiplayer

What a game needs that the engine does not have.

## 0. What is missing

- No WebRTC, so two browsers cannot reach each other without a relay.
- The browser has no WebTransport backend it can reach: `browser.rs` is a
  wasm-bindgen client gated on `target_family = "wasm"`, which the
  emscripten web target matches but cannot run, so a web build still falls
  back to websockets. The decision — a C shim beside `emscripten_websocket.c`,
  or a gate on `not(target_os = "emscripten")` — is step 13.
- Nothing bounds a session yet: the rollback journal's `arrived` and `used`
  are never pruned, a peer may name any tick and any player, the websocket
  client and listener have no connect or handshake timeout, and a
  WebTransport link never learns the datagram size the session negotiated.
  `docs/PLAN-hardening.md` phases 1 and 2 carry those.
- No replication, no RPC, no authority, no delta encoding. Inputs are
  predicted; state is not: no client-side prediction of the local player
  against a server, no reconciliation of the correction that comes back, no
  interpolation of the players a client does not own, no lag compensation,
  no interest management.
- No lobbies or matchmaking beyond a Gamend room.
- No script can open a session. `NetSession::new`, `add_peer`, `set_input`
  and `advance` are called by the two lockstep tests and `tests/faults.rs`
  and by nothing else; a script gets `rollback::input` and
  `rollback::is_resimulating`. `docs/PLAN-sessions.md` step 1.
- The roster is fixed at construction and every peer is the same peer: no
  end orders inputs or keeps their history, a closed link is never noticed,
  and nothing joins, leaves, reconnects or migrates. `docs/PLAN-sessions.md`
  steps 2 to 6.
- A run-time `scene.instantiate` still reuses the file's ids, so two
  instances of one scene collide.
- One reliable channel rather than many, so a big reliable message would
  block small ones behind it. Nothing sends one yet.
- An input older than the snapshot ring cannot be answered:
  `Session::stale_inputs` counts it and the caller has to resync.

## 1. Design

Four layers, each usable without the ones above it. The first three are
built — transport (one trait over websocket and QUIC), session (who is in the
game, at which tick, with which clock offset) and rollback (inputs on the
wire, a snapshot ring, the per-tick digest as the desync detector). The
fourth is the work:

**Replication.** A server simulates and sends deltas; clients apply, predict
their own node, and reconcile. Declared in the scene, not in code:

```toml
[[nodes]]
name = "Player"
id = "n_player"
replicate = { authority = "owner", components = ["transform", "body2d"], mode = "on_change" }
```

Change detection is a generation counter per `(entity, component)` written by
`components::patch`; the delta is the changed properties since the observer's
last acked tick, quantised off the schema's declared ranges. RPCs address a
node by `StableId`, never by path.

Replication's prediction and reconciliation reuse rollback's snapshot ring,
which is why rollback shipped first.

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
| Per-node restore of physics state, so a correction rewinds one body and not the world | Step 10; the rapier snapshot saves and restores a whole world, and reconciliation needs one body's |
| Server-ordered inputs, late join and spectators under lockstep, reconnect, host migration | `docs/PLAN-sessions.md` steps 3, 4 and 6 |
| Bots | Not engine code: a script writes a slot through `session::set_input_for` (`docs/PLAN-sessions.md` §2) |
| Voice | `docs/PLAN-voice.md`: its own datagram kind on the session link, never journaled |
| Client-authoritative hit registration | Not planned — the server decides a hit |

## 2. Transports

Every protocol a multiplayer engine could sit on, and where each stands here.

| Protocol | Decision |
| --- | --- |
| HTTP | Built. REST, Gamend, downloads. Not a game transport. |
| WebSocket | Built, binary frames included. Stays for turn-based games, lobbies, chat, and as the fallback where UDP is blocked. Wrong for anything twitchy: one lost packet stalls every frame behind it. |
| QUIC over WebTransport | Built natively (`balaur_webtransport` over `quinn`, through `web-transport-quinn`), and the transport under rollback and replication. The browser half is step 13. Client-server only: a browser cannot listen, and the server is an HTTP/3 endpoint with a certificate. |
| Raw UDP | Not exposed to scripts, now or later. No browser has it, it brings no encryption, congestion control or NAT story of its own, and every engine that ships it ends up writing a reliability layer on top. QUIC datagrams are that layer, standardised. |
| WebRTC data channels | Step 14, for one case: browser peer-to-peer without a relay. Needs a signalling channel (a Gamend room) and STUN/TURN; the unreliable-unordered mode gives UDP-like datagrams. Candidates are `matchbox` (native and browser, `matchbox_server` for signalling) or `webrtc` (webrtc-rs). Behind the same trait; not built until a game needs it. |
| Steam networking sockets and the Steam Datagram Relay | `docs/PLAN-steam.md` step 8, native only, behind the same trait: peer-to-peer through Valve's relay with no server and no certificate, which is what WebTransport cannot do. |
| A game server per lobby | Not a protocol but where the QUIC listener runs: `docs/PLAN-gamend.md` step S1 launches a headless engine per lobby and hands clients its address, which is how two NATed peers or two browsers (after step 13) meet without WebRTC. |
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

The numbering is the original plan's, so what is left keeps the names the
roadmap and ARCHITECTURE.md already use. A headless CI run of two engines on
loopback is the bar for 9 and 10, as it was for 5 and 6.

8. **Sessions from Gamend.** Split on 2026-09-05: the engine half is
   `docs/PLAN-sessions.md` (a session a script can open, roles, late join,
   reconnect, migration) and the server half is `docs/PLAN-gamend.md` (a
   game server per lobby, lobby tokens, rejoin, the match record, typed
   bindings). Done when `docs/PLAN-gamend.md` step E2 is: two engines
   matched by Gamend play on a server Gamend launched. First point at which
   a real game ships on this stack, which is why it comes before
   replication rather than after.
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
4. **Relay or direct.** Whether native rollback peers connect to each other
   (NAT traversal, no server cost) or always meet at a relay (one code path,
   browsers included). One code path is the default until latency says
   otherwise.
5. **Which nodes a client predicts.** Whether prediction follows from
   `authority = "owner"` or is its own key on `replicate`. A game that
   predicts a vehicle it is riding but not driving wants the second; every
   game wants the first to mean something sensible without saying so.
6. **Where the send rate is set.** A 60 Hz tick does not want 60 sends a
   second. Whether the rate is a `[webtransport]` default, per node, or per
   observer decides whether interpolation delay can be a fixed number of
   frames.
