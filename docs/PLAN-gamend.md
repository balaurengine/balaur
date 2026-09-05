> **Status:** not started. Written on 2026-09-05 from the Photon parity
> investigation. Two halves: what the Gamend server gains, which is done in
> the `appsinacup/gamend` repository and planned here so the two sides agree;
> and what `crates/balaur_gamend` gains, which is done here. The server steps
> are the server half of `docs/PLAN-networking.md` step 8; the engine's
> session itself is `docs/PLAN-sessions.md`.

# Plan: Gamend

Gamend is the backend beside the engine: accounts, lobbies, parties,
friends, chat, matchmaking, quests, leaderboards, payments, storage, server
hooks. Photon Realtime is the part of Photon that does the same job, and
Gamend already does more of it. What Gamend does not do is carry a game: its
realtime is Phoenix channels over a TCP websocket, and a lobby has no address a
game session could connect to. This plan gives a lobby a game server, binds
the engine's session to the lobby's roster, and gives scripts the whole API
as typed calls rather than a path string.

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
| Tests against an in-process Phoenix server | `crates/balaur_gamend/tests` |
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

**A lobby is the roster; the game does not go through Phoenix.** The BEAM is
the right place for ten thousand idle sockets and the wrong place for sixty
datagrams a second per player through a JSON channel. Three ways to give a
lobby a game transport:

| Option | What it is | Decision |
| --- | --- | --- |
| A headless Balaur game server per lobby | Gamend launches `balaur run --server` when a lobby starts, records its address and certificate hash on the lobby, and clients connect to it directly over QUIC. The server is the same engine, so ordering, digests, late join and the match recording come from `docs/PLAN-sessions.md` steps 3 and 4 with no Elixir networking code | **The plan.** Costs a process per lobby beside Gamend, a public UDP port range, and a browser waits for `docs/PLAN-networking.md` step 13 |
| The WebRTC peer generalised into a relay | `ex_webrtc` 0.17's `DataChannel` takes `ordered: :unordered` and `max_retransmits: 0`, which is a datagram. A per-lobby star with the server as hub relays game payloads instead of hook calls; the `@max_data_channels 1` cap and the per-user rate limit move to a per-lobby budget | The browser fallback, and already on Gamend's roadmap. The engine side is `docs/PLAN-networking.md` step 14 on native (`webrtc` or `matchbox`) and the browser's own API on the web. Every packet crosses the BEAM |
| QUIC in Elixir (`quicer` over msquic) | Raw QUIC, without the HTTP/3 and WebTransport framing the engine's client speaks, so it would need a second transport on the engine side too | Not planned |
| Raw UDP from Elixir (`:gen_udp`) | No encryption, no congestion control, no browser | Not planned, for the reasons `docs/PLAN-networking.md` §2 gives |

**The game server's life.** A `game_servers` table: lobby id, host, port,
certificate hash, status, started and last-seen. A hook on lobby start (the
host's "start" or matchmaking's `after_matchmaking_matched`) spawns
`balaur run --server --lobby <id> --token-secret <..>` through a `Port`
under a supervisor, waits for the process to print its bound port and
certificate hash, and writes the row. The lobby's `updated` event carries the
address; a client that sees it calls `session::join`. The process heartbeats
over REST; a missed heartbeat or a closed lobby ends it. Where it runs is
open question 1; the first answer is the Gamend machine itself, because a
Fly machine already has a public address and a cluster of them is what
Gamend's `cluster.ex` federates.

**One identity, bound to the link.** The join token is a lobby-scoped JWT
Gamend issues to a member: user id, lobby id, slot. The game server presents
it to Gamend once (`POST /api/v1/session/verify`, or a hook call) and binds
the slot to the link, which is `docs/PLAN-sessions.md` §1's rule. The same
token is the resume token: a reconnect within the lobby's grace presents it
again. A player is therefore the same person to the lobby, the chat and the
match, and a leaderboard write comes from the server that saw the match,
never from the client that claims a score.

**Rejoin at the lobby.** A membership survives socket loss for
`lobby.reconnect_grace` seconds, mirrored on the engine's
`session/reconnect_grace`; the lobby channel gains `user_rejoined` the way
the signaling channel has it; a kick after the grace is the host's or the
server's. Matchmaking tickets stay cancelled on close, since a queued player
who vanished should not be pulled into a match.

**The match record.** At the end of a session the game server uploads the
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
        this.session = session::join(this.node, e["lobby"]["server"]["url"], #{ token: e["lobby"]["token"] });
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
| TURN | Not shipped: `host_config.exs` names one STUN server. A relay-through-Gamend session needs none; step S5 needs a TURN for symmetric NATs and names `coturn` |

### Engine side, in this repository

| Feature | Decision |
| --- | --- |
| Generated typed calls over the OpenAPI document, one per operation: `me`, `user`, `session`, `provider`, `lobby`, `party`, `friend`, `group`, `chat`, `chat_mute`, `matchmaking`, `ready_check`, `notification`, `push_token`, `quest`, `leaderboard`, `tournament`, `economy`, `kv`, `storage`, `payment`, `client_log`, `hook`, `stats`, `time`, `health`, `signaling` | Step E1 |
| Typed realtime events per channel: user, lobby, lobbies, party, group, groups, signaling; `kv:subscribe` | Step E1 |
| Lobby to session glue: the address and token off `lobby_updated` into `session::join`; a host-run game that needs no server as the fallback | Step E2 |
| The wasm stub replaced by the Fetch and WebSocket client | `docs/PLAN-web-editor.md` step 4 |
| WebRTC data channels behind `Transport`, native and browser | `docs/PLAN-networking.md` step 14 |
| Admin endpoints | Step E1 generates them too, gated on the token's role; an editor plugin that manages a game's server from the palette is the reason to have them |

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
- **E1. Typed bindings.** The generator, the generated module, the events,
  the docs regenerated. Ends with: `docs/generated/api.json` lists every
  Gamend operation and the tests call each against the in-process server.
- **E2. Lobby to session.** With S1 and S2: matchmaking, lobby, game server,
  session, in one script. Ends with: two engines matched by Gamend play on a
  server Gamend launched, which is `docs/PLAN-networking.md` step 8 done.

## 4. What CI can prove

Engine: the in-process Phoenix server in `crates/balaur_gamend/tests` gains
the lobby, matchmaking, token and game-server endpoints as stubs, and E2's
test spawns a real `balaur run --server`. Gamend: its own suite
(`lobbies_test.exs`, `matchmaking_test.exs`, `signaling_test.exs`) plus one
that spawns a stub `balaur` script printing a port and a hash. What neither
can: a real NAT, a real region, a phone.

## 5. Open questions

1. **Where a game server runs.** On the Gamend machine, or in a separate
   pool app that Gamend addresses over the private network. The first is
   simpler and is enough for one region; the second is what regions need.
2. **A process per lobby or a process hosting many sessions.** Per lobby is
   isolation and simplicity; per process is memory and port economy. Start
   per lobby and measure.
3. **When the bindings are generated.** A build step needs the OpenAPI
   document in this repository and re-runs on every change; a checked-in
   `generated.rs` with a script matches `docs/generated/`. The second is the
   default, with a CI check that the file is current.
4. **Whether the host-run game needs Gamend at all.** A host that listens
   still needs an address a friend can reach, which is NAT; Steam's relay
   (`docs/PLAN-steam.md` step 8) or the WebRTC relay answers it without a
   game server. Both stay.
