> **Status:** step 1 built on 2026-09-18. `crates/balaur_multiplayer` gives
> scripts `multiplayer.*`: a host listens and takes slot 0, a joiner dials
> it, the lobby fills, and every machine loads the match scene and starts
> from the host's world. The host relays each player's inputs to the others
> without stamping them, which is step 2's job. Two engines in one process
> agree on every settled tick over a websocket and over QUIC
> (`crates/balaur_multiplayer/tests/match.rs`), and `examples/multiplayer`
> plays 900 ticks between two `balaur run` processes with no Rust written.
> Written on 2026-09-05 from the Photon parity investigation as
> `PLAN-sessions.md`. This is the engine half of `docs/PLAN-networking.md`
> step 8; `docs/PLAN-gamend.md` is the server half, and `docs/PLAN-voice.md`
> rides on the roster this plan defines.

# Plan: multiplayer

A match a script can open: host it, join it, leave it, and be told who else
is in it. Roles for the machine that orders inputs, late join, reconnect,
spectators, host migration, a binary wire format, and the editor's view of all
of it. Photon calls the same ground Realtime rooms, Fusion's Host and Server
modes, and Quantum's session runner; here it is one module, `multiplayer`,
over the rollback session that already exists.

## 0. Where the tree is today

| Have | Where |
| --- | --- |
| One link to one peer: a reliable ordered channel and datagrams, polled once per tick and recorded verbatim | `balaur_core::transport::Transport`, `Received` |
| A rollback session: journal, snapshot ring, prediction by repeating the last input, digest per tick, and players who left played as nil | `balaur_core::rollback::Session`, `set_absent` |
| The same session over links bound to the slots they speak for, relaying for a host, with ping, loss and bytes per link and a lead check that makes a fast machine wait | `balaur_core::netsession::NetSession`, `add_bound_peer`, `should_wait` |
| `multiplayer.*`: host, join, start, leave, bots, the roster, input, events, stats | `crates/balaur_multiplayer` |
| The frame hook a match steps the world through | `balaur_core::app::FrameDriver` |
| Nodes a digest leaves out: a camera or HUD that differs per machine | `digest::TAG_LOCAL`, `multiplayer.TAG_LOCAL` |
| Fault injection on every link from the editor's settings | `multiplayer/faults`, `delay`, `jitter`, `loss`, `transport::Faulty` |
| A recording that carries the bytes the peers delivered, with the link each came on | `PeerTraffic`, the `multiplayer` replay source |
| A websocket client and listener; a QUIC client and listener, native only | `balaur_websocket::listener`, `balaur_webtransport` (`bind`, `accept`) |
| Ids minted at run time that survive a rollback, and script fields holding a node that survive it too | `ids::mint`; nodes in a script's saved fields travel by stable id |

Missing:

- **Nobody orders inputs.** The host relays; each input still names its own
  tick. Late join, spectators and a server-side referee need an end that
  stamps and keeps the history (steps 2 and 3).
- **No headless server** (`balaur run --server`, step 3).
- **No replay of a recorded match.** A replay steps frames with `App::tick`
  and never reaches the frame driver, so a recording made in a match does
  not play back as one.
- **`--frames` runs and `balaur play` skip the driver too.** Their loops call
  `App::tick` unpaced, which is right for a smoke test and wrong for a match;
  `examples/multiplayer` quits by itself under `-- auto` instead.
- **Web builds leave `multiplayer` out.** A browser cannot listen, and joining
  over a websocket from a tab waits on web export.
- **JSON on the wire, one reliable channel.** The start snapshot rides the
  reliable channel in front of everything behind it (step 5).
- **Stats have no dock** (step 7).

## 1. Design

**Not Gamend.** Gamend is who is playing and the server that answers them:
accounts, lobbies, matchmaking, presence, hooks (`docs/PLAN-gamend.md`).
`multiplayer` is engines exchanging inputs sixty times a second. Neither
knows the other exists. A game whose lobby carries an address calls
`multiplayer::join` with that address, in its own script, and that is the
whole seam. Gamend can carry the bytes later if a game wants it to: its
WebRTC relay (`docs/PLAN-gamend.md` step S5) is one more `Transport`, and
the game server it launches per lobby (step S1) is a headless engine running
this module's `server` topology. Both are options a game picks, not a layer
this module stands on.

**A script never sends.** The engine owns the fixed tick: it gathers every
slot's input, predicts the missing ones, and when a late input lands it
restores a snapshot and runs the ticks again, calling the game's scripts as
it goes. So the module is Rust: a script cannot own the loop that re-runs
it, every byte a peer delivers is `ExternalIo`, and the sockets are the QUIC
and websocket crates. It is its own crate, `balaur_multiplayer`, because
core cannot reach the transport crates; core gives it one hook,
`FrameDriver`, which takes a live frame instead of `App::tick` while a match
plays. A re-run tick skips the Render stage. What a script does is set its
own input and read everyone's. When replication lands
(`docs/PLAN-networking.md` step 9), its RPC and its `replicate` component
join the same module, so a game that sends messages reaches for the same
word.

**How a match starts.** Lockstep needs one world on every machine, and a
lobby is not that: each machine got there by its own menus. So the start is
a load and a copy. The host loads the match scene, captures its world, and
sends it with `Start`; each joiner loads the same scene and restores that
world, which carries the RNG, the id counter, physics and every script's
fields. Both then run tick 1. A script field holding a node is written by
the node's stable id in every snapshot, so it names the same node on the
joiner, and after a rollback respawns it.

**What differs per machine.** A camera that follows this machine's player,
or a label saying which player you are, is meant to differ. A node tagged
`local`, and everything under it, is left out of the digest, so it cannot
read as a desync. It is still rolled back with the rest.

**Events report; they do not simulate.** Every script's
`on_multiplayer_event` hears each event once, at the top of a live tick. A
change to the world made from one is made on this machine only. What a
tick decides from belongs to the tick: `rollback::input`, and
`multiplayer::players()`, whose `status` is as of the tick being simulated.

**The name.** One word everywhere:

| Place | Name |
| --- | --- |
| The script module | `multiplayer::host`, `join`, `leave`, ... |
| The project table | `[multiplayer]` in `project.toml` |
| The editor's fault settings | `multiplayer/faults`, `delay`, `jitter`, `loss`, moved from `netcode/*`; a setting's scope is its own, so one category holds the project's keys and the person's |
| The script hook | `on_multiplayer_event`, on every script |
| The recording source | `multiplayer`, renamed from `session` |
| The dock | Multiplayer |
| The thing itself, in prose, docs and the editor | a *match* |

The Rust types keep their names: `NetSession`, `rollback::Session` and
`SessionStats` are never seen by a script. The names passed over:

| Name | Why not |
| --- | --- |
| `session` | A login in the Gamend addon (`save_session`, `restore_session`); a play run on disk is a recording |
| `netcode` | Names the technique, not the match |
| `net` | Beside `http` and `websocket` it reads as a socket library |
| `room` | Photon's word; a second name for what Gamend calls a lobby |
| `lobby` | Gamend's, and a lobby is who is waiting, not the running game |
| `match` | A Rune keyword, so it cannot name a module |

**One match, two topologies.** Every machine in a match runs the same
`Session`: the same tick, the same journal, the same ring. What a topology
decides is where the links go and who stamps an input with its tick.

| Topology | Who links to whom | Who orders inputs |
| --- | --- | --- |
| `host` | Every player to one player, who is also playing | Built as a relay: the host passes each input on as it came, and each still names its own tick. Step 2 makes the host stamp and rebroadcast |
| `server` | Every player to a headless engine that is not a player | The server, which also simulates, verifies digests and holds the history (step 3) |

`host` is Photon Fusion's Host mode; `server` is its Server mode and
Quantum's custom server at once, because a headless Balaur is the same
simulation. A mesh of every player to every other was the tests' shape and
is not built for scripts: the relay does the same job over one link each.

**Roles answer for this machine.** `multiplayer::role()` is `ROLE_HOST`
(the listening player) or `ROLE_CLIENT` (a player linked to a host), and
nil when idle. `ROLE_SERVER` and `ROLE_SPECTATOR` come with steps 3 and 4.

**Server-ordered inputs.** Under `host` and `server`, a player sends "this is
my input, as early as I could", and the ordering end answers with the tick it
was assigned and rebroadcasts it to everyone. That settles three things the
relay cannot: which of two disagreeing peers is right, how far ahead of the
others a fast client may run, and where the whole input history lives,
which is what a late joiner and a spectator replay from. Until then a
machine running more than two ticks ahead of the slowest peer, or near the
edge of the snapshot ring, waits a frame.

**A player is bound to a link.** The host assigns slots at the handshake, as
Photon assigns actor numbers, and a link may submit inputs for its own slot
only; a joiner's one link to the host speaks for every slot but its own.

**Lifecycle.**

- *Join.* `Hello` carries the protocol, a name and a token; `Welcome`
  carries the slot and the roster, and `Refuse` the reason. A join that is
  not welcomed inside `multiplayer/timeout` fails with `EVENT_FAILED`. The
  lobby starts when `players` slots are filled, or when the host calls
  `start`. Joining a running match is step 4.
- *Leave.* A slot whose player left is absent: from the tick after its last
  input the match feeds it nil rather than predicting, and keeps the slot so
  ids and history stay stable. The host notices a closed link, a goodbye, or
  a link silent for `timeout`, and tells the others with the leaver's last
  inputs attached. A host leaving ends the match.
- *Reconnect.* A link that closes keeps its slot for a grace period, and a
  new link presenting the same token resumes it (step 4).
- *Host migration.* When the host's link closes, the next roster member
  becomes the host and everyone re-links to it. Under lockstep every machine
  already holds the same state, so migration is a roster edit and new
  links; under replication (`docs/PLAN-networking.md` step 9) the new host
  restores its newest confirmed snapshot and the rest rejoin from it.
- *Spectators.* A link with no slot. It receives inputs (lockstep) or deltas
  (replication), simulates a few ticks behind the newest confirmed one, and
  never sends. A spectator is what a match replay from the server is, live.

**The script surface.** `multiplayer` beside `rollback`: `multiplayer` owns
who is in and how they are reached, `rollback` owns what they pressed.

```rune
pub fn host(this) {
    // Listen, then hand the address out however the game likes.
    let at = multiplayer::host(#{ transport: multiplayer::TRANSPORT_WEBTRANSPORT });
    log::info(`${at["url"]} ${at["cert_hash"]}`);
}

pub fn join(this, url, hash) {
    multiplayer::join(url, #{ cert_hash: hash, name: "Guest" });
}

pub fn fixed_update(this, dt) {
    multiplayer::set_input(#{ x: input::action_value("move_x") });
    let me = rollback::input(multiplayer::local_player());
}

pub fn on_multiplayer_event(this, e) {
    if e["kind"] == multiplayer::EVENT_DESYNC {
        log::error(`the machines disagree from tick ${e["tick"]}`);
    }
}
```

| Call | Answers |
| --- | --- |
| `multiplayer::host(options)` | Listens and takes slot 0; returns `#{ url, cert_hash, transport }`, the hash only for a self-signed QUIC listener. `options` override the `[multiplayer]` table and add `name` and `token` |
| `multiplayer::join(url, options)` | Dials a `ws://`, `wss://` or `https://` host; `options` carry `cert_hash`, `name`, `token` and `timeout` |
| `multiplayer::start()`, `add_bot(name)` | On a host: start with whoever is in; a slot the host's own script plays |
| `multiplayer::leave()` | Say goodbye and go idle |
| `multiplayer::set_input(value)`, `set_input_for(slot, value)` | The input for the next tick, kept until set again; the second form drives a bot's slot |
| `multiplayer::players()`, `local_player()`, `role()`, `state()` | The roster as `{ slot, name, bot, local, status }`; this machine's slot; `ROLE_*`; `STATE_IDLE`, `CONNECTING`, `LOBBY` or `PLAYING` |
| `multiplayer::tick()`, `settled_tick()` | The tick being simulated, and the tick before which nothing can be taken back (`rollback::Clock`) |
| `multiplayer::stats(slot)` | `rtt_ms`, `loss`, `bytes_in`, `bytes_out` for the link a slot is reached over: an observer, never hashed |

Words come from constants, as everywhere: `TRANSPORT_*`, `ROLE_*`,
`STATE_*`, `EVENT_*`, `STATUS_*` and `TAG_LOCAL`, from one `vocabulary`
module in the crate. `EVENT_CONNECTED` and `EVENT_FAILED` answer this
machine's own join; `EVENT_JOINED` and `EVENT_LEFT` are another slot coming
or going; `EVENT_STARTED` is the match scene loaded; `EVENT_CLOSED` is this
machine's lobby or match ending, with a `reason`.

```toml
[multiplayer]
transport = "webtransport"   # or "websocket"; the fallback where UDP is blocked
address = "127.0.0.1:0"      # where a host listens; 0.0.0.0 for other machines
players = 2                  # slots filled before the match starts on its own
scene = "scenes/arena.toml"  # what every machine loads; empty for the main scene
depth = 16                   # snapshot ring: the furthest rollback, in ticks
timeout = 5.0                # seconds a join may take, or a link stay silent
```

**Wire format.** Messages stay serde types and move from JSON to bincode 2
(already a workspace dependency) behind one version byte, so a recording
from before the change still decodes. The reliable side gains a stream id:
snapshots travel on their own stream, so a late joiner's world never queues
in front of a digest. `Transport` grows `send_reliable_on(stream, bytes)`
with `0` as today's channel; the websocket implementation multiplexes with a
prefix, QUIC opens a stream.

**Determinism.** Inputs and absences enter through `PeerTraffic`, which is
`ExternalIo`, so they are recorded and never sent during a re-simulated
tick. Stats and round trips stay outside the digest. The lobby's handshake is
not recorded: the start snapshot is where a match's recording would begin.

**The editor.** A Multiplayer dock: one row per link with round trip, loss
and bytes, plus rollbacks per second, stale inputs, the settled tick, and
the desync tick with a button that runs `replay --entries-at` on it. And
"Play as two": the editor launches a second instance of the project with
`balaur run --headless` (or `--offscreen`, to see both), joins it over
loopback with the fault settings on, and shows both in the dock. Fusion's
multi-peer mode does this inside one Unity process; two processes are
honest about the boundary and reuse the CLI.

## 2. The surface

Every feature Photon Realtime, Fusion and Quantum ship, and the decision on
each.

| Feature | Decision |
| --- | --- |
| Host, join, leave from a script; events; the `[multiplayer]` table | Have, step 1 |
| Player slot bound to a link, roster handshake | Have, step 1 |
| A player leaving; absent slots played as nil | Have, step 1 |
| Stats readable from a script | Have, step 1 |
| Journal bounds, unknown players and far ticks refused, join timeout | Have |
| `max_datagram` read from the link a match negotiated | Step 5, with the wire format |
| Host that stamps inputs | Step 2 |
| Server role: a headless engine that orders inputs and simulates | Step 3 |
| Server-ordered inputs and the input history | Step 3 |
| Server-side digest verification, so a modified client is refused rather than trusted | Step 3. A lockstep game's only anti-cheat for state; hidden information stays impossible under lockstep (`docs/PLAN-networking.md` §4.1) |
| Late join from a snapshot, for lockstep | Step 4. Replication's join in progress is `docs/PLAN-networking.md` step 9 on the same stream |
| Reconnect with a grace period | Step 4 |
| Spectators | Step 4 |
| bincode with a version byte; a stream id on the reliable side | Step 5 |
| Host migration | Step 6, lockstep first; replication's variant lands with `docs/PLAN-networking.md` step 9 |
| Multiplayer dock; "Play as two" | Step 7 |
| Replay of a recorded match | Step 8 |
| Bots | Have: `add_bot` on a host, played through `set_input_for`; behaviour trees and state machines are a game's, or a plugin's |
| Offline mode | Have: a match of one player with no link is today's `Session` |
| Lag simulation | Have: `multiplayer/*` |
| Encryption | Have: QUIC is TLS, and the websocket is `wss` |
| Master client | Have as the host slot; under `server` there is none, and a game that wants one elects it in script from the roster |
| Lobbies, matchmaking, who is online, room and player properties | Gamend, `docs/PLAN-gamend.md`. The match never learns a lobby exists; a game's script carries the address from one to the other |
| Gamend as the transport | Later, if a game wants it: the WebRTC relay (`docs/PLAN-gamend.md` step S5) behind `Transport`, or the game server Gamend launches (step S1) running `server` |
| Event cache for late joiners | Not needed: a late joiner receives the snapshot and the inputs since, which is every event |
| Interest groups on a relay | `docs/PLAN-networking.md` step 12 for replication; under lockstep every peer needs every input |
| NAT traversal, a relay | `docs/PLAN-gamend.md` (a game server per lobby), `docs/PLAN-steam.md` step 8 (the Steam Datagram Relay), `docs/PLAN-networking.md` step 14 (WebRTC) |
| A browser in a match | Blocked on `docs/PLAN-networking.md` step 13 and web export; the web templates leave `multiplayer` out until then |
| Deterministic pathfinding for a lockstep game | Not this plan; the roadmap's navigation row carries the constraint |

## 3. Steps

Two or three engines in one process on loopback, behind `transport::Faulty`,
is the bar for every step, as it was for the lockstep tests.

1. **A match from a script — built.** `crates/balaur_multiplayer`: the
   module, `host` and `join` over both transports, the handshake with slots
   bound to links, `start`, bots, `leave` and absent slots, events, `stats`,
   the `[multiplayer]` table, and the start from the host's world. In core:
   `FrameDriver`, `NetSession`'s bound links, relay and lead check,
   `Session::set_absent`, the `local` digest tag, script fields that keep
   their nodes across a snapshot, and the `netcode/*` settings and the
   `session` replay source renamed. Ends with: `examples/multiplayer` played
   by two `balaur run` processes, and the match test over both transports.
2. **Host.** The host stamps each input with the tick it will run on and
   rebroadcasts it. Ends with: three players through one host, under loss.
3. **Server.** `balaur run --server`: headless, no slot, ordering inputs,
   simulating, keeping the input history, and comparing every client's
   digest to its own. A client whose digest differs is told and dropped.
   It prints its bound port and certificate hash on start, which is what
   `docs/PLAN-gamend.md` step S1 reads. Ends with: a server refereeing two
   clients, one of them modified.
4. **Late join, reconnect, spectators.** The snapshot stream, the inputs
   since, the grace period, the resume token. Ends with: a client killed
   mid-game restarts and is back at the live tick.
5. **Wire format.** bincode behind a version byte, the stream id, the
   datagram size read from the link, bytes per tick measured in
   `examples/benchmark`. Ends with: a number in `docs/BENCHMARKS.md`.
6. **Host migration.** Ends with: the host killed, the game continues.
7. **The editor.** The Multiplayer dock and "Play as two".
8. **Recording a match.** A replay drives recorded frames through the frame
   driver, and a re-run tick records no frame of its own; the `--frames`
   and `balaur play` loops step through it too. Ends with: a match recorded
   on each machine replays to the same digests.

## 3b. What a digest covers, and why a session in the editor once could not

`digest::entries` narrows its node walk to `eng.debug_scope()`, so inside an
editor the game is the subtree checked and the editor's own nodes are not.
The sources a plugin registers did not know that, and animation and physics
reported every node in the world.

So `test:recordings` never verified: the game reproduced exactly, every shared
label matching, and the replay carried one extra entry, a tween on the
editor's own bottom dock, which animates when play opens the Output dock.
`digest::scope_of` hands a source the same scope. Fixed 2026-09-07. A
subtree tagged `local` is left out of the same scope.

A source added later has to ask for the scope too; nothing makes it.

## 4. What CI can prove

Loopback with injected loss, delay and jitter, which is where the twelve-tick
repeat was found to be necessary, so it is not a weak bar. Every step ends
with a headless test of two or three engines, and step 3's adds a fourth
process. What CI cannot: NAT, a real route, a browser, a phone on a cellular
link. Those wait for `docs/PLAN-gamend.md`'s game server on a real host.

## 5. Open questions

1. **A joining player's first tick.** A joiner starts at tick 1 a round
   trip after the host, and the lead check holds the host back until they
   are level. Whether the ordering end should instead hand a joiner a tick a
   round trip ahead is step 2's to measure.
2. **The absent input.** Nil today. A game with a "hold to block" input may
   want a value it declares in `[multiplayer]`.
3. **Where the resume token comes from.** Minted by the ordering end, or a
   token the game brought from outside, a Gamend lobby's for one. The
   module takes whatever `options.token` holds and never asks where it came
   from.
4. **Events inside the simulation.** Events reach scripts once, outside a
   re-run, so a script that changes the world from one changes this machine
   only. Whether `EVENT_LEFT` should instead reach scripts inside the tick
   it applies to, re-delivered on every re-run, waits on a game that needs
   it.
