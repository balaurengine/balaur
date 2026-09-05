> **Status:** not started. Written on 2026-09-05 from the Photon parity
> investigation: rollback works between two engines in a Rust test and
> nothing else can reach it. This is the engine half of
> `docs/PLAN-networking.md` step 8; `docs/PLAN-gamend.md` is the server half,
> and `docs/PLAN-voice.md` rides on the roster this plan defines. Prerequisite
> rows in `docs/PLAN-hardening.md` (animation outside the snapshot, widget
> clicks in the digest, the held-frame clock, the unbounded journal, the
> missing handshake timeouts, `max_datagram` never raised) land with step 1 or
> before it, because a session with one animated node desyncs today.

# Plan: sessions

A session a script can open: host it, join it, leave it, and be told who else
is in it. Roles for the machine that orders inputs, late join, reconnect,
spectators, host migration, a binary wire format, and the editor's view of all
of it. Photon calls the same ground Realtime rooms, Fusion's Host and Server
modes, and Quantum's session runner; here it is one module over the rollback
session that already exists.

## 0. Where the tree is today

| Have | Where |
| --- | --- |
| One link to one peer: a reliable ordered channel and datagrams, polled once per tick and recorded verbatim | `balaur_core::transport::Transport`, `Received` |
| A rollback session: journal, snapshot ring, prediction by repeating the last input, digest per tick | `balaur_core::rollback::Session` |
| The same session over links: inputs unreliable with a twelve-tick repeat, digests reliable, ping, loss and bytes per link | `balaur_core::netsession::NetSession`, `LinkStats`, `SessionStats` |
| Fault injection on every link from the editor's settings | `netcode/faults`, `netcode/delay`, `netcode/jitter`, `netcode/loss`, `transport::Faulty` |
| A recording that carries the bytes the peers delivered, so a networked desync replays from a file | `PeerTraffic`, the `session` replay source |
| A websocket client and listener; a QUIC client and listener, native only | `balaur_websocket::listener`, `balaur_webtransport` (`bind`, `accept`) |
| A run with no window | `balaur run --headless`, `--offscreen`, `--fixed-tick` |
| Ids minted at run time that survive a rollback | `ids::mint`, the id counter in the snapshot |
| What a script can ask | `rollback::input(player)`, `rollback::is_resimulating()` |

Missing:

- **No script can open a session.** `NetSession::new`, `add_peer`,
  `set_input` and `advance` have three callers: the two lockstep tests and
  `tests/faults.rs`. A game written in Rune cannot play against another.
- **The roster is fixed and unchecked.** The player list is an argument to
  `new`, a datagram may name any player and any tick, a link that closes is
  never noticed, and nothing joins, leaves, reconnects or migrates.
- **Every peer is the same peer.** There is no end that orders inputs, keeps
  the input history, or holds a snapshot for someone arriving late. Late
  join, spectators and a server-side referee all need that end.
- **Stats go nowhere.** `SessionStats` is a resource with no script call and
  no dock.
- **JSON on the wire, one reliable channel.** Documented as fine until the
  wire is the bottleneck; a snapshot for a late joiner is where it becomes
  one, since it would sit in front of every digest behind it.
- **No `[session]` table.** Transport, player count, ring depth and timeouts
  are constructor arguments.

## 1. Design

**One session, three roles.** Every machine in a session runs the same
`Session`: the same tick, the same journal, the same ring. What a role
decides is where the links go and who stamps an input with its tick.

| Role | Who links to whom | Who orders inputs |
| --- | --- | --- |
| `peer` | Everyone to everyone; what the tests do today | Nobody: each input names its own tick, and the twelve-tick repeat repairs loss |
| `host` | Every player to one player, who is also playing | The host: it stamps each input with the tick it will run on and rebroadcasts |
| `server` | Every player to a headless engine that is not a player | The server, which also simulates, verifies digests and holds the history |

`host` is Photon Fusion's Host mode; `server` is its Server mode and
Quantum's custom server at once, because a headless Balaur is the same
simulation. `peer` stays for two native machines that can reach each other,
and for the tests. The script API is identical under all three.

**Server-ordered inputs.** Under `host` and `server`, a player sends "this is
my input, as early as I could", and the ordering end answers with the tick it
was assigned and rebroadcasts it to everyone. That settles three things the
mesh cannot: which of two disagreeing peers is right, how far ahead of the
others a fast client may run (`INPUT_WINDOW` becomes a rule the server
enforces rather than a courtesy), and where the whole input history lives,
which is what a late joiner and a spectator replay from. Under `peer` the
protocol is today's.

**A player is bound to a link.** The ordering end assigns player slots at the
handshake, as Photon assigns actor numbers, and a link may submit inputs for
its own slot only. Under `peer`, both ends derive the slots from a roster
they were both given (a lobby's member list), so they agree without an
authority. The hardening plan's "bind `player` to the link" is this rule.

**Lifecycle.**

- *Join.* A handshake on the reliable channel carries the roster, the
  session settings and the tick. A player joining a running session receives
  the newest snapshot the ordering end holds, on a reliable stream of its
  own, then the inputs since, then runs to the live tick.
- *Leave.* A slot whose player left is `absent`: the session feeds it a
  game-declared default input rather than predicting, and keeps the slot so
  ids and history stay stable.
- *Reconnect.* A link that closes keeps its slot for
  `session/reconnect_grace` seconds. A new link presenting the same token
  resumes the slot and joins as above. Past the grace the slot is `absent`.
- *Host migration.* When the host's link closes, the next roster member
  becomes the host and everyone re-links to it. Under lockstep every machine
  already holds the same state, so migration is a roster edit and new
  links; under replication (`docs/PLAN-networking.md` step 9) the new host
  restores its newest confirmed snapshot and the rest rejoin from it.
- *Spectators.* A link with no slot. It receives inputs (lockstep) or deltas
  (replication), simulates a few ticks behind the newest confirmed one, and
  never sends. A spectator is what a match replay from the server is, live.

Every lifecycle message travels the reliable channel through `PeerTraffic`,
so a recording replays a join, a migration and a desync exactly.

**The script surface.** A `session` module beside `rollback`: `session` owns
who is in and how they are reached, `rollback` owns what they pressed.

```rune
pub async fn init(this) {
    // Host: listen, then hand the address out through Gamend or a friend.
    this.session = session::host(this.node, #{
        players: 2,
        transport: session::TRANSPORT_WEBTRANSPORT,
    });
    // Or join one.
    this.session = session::join(this.node, "https://host:4433", #{ token: token });
}

pub fn fixed_update(this, dt) {
    session::set_input(#{ x: input::axis("move"), jump: input::just_pressed("jump") });
    let me = rollback::input(session::local_player());
}

// joined, left, reconnecting, resumed, migrated, desync, closed
pub fn on_session(this, e) {
    if e["kind"] == session::EVENT_DESYNC { log::error(`tick ${e["tick"]}`); }
}
```

| Call | Answers |
| --- | --- |
| `session::host(node, options)` / `join(node, url, options)` / `leave()` | The lifecycle; `options` override the `[session]` table |
| `session::set_input(value)`, `session::set_input_for(player, value)` | This tick's input; the second form drives a local bot's slot |
| `session::players()`, `local_player()`, `role()` | The roster with ids, names and `present` / `absent` / `reconnecting`; which slot is this machine's; `ROLE_PEER` / `ROLE_HOST` / `ROLE_SERVER` / `ROLE_SPECTATOR` |
| `session::tick()`, `settled()` | The tick about to run, and the tick before which nothing can be taken back (`rollback::Clock`) |
| `session::stats(player)` | `rtt_ms`, `loss`, `bytes_in`, `bytes_out`, `rollbacks`, `stale_inputs`: an observer, never hashed |

Words come from constants, as everywhere: `TRANSPORT_*`, `ROLE_*`,
`EVENT_*`, `STATUS_*` in one `vocabulary` module in core and installed as
script constants.

```toml
[session]
transport = "webtransport"   # or "websocket"; the fallback where UDP is blocked
players = 2
depth = 16                   # snapshot ring: the furthest rollback, in ticks
reconnect_grace = 30.0       # seconds a closed link keeps its slot
input_window = 120           # how far ahead of the settled tick a peer may run
```

**Wire format.** Messages stay serde types and move from JSON to bincode 2
(already a workspace dependency) behind one version byte, so a recording
from before the change still decodes. The reliable side gains a stream id:
snapshots travel on their own stream, so a late joiner's world never queues
in front of a digest. `Transport` grows `send_reliable_on(stream, bytes)`
with `0` as today's channel; the websocket implementation multiplexes with a
prefix, QUIC opens a stream.

**Determinism.** Nothing changes: inputs, roster edits and snapshots enter
through `PeerTraffic`, which is `ExternalIo`, so they are recorded and
replayed and never sent during a re-simulated tick. Stats and round trips
stay outside the digest. The recording header gains the roster and the role,
so a replay of a server's recording is the match.

**The editor.** A Network dock: one row per link with round trip, loss and
bytes, plus rollbacks per second, stale inputs, the settled tick, and the
desync tick with a button that runs `replay --entries-at` on it. And "Play
as two": the editor launches a second instance of the project with
`balaur run --headless` (or `--offscreen`, to see both), joins it over
loopback with the fault settings on, and shows both sessions in the dock.
Fusion's multi-peer mode does this inside one Unity process; two processes
are honest about the boundary and reuse the CLI.

## 2. The surface

Every session feature Photon Realtime, Fusion and Quantum ship, and the
decision on each.

| Feature | Decision |
| --- | --- |
| Host, join, leave from a script; events; the `[session]` table | Step 1 |
| Player slot bound to a link, roster handshake | Step 1 |
| A player leaving; absent slots with a default input | Step 1 |
| Stats readable from a script | Step 1 |
| Journal bounds, handshake and connect timeouts, `max_datagram` from the negotiated session | Step 1, from `docs/PLAN-hardening.md` |
| Host role | Step 2 |
| Server role: a headless engine that orders inputs and simulates | Step 3 |
| Server-ordered inputs and the input history | Step 3 |
| Server-side digest verification, so a modified client is refused rather than trusted | Step 3. A lockstep game's only anti-cheat for state; hidden information stays impossible under lockstep (`docs/PLAN-networking.md` §4.1) |
| Late join from a snapshot, for lockstep | Step 4. Replication's join in progress is `docs/PLAN-networking.md` step 9 on the same stream |
| Reconnect with a grace period | Step 4 |
| Spectators | Step 4 |
| bincode with a version byte; a stream id on the reliable side | Step 5 |
| Host migration | Step 6, lockstep first; replication's variant lands with `docs/PLAN-networking.md` step 9 |
| Network dock; "Play as two" | Step 7 |
| Bots | Not engine code. A bot is a script writing a slot through `set_input_for`; behaviour trees and state machines are a game's, or a plugin's |
| Offline mode | Have: a session of one player with no link is today's `Session` |
| Lag simulation | Have: `netcode/*` |
| Encryption | Have: QUIC is TLS, and the websocket is `wss` |
| Master client | Have as the host slot under `host`; under `server` there is none, and a game that wants one elects it in script from the roster |
| Room and player properties | `docs/PLAN-gamend.md`: a lobby's data and its members' metadata; not session state |
| Event cache for late joiners | Not needed: a late joiner receives the snapshot and the inputs since, which is every event |
| Interest groups on a relay | `docs/PLAN-networking.md` step 12 for replication; under lockstep every peer needs every input |
| NAT traversal, a relay | `docs/PLAN-gamend.md` (a game server per lobby), `docs/PLAN-steam.md` step 8 (the Steam Datagram Relay), `docs/PLAN-networking.md` step 14 (WebRTC) |
| A browser in a session | Blocked on `docs/PLAN-networking.md` step 13 and web export; the websocket transport works from a tab once the canvas does |
| Deterministic pathfinding for a lockstep game | Not this plan; the roadmap's navigation row carries the constraint |

## 3. Steps

Two or three engines in one process on loopback, behind `transport::Faulty`,
is the bar for every step, as it was for the lockstep tests.

1. **A session from a script.** The `session` module, `host` and `join` over
   both transports, the roster handshake with slots bound to links, `leave`
   and absent slots, events, `stats`, and the `[session]` table. The
   hardening rows land here: the journal pruned below the ring, unknown
   players and far ticks refused, timeouts on connect and handshake, the
   datagram size read from the session. Ends with: an example two
   `balaur run` processes play on one machine, with no Rust written.
2. **Host.** One player listens, stamps and rebroadcasts. Ends with: three
   players through one host, under loss.
3. **Server.** `balaur run --server`: headless, no slot, ordering inputs,
   simulating, keeping the input history, and comparing every client's
   digest to its own. A client whose digest differs is told and dropped.
   Ends with: a server refereeing two clients, one of them modified.
4. **Late join, reconnect, spectators.** The snapshot stream, the inputs
   since, the grace period, the resume token. Ends with: a client killed
   mid-game restarts and is back at the live tick.
5. **Wire format.** bincode behind a version byte, the stream id, bytes per
   tick measured in `examples/benchmark`. Ends with: a number in
   `docs/BENCHMARKS.md`.
6. **Host migration.** Ends with: the host killed, the game continues.
7. **The editor.** The Network dock and "Play as two".

## 4. What CI can prove

Loopback with injected loss, delay and jitter, which is where the twelve-tick
repeat was found to be necessary, so it is not a weak bar. Every step ends
with a headless test of two or three engines, and step 3's adds a fourth
process. What CI cannot: NAT, a real route, a browser, a phone on a cellular
link. Those wait for `docs/PLAN-gamend.md`'s game server on a real host.

## 5. Open questions

1. **The name.** `session` matches `NetSession` and `SessionStats`, and the
   editor's Session dock already means a recording. `netcode` is the settings
   category and would make `netcode/faults` and `netcode::join` one word.
   Decide before step 1; renaming a script module later costs every game.
2. **A joining player's first tick.** Whether the ordering end assigns the
   next unsettled tick, or a tick a round trip ahead so the first input is
   not already late.
3. **Whether `peer` survives.** Once `host` exists, a mesh is a second code
   path for two native machines. One code path is the default until latency
   between two peers behind one router says otherwise.
4. **The absent input.** Nil, the last one, or a value the game declares in
   `[session]`. Nil is the honest default; a game with a "hold to block"
   input wants the third.
5. **Where the resume token comes from.** Minted by the session, or the
   Gamend session token the lobby already issued. `docs/PLAN-gamend.md` says
   the second, so a player is the same person to the lobby and the game.
