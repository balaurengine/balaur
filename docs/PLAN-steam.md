# Steam

`crates/balaur_steam` is a backend behind the `platform` module on Windows,
macOS and Linux: sign-in, the Web API ticket, achievements, leaderboards,
cloud calls and presence answer through `platform.*`, and Valve's whole flat
API is in the `steam_*` script modules with its callbacks on `steam_event`.
Its tests run against a fake `steam_api`; what is left is a real client, the
Auto-Cloud pattern, and the engine seams Steam fills beside `platform`.

## Steps

1. **A run against a real client.** A `--steam` export launched from the
   Steam client, and a notarized macOS bundle carrying the library. Done: the
   game reports its app id, language and whether it runs on a Deck; a Gamend
   session whose player id came from Steam; an achievement in the player's
   profile; and a lobby formed between two clients with no server of the
   developer's.
2. **Auto-Cloud.** A path pattern over what `save` writes, so a game gets
   Steam Cloud with no `cloud_read` or `cloud_write`. Done: a save made on one
   machine loads on another.
3. **Steam networking as a `Transport`.** Reliable and unreliable messages
   over `ISteamNetworkingSockets`, peer to peer through the Steam Datagram
   Relay, behind `balaur_net::transport::Transport`. Done: a rollback session
   between two Steam clients behind NAT.
4. **Workshop.** Subscribed `ISteamUGC` items as an asset source, downloads
   reported as events, and item creation and update. Done: a subscribed item
   loads as a directory of assets.
5. **Steam Input.** A backend under `crates/balaur_input` that reads pads in
   place of gilrs and `sensors.rs` on a Steam build and feeds the same recorded
   snapshot, with action sets generated from `[input.actions]` and glyphs
   for on-screen hints. Done: a pad remapped in Steam drives the game, and a
   recording of it replays to the same digest.
6. **Shipping.** SteamPipe through `balaur deploy` (`docs/PLAN-deploy.md`),
   and the Deck Verified checklist with suspend and resume run on a Deck.
   Done: a build reaches a Steam branch from `balaur deploy`.
7. **The rest of the API in scripts.** The 166 functions the `steam_*`
   modules leave out are in `crates/balaur_steam/sdk/unmapped.md` with why.
   Each comes in by teaching `scripts/gen_steam.py` its shape, never by a
   hand-written call: networking message handles (`SteamNetworkingMessage_t
   **`, with step 3), lists of pointers (matchmaking filters,
   `SteamParamStringArray_t`), the server browser's C++ response objects,
   and the structs with no recorded layout (with the next SDK). Done:
   `unmapped.md` lists only what has no script shape at all and the
   game-server API, which Gamend stands in for.

CI proves what needs no client. Every desktop build compiles it, a `--steam`
export carries the redistributable and no `steam_appid.txt`, each desktop
runner runs one with Valve's library and no client, and a recording of the
fake's answers replays the same with no library loaded. Still to prove there:
a `--steam` macOS `.app` passing `codesign --verify --deep`.

Not planned: the DRM wrapper (it rewrites the executable a pack is appended
to), VAC and Easy Anti-Cheat.

## Open questions

1. **One platform backend at a time?** A build for two stores at once needs
   `platform` to name which backend answers. Nothing has asked.
