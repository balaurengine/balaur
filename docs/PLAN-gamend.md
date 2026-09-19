> **Status:** not started. Written on 2026-09-05 from the Photon parity
> investigation. Two halves: what the Gamend server gains, which is done in
> the `appsinacup/gamend` repository and planned here so the two sides agree;
> and what `crates/balaur_gamend` gains, which is done here. The server steps
> are the server half of `docs/PLAN-networking.md` step 8; the engine's
> match itself is `docs/PLAN-multiplayer.md`.

# Plan: Gamend

Gamend is the backend beside the engine: accounts, lobbies, parties,
friends, chat, matchmaking, quests, leaderboards, payments, storage, server
hooks. Photon Realtime is the part of Photon that does the same job, and
Gamend already does more of it. What Gamend does not do is carry a game: its
realtime is Phoenix channels over a TCP websocket, and a lobby has no address a
game could connect to. This plan gives a lobby a game server and a token a
member carries into the match, and gives scripts the whole API as typed
calls rather than a path string.

## 0. Where the two trees are today

Gamend, verified in the tree on 2026-09-05:

| Have | Where |
| --- | --- |
| Auth: email and password, magic link, OAuth for Discord, Google, Apple, Facebook and Steam, JWT API tokens, sessions | `Gamend.Accounts`, `oauth`, `session_controller` |
| Lobbies: create, join, leave, kick, update by host, list and filter, hidden, locked, password, max users, live `updated` and `user_updated` events, server time | `Gamend.Lobbies`, `LobbyChannel`, `LobbiesChannel` |
| Parties of two to ten, invites, lobby integration | `Gamend.Parties`, `PartyChannel` |
| Friends, blocking; groups with roles and join requests | `Gamend.Friends`, `Gamend.Groups`, `GroupChannel` |
| Chat for lobby, group, party and DMs, read cursors, unread counts, mutes | `Gamend.Chat`, `chat_mute_controller` |
| Notifications, push over FCM and APNs, device tokens | `Gamend.Notifications`, `push_token_controller` |
| Ticket matchmaking: exact `match_params`, `min_players` / `max_players`, parties as one unit, a hidden lobby per match, three hooks (`after_matchmaking_join`, `matchmaking_form_matches`, `after_matchmaking_matched`), ready check | `Gamend.Matchmaking`, `Worker`, `ready_check_controller` |
| Quests and achievements, leaderboards, tournaments, economy and inventory, payments for Stripe, Play, App Store and Steam, a key-value store with `kv:subscribe` | one context each under `apps/gamend_core/lib/gamend` |
| Server hooks in Elixir or compiled GDScript, background and scheduled jobs, object storage, an admin portal, analytics, client logs, ip bans, rate limits | `Gamend.Hooks`, `Jobs`, `storage_controller`, `admin` |
| Presence by CRDT across a cluster, used for signaling rooms and lobby spectators | `Gamend.Presence`, `cluster.ex` |
| WebRTC signaling: `offer`, `answer`, `ice`, `list_users`, `broadcast_offer` for a star host; `user_rejoined` when a socket returns | `SignalingChannel`, `Gamend.Signaling` |
| A server-side WebRTC peer per user over `ex_webrtc` 0.17, one data channel, JSON or protobuf, carrying hook RPC only, rate limited | `GamendWeb.WebRTCPeer`, `webrtc:offer` / `ice` / `close` on `UserChannel` |
| Lobby snapshots: a timeline of lobby state and the events that changed it, off by default | `Gamend.LobbySnapshots` |
| An OpenAPI document and a realtime protobuf, and SDKs generated from them for JavaScript and Godot | `clients/`, `openapitools.json`, `gamend_realtime.pb.js` |
| Deployment on Fly or Docker, Prometheus and Grafana | `fly.toml`, `docker-compose*.yml` |

Gamend's own roadmap, which this plan does not repeat: skill matchmaking
with ratings and widening bands, cloud saves, signed webhooks and remote
config, event tracking, economy extensions, "generalize the WebRTC layer",
KV prefix queries and streaming, Discord notifications, Unity and Unreal SDKs.

The engine:

| Have | Where |
| --- | --- |
| `gamend::configure`, `login`, `rest`, `connect`, `join`, `push`, `leave`, `call_hook`, `close`: nine calls, all delivered once per tick and replayable | `crates/balaur_gamend/src/lib.rs` |
| Phoenix Channels V2 over the websocket, Fetch and WebSocket in the browser, a refusing stub on emscripten | `client/phoenix.rs`, `browser.rs` |
| Tests against a real server, `GAMEND_URL` or gamend.org, in the e2e suite: the public API, and accounts that register by device, sign in again, open the socket and delete themselves | `crates/balaur_gamend/tests` |
| The plans that already hand Gamend a job: Steam and Google sign-in verification, purchase verification, web hosting of a game | `docs/PLAN-steam.md` step 2, `docs/PLAN-google.md` steps 2, 5, 6, `docs/PLAN-deploy.md` step 3 |

Missing:

- **A game traffic path.** Everything realtime is JSON over one TCP
  websocket. The WebRTC data channel exists and is ordered, reliable, single,
  rate limited and carries only hook calls. Nothing gives a lobby datagrams.
- **A game server.** A lobby has members and data but no address, no
  process ordering its inputs, no snapshot for a late joiner, nothing that
  verifies what a client claims.
- **A typed client.** Every Gamend feature above is reachable from a script
  only as `gamend::rest("GET", "/api/v1/lobbies")` or
  `gamend::push(socket, topic, event, payload)`. Gamend's JavaScript and Godot
  SDKs are typed; Photon's client is typed; a Balaur script guesses paths.
- **Rejoin at the lobby.** A socket closing cancels the user's matchmaking
  tickets (`UserChannel.terminate`). The signaling room keeps a user across a
  reconnect; a lobby membership has no grace.
- **A match record.** Lobby snapshots record lobby state, not the game. The
  engine writes a recording that replays the match bit for bit and nothing
  uploads it.
- **Regions.** One deployment, one place. A cluster spreads load, not
  latency.

## 1. Design

**Gamend is not the match.** Gamend is who is playing and the server that
answers a hook; the match is `docs/PLAN-multiplayer.md`, engines exchanging
inputs at tick rate. This plan gives a lobby a game server and a token, and
a game's own script hands the one to the other. Nothing in the addon opens
a match, and nothing in the match knows a lobby exists; if a game later
wants its match traffic to go through Gamend, that is step S5, a transport,
not a merge.

**A lobby is the roster; the game does not go through Phoenix.** The BEAM is
the right place for ten thousand idle sockets and the wrong place for sixty
datagrams a second per player through a JSON channel. Three ways to give a
lobby a game transport:

| Option | What it is | Decision |
| --- | --- | --- |
| A headless Balaur game server per lobby | Gamend launches `balaur run --server` when a lobby starts, records its address and certificate hash on the lobby, and clients connect to it directly over QUIC. The server is the same engine, so ordering, digests, late join and the match recording come from `docs/PLAN-multiplayer.md` steps 3 and 4 with no Elixir networking code | **The plan.** Costs a process per lobby beside Gamend, a public UDP port range, and a browser waits for `docs/PLAN-networking.md` step 13 |
| The WebRTC peer generalised into a relay | `ex_webrtc` 0.17's `DataChannel` takes `ordered: :unordered` and `max_retransmits: 0`, which is a datagram. A per-lobby star with the server as hub relays game payloads instead of hook calls; the `@max_data_channels 1` cap and the per-user rate limit move to a per-lobby budget | The browser fallback, and already on Gamend's roadmap. The engine side is `docs/PLAN-networking.md` step 14 on native (`webrtc` or `matchbox`) and the browser's own API on the web. Every packet crosses the BEAM |
| QUIC in Elixir (`quicer` over msquic) | Raw QUIC, without the HTTP/3 and WebTransport framing the engine's client speaks, so it would need a second transport on the engine side too | Not planned |
| Raw UDP from Elixir (`:gen_udp`) | No encryption, no congestion control, no browser | Not planned, for the reasons `docs/PLAN-networking.md` §2 gives |

**The game server's life.** A `game_servers` table: lobby id, host, port,
certificate hash, status, started and last-seen. A hook on lobby start (the
host's "start" or matchmaking's `after_matchmaking_matched`) spawns
`balaur run --server --lobby <id> --token-secret <..>` through a `Port`
under a supervisor, waits for the process to print its bound port and
certificate hash, and writes the row. The lobby's `updated` event carries the
address; a client that sees it calls `multiplayer::join`, in the game's own
script. The process heartbeats over REST; a missed heartbeat or a closed
lobby ends it. Where it runs is open question 1; the first answer is the
Gamend machine itself, because a Fly machine already has a public address
and a cluster of them is what Gamend's `cluster.ex` federates.

**One identity, bound to the link.** The join token is a lobby-scoped JWT
Gamend issues to a member: user id, lobby id, slot. The game server presents
it to Gamend once (`POST /api/v1/match/verify`, or a hook call) and binds
the slot to the link, which is `docs/PLAN-multiplayer.md` §1's rule. The same
token is the resume token: a reconnect within the lobby's grace presents it
again. A player is therefore the same person to the lobby, the chat and the
match, and a leaderboard write comes from the server that saw the match,
never from the client that claims a score.

**Rejoin at the lobby.** A membership survives socket loss for
`lobby.reconnect_grace` seconds, mirrored on the engine's
`multiplayer/reconnect_grace`; the lobby channel gains `user_rejoined` the way
the signaling channel has it; a kick after the grace is the host's or the
server's. Matchmaking tickets stay cancelled on close, since a queued player
who vanished should not be pulled into a match.

**The match record.** At the end of a match the game server uploads the
`.blr` recording and the final digest as a lobby snapshot blob, so a lobby's
timeline reads lobby events, then the match that replays bit for bit, then
the results. A disputed result is `balaur replay --verify` on the server's
file. Quantum's server does the same job; this one is a file the engine
already writes.

**Typed bindings, generated, the whole surface.** Gamend publishes an
OpenAPI document and its realtime messages as protobuf, and generates its
JavaScript and Godot SDKs from them. The engine does the same: a
`scripts/gen_gamend.py` (or a build step, open question 3) reads the two and
emits `crates/balaur_gamend/src/generated.rs`, one script function per
operation, flat under `gamend` because script modules are flat:
`gamend::lobby_create(node, attrs)`, `gamend::lobby_join(node, id, options)`,
`gamend::matchmaking_join(node, params, options)`,
`gamend::friend_request(node, user_id)`, `gamend::kv_get(node, key)`,
`gamend::leaderboard_scores(node, id, options)`. Each returns the id its
`rest` call returns today and delivers the same way. Realtime events arrive
typed per channel: `lobby_updated`, `lobby_user_updated`, `party_*`,
`friend_*`, `notification`, `kv_changed`, `matchmaking_matched`,
`ready_check`. `rest` and `push` stay as the escape hatch for a server hook a
game added. Nothing is curated out: an endpoint the engine has no use for
still gets a function, because the next game has one.

```rune
pub async fn init(this) {
    gamend::configure("https://play.example");
    task::wait(gamend::login(#{ provider: gamend::PROVIDER_STEAM, ticket: ticket })).await;
    let ticket = task::wait(gamend::matchmaking_join(this.node, #{ mode: "duel" }, #{ min_players: 2 })).await;
}

pub fn on_gamend_event(this, e) {
    if e["kind"] == gamend::EVENT_MATCHMAKING_MATCHED {
        gamend::lobby_join(this.node, e["lobby_id"]);
    }
    if e["kind"] == gamend::EVENT_LOBBY_UPDATED && e["lobby"]["server"] != () {
        // The seam: a Gamend address and token into the engine's match.
        multiplayer::join(this.node, e["lobby"]["server"]["url"], #{ token: e["lobby"]["token"] });
    }
}
```

**Determinism.** Unchanged: every arrival is an `ExternalIo` completion at
`Stage::First`, recorded and replayed, so a recording of a matchmade game
replays with no server running.

## 2. The surface

Two tables, because two repositories. Each row says which side, and where it
stands.

### Server side, in the `gamend` repository

| Feature | Decision |
| --- | --- |
| A game server per lobby: spawn, register, heartbeat, teardown, admin page | Step S1 |
| Lobby-scoped tokens and their verification endpoint | Step S2 |
| Membership grace and `user_rejoined` on the lobby channel | Step S3 |
| The match record: recording and digest uploaded as a lobby snapshot blob; results written by the server; leaderboard and quest hooks fed from it | Step S4 |
| The WebRTC peer as a per-lobby relay with unordered, unreliable channels, more than one channel, a per-lobby budget | Step S5, the browser fallback; the engine half is `docs/PLAN-networking.md` step 14 |
| Skill matchmaking: ratings, widening bands, leaver refill | Gamend's roadmap; the engine binds whatever the API exposes |
| Room and player properties | Have: lobby attrs and member metadata; live through `updated` and `user_updated` |
| Master client | Have: the lobby host |
| Random join, join by filter | Have: `list_lobbies` with filters, matchmaking by exact params; a filter language beyond equality is Gamend's call |
| Lobby chat, DMs, presence, friend status | Have |
| Custom authentication, third-party sign-in | Have: OAuth for five providers, JWT; Steam and Play ticket verification are `docs/PLAN-steam.md` step 2 and `docs/PLAN-google.md` step 2 |
| Webhooks | Gamend's roadmap |
| A hosted service, regions, best-region selection | Not planned. Gamend is self-hosted by design; a game with players on two continents runs two deployments and picks by ping in script, and its game servers run near the players (open question 1) |
| Traffic and CCU dashboards | Have: the admin portal, Prometheus and Grafana; a game-server row per lobby is step S1's addition |
| TURN | Not shipped: `host_config.exs` names one STUN server. A relay-through-Gamend match needs none; step S5 needs a TURN for symmetric NATs and names `coturn` |

### Engine side, in this repository

| Feature | Decision |
| --- | --- |
| Generated typed calls over the OpenAPI document, one per operation: `me`, `user`, `session`, `provider`, `lobby`, `party`, `friend`, `group`, `chat`, `chat_mute`, `matchmaking`, `ready_check`, `notification`, `push_token`, `quest`, `leaderboard`, `tournament`, `economy`, `kv`, `storage`, `payment`, `client_log`, `hook`, `stats`, `time`, `health`, `signaling` | Step E1 |
| Typed realtime events per channel: user, lobby, lobbies, party, group, groups, signaling; `kv:subscribe` | Step E1 |
| Lobby to match glue: the address and token off `lobby_updated` into `multiplayer::join`, in the game's script; a host-run match that needs no server as the fallback | Step E2 |
| A Gamend dock in the editor: a header with the server target, Overview, User, Lobby, Data, Activity and Logs sub-tabs, and a tab per server feature | Built (E3), designed in §2b |
| The server target: `gamend/url`, `local_url`, `plugin` for the project and `gamend/target` for the person | Built (E3c) |
| Player prefs kept locally, `prefs.rn`, seen and edited from the dock and the editor's User data dock | Built (E3d) |
| A log cursor, a log file that survives a crash, and batched shipping to Gamend | Built (E4) |
| The wasm stub replaced by the Fetch and WebSocket client | `docs/PLAN-web-editor.md` step 4 |
| WebRTC data channels behind `Transport`, native and browser | `docs/PLAN-networking.md` step 14 |
| Admin endpoints | Step E1 generates them too, gated on the token's role; an editor plugin that manages a game's server from the palette is the reason to have them |

## 2b. The Gamend dock, the server target and logs

Designed on 2026-09-18, before building past the first tab. The dock is an
editor plugin the addon carries (`addons/gamend/editor/gamend.rn`), so only a
project with the addon shows it, and it is written the way anyone's plugin
is: `register()` returning docks, windows and commands.

**One header, six sub-tabs, then a tab per feature.** The header is one
row: the server target as two chips, `Open` and `Admin` links to the
configured URL, and who is signed in over what socket. The tabs are a
sidebar beside the body, since a bottom dock has width and little height.

| Tab | Shows | Works signed out |
| --- | --- | --- |
| Overview | Server URL and health, server version against the addon's `GAMEND_VERSION`, the hook plugin, the user, each socket and its topics, whether a session is saved | Yes |
| User | The profile from `GET /api/v1/me` as a structure; the access token masked, with reveal, copy and its expiry counting down; the refresh token masked; sign in with a device id, sign out, forget the saved session | The saved session, marked as saved rather than live |
| Lobby | The lobby behind a joined `lobby:<id>` topic: its fields, `data`, host, and members with online state; the party the same way | No |
| Data | Local: the addon's save slots, `gamend` (the kept session, read only) and `gamend_prefs` (player prefs, editable per key or as JSON), with the folder revealed. Server: each subscribed key-value key with its last value, and get and set for one key | Local yes, server no |
| Activity | What is built: every call with its reply and round trip, and every realtime message. Adds filters (calls, messages, errors), clear, and a row that opens to its arguments and reply | Yes |
| Logs | Whether shipping is on and at what floor, the run id, entries pending, the last flush with accepted and dropped counts, the log file, flush now | Yes |

After the six, one tab per server feature: Leaderboards, Quests, Economy,
Friends, Chat, Parties, Groups, Tournaments, Matchmaking, Notifications,
Payments, Hooks and Users. Each is a list of `GET` views of that feature
(a leaderboard's records, the wallet, the ledger), with an id field where a
path takes one, drawn as structures and fetched again on Refresh. They read;
changing server data stays the game's and the admin site's.

A structure is drawn as a tree: a map or a list opens and closes, a leaf
shows its value in mono with a copy mark. One drawing function serves the
profile, the lobby, a hook's reply and a key-value value.

**What any plugin can reuse.** A plugin's script is its own Rune unit, so it
cannot import the editor's modules; it gets `S`, the theme `k` and the `ui`
verbs. The parts this dock needs are the parts anyone's dock needs, so they
are written once in the editor and handed to every plugin as closures at
`S.kit`: `tabs(S, k, id, names)` answering the active one and wrapping to the
width, `pages(S, k, id, groups, draw)` (a sidebar of pages beside the open
one, a dropdown when narrow), `tree(S, k, id, value, opts)` (editable with `edit`),
`field(S, k, label, value, opts)` with copy and mask, `section(k, title)`,
`empty(k, text)`, `files(S, k, id, opts)` (a folder's TOML and JSON files as
editable structures, with reveal), `game_data(S)` and `save_prefs(S)`. The
editor's own User data dock (`editor/plugins/userdata.rn`) is `files` over
the game's user data directory, which is the generic half of the Data
tab. A plugin adds a dock tab, a floating
window (`windows`), a palette command or an inspector section the same way
the Gamend one does; the manual's "Extending the editor" is the reference.

**What the engine adds for it.** The dock reads state; it keeps none.

| Call | Answers |
| --- | --- |
| `gamend::connection()`, `activity()` | Built. `activity` rows gain `args` and `reply`, kept for the newest fifty and cut at a few kilobytes each |
| `gamend::session()` | `{ user_id, username, display_name, access_token, refresh_token, expires_at }`, or nil. What the token row and the log shipper read. A game already holds its own token in every other SDK; the dock masks it by default |
| `gamend::restore(session)` | Sign in from a kept session without asking the server for a new one, which is what `auth.rn`'s `saved()` needs to be useful |
| `gamend::clear_activity()` | The Activity tab's clear |

Lobby, profile and key-value values need no engine call: the dock asks the
server through `gamend::rest` with the session the game made, and reads the
addon's caches (`gamend:rows`, `gamend:users`) off the nodes that hold them.

**Player prefs.** `addons/gamend/prefs.rn`: `get(key, fallback)`,
`set(key, value)`, `all()`, `remove(key)`, over the `gamend_prefs` save
slot. What Godot's addon did with a `ConfigFile` under `user://`. Local
only and never synced; a value that should follow the player to another
device goes in the server's key-value store. The Data tab edits the same
slot, so a developer sees and changes what the game reads.

**Where saves are while the editor plays.** `save::` files live under the
user data directory named after the project. A game played inside the
editor runs in the editor's engine, whose directory is the editor's own, so
the editor points `save::` at the game's with `project::use_data` when it
loads a project. A game played there and the same game run alone keep one
set of saves, and the dock reads what either wrote.

**The server target.** A `[gamend]` settings group the engine plugin
declares, so it is on the settings screen like any other:

| Setting | Scope | Default | Is |
| --- | --- | --- | --- |
| `gamend/url` | project | `https://gamend.org` | The server a shipped game talks to: `https://polyglotpirates.com` for that game |
| `gamend/local_url` | project | `http://localhost:4000` | A developer's own `mix dev.start` |
| `gamend/plugin` | project | empty | The server plugin hooks are called in |
| `gamend/target` | editor | `production` | `production` or `local`: the person's choice, kept in the editor's own file so it can never ship |

`client.rn`'s `configure()` with no argument reads them. `gamend/target`
is the header's two chips. An exported game has no editor file, so it can
only ever read `gamend/url`. `configure(url)` with an argument still wins,
for a game that picks its server at run time.

**Logs: a cursor, a file, and shipping.** Today `log::recent(n)` is the
only way a script reads the log, and it cannot tell new lines from ones it
saw. Three pieces, each useful alone:

| Piece | Where | What |
| --- | --- | --- |
| A cursor | Engine, `log::since(cursor)` | Answers `{ entries, cursor, missed }`: each line once, in order, and how many the ring dropped between two reads. Entries gain `seq`. Polled once a frame, like every other source; an observer, never recorded |
| A file | Engine, `[log] file`, `[log] keep` | The same stream teed to `logs/run.log` under the user data directory, the last runs kept as `run.1.log` and on, with a panic written before the process dies. What a script cannot do, because the run that matters is the one that crashed |
| Shipping | Addon, `logs.rn` and `log_sink.rn` | `setup(url)`, `pump(sink, node, dt)` each frame, `flush(sink, node)`, `settle(sink, reply)`; `log_sink.rn` is the node script that does it and reports to the dock. Reads `log::since`, keeps what the server's policy asks for (`GET /api/v1/client_logs/policy`: on or off, a level floor, per-category floors), folds an immediate repeat into one line with a count, and posts batches to `POST /api/v1/client_logs` |

Shipping batches as `GamendLogs.gd` does: a flush at fifty entries, every
ten seconds, and at once on an error; at most five hundred held, oldest
dropped, with the gap in `seq` telling the server what was lost. A refused
batch (4xx) is dropped, a failed one (5xx, no route) goes back in front.
Pending entries are mirrored to a spool file and sent on the next launch,
and that launch also ships the tail of the previous run's log file, which
is where a crash left its trace. It posts through `http::` with its own
headers (`x-gamend-session`, the bearer token from `gamend::session()`),
not through the SDK's client: a sink that needs a healthy client to report
an unhealthy one goes quiet when it matters.

## 3. Steps

Server steps run in the `gamend` repository later; engine steps here. E1 has
no server dependency and can start now.

- **S1. A game server per lobby.** The table, the spawn through a `Port`,
  the registration, the heartbeat, the teardown, the admin page. Ends with:
  a lobby started in the admin portal shows a running `balaur` process and
  an address.
- **S2. Tokens.** Lobby-scoped JWTs with a slot, the verify endpoint. Ends
  with: a game server refuses a token for another lobby.
- **S3. Rejoin.** Membership grace, `user_rejoined`. Ends with: a socket
  dropped and reopened inside the grace is the same member.
- **S4. The match record.** Upload, results, hooks. Ends with: a finished
  match replays from the admin portal's download with every digest matching.
- **S5. The WebRTC relay.** Ends with: two browsers in one lobby exchange
  datagrams through Gamend with no engine web transport.
- **E1. The SDK.** Its own plan, `docs/PLAN-gamend-bindings.md`: a
  generator in Gamend's `clients/`, beside the Godot and JavaScript ones,
  emitting a Rune addon that is copied into the engine's library and into
  a game. Ends with: a game requires `addons/gamend` and calls every
  operation by the name the Godot addon gives it.
- **E2. Lobby to match.** With S1 and S2: matchmaking, lobby, game server,
  match, in one script. Ends with: two engines matched by Gamend play on a
  server Gamend launched, which is `docs/PLAN-networking.md` step 8 done.
- **E3. The Gamend dock.** §2b is the design. In order:
  - **E3a, built.** The plugin seam for addons (`addons/<name>/editor/`),
    `gamend::connection()` and `activity()`, and a first dock drawing both.
  - **E3b, built.** `S.kit` in the editor (`tabs`, `pages`, `tree`,
    `field`, `section`, `empty`, `files`), the header, the sub-tabs, and
    Overview and Activity on them, with `args` and `reply` kept per row and
    `clear_activity`. `editor/plugins/counter.rn` is on the kit as the
    worked example, and `editor/plugins/userdata.rn` is the generic User
    data dock.
  - **E3c, built.** The `[gamend]` settings, `configure()` reading them,
    the two chips and the two links.
  - **E3d, built.** `gamend::session()` and `restore()`, `prefs.rn`, the
    User tab with its masked tokens, the Data tab's local slots with
    editing, one server key-value key at a time, and the rows a playing
    game keeps (`gamend:rows` on its nodes). The editor's device id is the
    game's, so a device sign-in from the dock is the game's account.
  - **E3e, built.** The lobby and party behind the joined topics, fetched
    again when a newer message lands on their topic (`activity` rows carry
    a `seq`). Checked on a local Gamend: a rename shows in the dock.
  - **E3f, built.** A tab per server feature, each a list of `GET` views.
- **E4, built.** `log::since` and the log file in the engine (lines logged
  before it opened go in first), `logs.rn` and `log_sink.rn` in the addon,
  and the Logs tab. One run id (`gamend::run_id()`) rides every REST call
  as `x-gamend-session` and the socket as `client_session`, so server lines
  join the run's. Each line carries the joined lobby, so the server files
  the run under it. A game played in the editor sends neither the editor's
  earlier lines nor its log file. Checked against `gamend.org` (policy off)
  and a local Gamend with collection on: the run, its device and user, and
  its lobby land in `client_sessions`.

## 4. What CI can prove

Engine: `crates/balaur_gamend/tests` talks to a real server, never a
stand-in: `GAMEND_URL`, or gamend.org, in the e2e suite. A test that signs
in registers its own account by device and deletes it before it ends
(`DELETE /api/v1/me`, with `current_password` once it has one). E2's test
spawns a real `balaur run --server`. Gamend: its own suite
(`lobbies_test.exs`, `matchmaking_test.exs`, `signaling_test.exs`) plus one
that spawns a stub `balaur` script printing a port and a hash. What neither
can: a real NAT, a real region, a phone.

## 5. Open questions

1. **Where a game server runs.** On the Gamend machine, or in a separate
   pool app that Gamend addresses over the private network. The first is
   simpler and is enough for one region; the second is what regions need.
2. **A process per lobby or a process hosting many matches.** Per lobby is
   isolation and simplicity; per process is memory and port economy. Start
   per lobby and measure.
3. **When the bindings are generated.** Answered in
   `docs/PLAN-gamend-bindings.md` §1: in Gamend's own CI, as a Rune addon
   published beside the Godot one, and copied into this repository's
   `editor/library/addons/gamend` by `scripts/sync_gamend.sh --check`.
4. **Whether the host-run game needs Gamend at all.** A host that listens
   still needs an address a friend can reach, which is NAT; Steam's relay
   (`docs/PLAN-steam.md` step 8) or the WebRTC relay answers it without a
   game server. Both stay.
5. **Whose user data a played game writes.** Answered: the game's. The
   editor points `save::` at the game's directory (`project::use_data`), so a
   session kept while playing in the editor is found by `balaur run`.
6. **The default server.** Answered: `https://gamend.org`, a hosted Gamend
   a new game tests against until it names its own.
