# Plan: Gamend

Gamend is the backend beside the engine: accounts, lobbies, matchmaking,
chat, storage and server hooks. This plan gives a lobby a game server and a
token its members carry into the match, which is `docs/PLAN-multiplayer.md`.
S steps run in the `gamend` repository and E steps here.

## Steps

- **S1. A game server per lobby.** On lobby start Gamend spawns `balaur run
  --server --lobby <id> --token-secret <..>` through a supervised `Port`. The
  process prints its port and certificate hash, which go into a
  `game_servers` row and the lobby's `updated` event. A missed REST heartbeat
  or a closed lobby ends it, and an admin page lists them. Ends with: a lobby
  started in the admin portal shows a running `balaur` process and an
  address.
- **S2. Tokens.** A lobby-scoped JWT holding the user, the lobby and the
  slot, and a verify endpoint. The game server verifies it once and binds the
  slot to the link, so a leaderboard write comes from the server that saw the
  match. Ends with: a game server refuses a token for another lobby.
- **S3. Rejoin.** A membership survives socket loss for
  `lobby.reconnect_grace` seconds, mirrored by the engine's
  `multiplayer/reconnect_grace`, and the lobby channel sends
  `user_rejoined`. Matchmaking tickets stay cancelled on close. Ends with: a
  socket dropped and reopened inside the grace is the same member.
- **S4. The match record.** The game server uploads the `.blr` and the final
  digest as a lobby snapshot blob and writes the results. Ends with: a
  finished match replays from the admin portal's download under `balaur
  replay --verify`.
- **S5. The WebRTC relay.** Gamend's `ex_webrtc` peer as a per-lobby star
  with unordered, unreliable channels, and `coturn` for symmetric NATs. Ends
  with: two browsers in one lobby exchange datagrams through Gamend with no
  engine web transport.
- **S6. The addon's WebRTC layer.** The generator in Gamend's
  `clients/sdkgen` emits a client that joins `signaling:<lobby_id>`, relays
  offers, answers and ICE, and gives a game `send_data(user_id, label,
  bytes)`, `broadcast_data` and the `peer_*` and `channel_*` events. Waits on
  `docs/PLAN-networking.md` step 14. Ends with: two engines in one lobby
  exchange bytes through `send_data`.
- **E2. Lobby to match.** With S1 and S2, a game script carries the address
  and token off `lobby_updated` into `multiplayer::join`. Nothing in the
  addon opens a match, and nothing in the match knows a lobby exists. Ends
  with: two engines matched by Gamend play on a server Gamend launched, in a
  suite that spawns a real `balaur run --server`.

Not planned: QUIC or raw UDP from Elixir, and a hosted service with regions.
A game with players on two continents runs two deployments and picks one by
ping in script.

## Open questions

1. **Where a game server runs.** On the Gamend machine, or in a separate pool
   Gamend addresses over a private network. The second is what regions need.
2. **A process per lobby or one hosting many matches.** Start per lobby and
   measure.
3. **Whether a host-run game needs Gamend.** A listening host still needs an
   address a friend can reach. Steam's relay (`docs/PLAN-steam.md`) or the
   WebRTC relay answers it without a game server.
4. **Protobuf on the socket.** The server serves it on request, and the addon
   speaks JSON. The generator could emit a decoder from the same proto when
   the bytes matter.
5. **OAuth sign-ins.** `authentication::oauth_google_id_token` and
   `oauth_api_callback` answer a session that no `client` function adopts. A
   game-side one would be a `client::login_*` beside `login_device`.
