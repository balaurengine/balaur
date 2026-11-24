# Steam

Steamworks on Windows, macOS and Linux, as a backend behind the `platform`
module the Apple backend already fills, and a `steam` module beside it for
what only Steam has. A game can ship on Steam after step 4.

## Steps

1. **The client, initialised and pumped.** `crates/balaur_steam` in every
   desktop build, Valve's `steam_api` opened at run time from beside the
   executable (`Contents/MacOS` in a `.app`). With no library there, or no
   client running the game, `SteamAPI_InitFlat` is never reached or refuses,
   the plugin logs at info, `platform.*` stays as it was and every `steam_*`
   call answers nil. `SteamAPI_RestartAppIfNecessary` is never called.
   Callbacks are Valve's manual dispatch, pumped at `Stage::First` through
   `ExternalIo`, so every arrival is recorded and replays with no client.
   `balaur export --steam` copies the target's library beside the game, from
   beside the runtime where a release stages it as
   `balaur-runtime-<target>.<library>`, and needs `[steam] app_id`; `--dev`,
   which needs `--steam`, writes the id to `steam_appid.txt`. In a `.app` the
   library is signed with the game before the bundle, and the bundle gets
   `com.apple.security.cs.disable-library-validation` and
   `allow-dyld-environment-variables` for the overlay. An export without
   `--steam` ships no Valve file, whatever `[steam]` says. Done: a game
   launched from the Steam client reports its app id, language and whether
   it runs on a Deck; with no client, every `platform` call answers
   `unsupported`.
   *Built 2026-11-24.* Valve's whole flat API (SDK 1.64: 33 interfaces, 913
   functions, 225 structs and callbacks) comes from the vendored
   `sdk/steam_api.json` through `scripts/gen_steam.py` into
   `src/sdk/`, with every struct's layout asserted per platform against
   steamworks-sys's bindgen record (`sdk/layouts.json`). The script side is a
   `steam_*` module per interface, Valve's names in snake case, 817 functions
   mapped; what is not is `sdk/unmapped.md`, with why. Tests cover a build
   with no library and the export on fake runtimes for Linux, Windows and a
   macOS `.app`; `package.sh` exports a `--steam` game on each desktop CI
   runner and runs it with Valve's real library and no client. Not yet
   verified: a run against a real client, and a notarized bundle carrying the
   library.
2. **Sign-in.** `platform::sign_in` and `platform::player` from
   `ISteamUser`, and a `GetAuthTicketForWebApi` ticket that a Gamend hook
   verifies with `ISteamUserAuth/AuthenticateUserTicket`. Done: a Gamend
   session whose player id came from Steam.
   *Built 2026-11-23*: `platform.sign_in` answers the Steam id and persona
   name, and `platform.identity` a Web API ticket with no identity, as core
   checks it, delivered by `GetTicketForWebApiResponse_t`. Still to do: a run
   against a real client (only the no-client path is tested).
3. **Achievements and leaderboards.** `unlock`, `progress`, `submit_score`
   and `scores` over `ISteamUserStats`, each write held until its tick
   settles. Done: an achievement in the player's profile.
4. **Cloud saves.** Auto-Cloud first, a path pattern over what `save`
   writes, then `cloud_read` and `cloud_write` over `ISteamRemoteStorage` for
   a game that resolves its own conflicts. Done: a save made on one machine
   loads on another.
5. **Presence, the overlay and friends.** `platform::set_presence`, the
   overlay's opened and closed events so a game can pause, the invite dialog,
   and Deck and Big Picture detection from `ISteamUtils`.
6. **DLC and app state.** Installed DLC, build id, branch and launch
   parameters from `ISteamApps`.
7. **Lobbies.** Create, join, leave, lobby data and chat over
   `ISteamMatchmaking`, and an invite that launches the game. Done: a match
   formed with no server of the developer's.
8. **Steam networking as a `Transport`.** Reliable and unreliable messages
   over `ISteamNetworkingSockets`, peer to peer through the Steam Datagram
   Relay, behind `balaur_net::transport::Transport`. Done: a rollback session
   between two Steam clients behind NAT.
9. **Workshop.** Subscribed `ISteamUGC` items as an asset source, downloads
   reported as events, and item creation and update. Done: a subscribed item
   loads as a directory of assets.
10. **Steam Input.** A backend under `crates/balaur_input` that replaces both
    gilrs and `sensors.rs` on a Steam build and feeds the same recorded
    snapshot, with action sets generated from `[input.actions]` and glyphs
    for on-screen hints. Done: a pad remapped in Steam drives the game, and a
    recording of it replays to the same digest.
11. **The long tail.** Inventory and microtransactions, the screenshot hook,
    and timeline markers.
12. **Shipping.** SteamPipe through `balaur deploy` (`docs/PLAN-deploy.md`),
    and the Deck Verified checklist with suspend and resume run on a Deck.

CI proves what needs no client. Every desktop build compiles it, and a
`--steam` export carries the redistributable and no `steam_appid.txt`.
The macOS `.app` passes `codesign --verify --deep`, and canned Steam events
replay to a bit-identical digest.

Not planned: the game-server interfaces (Gamend is the server), music
control, the HTML surface, the DRM wrapper (it rewrites the executable a pack
is appended to), VAC and Easy Anti-Cheat.

## Open questions

1. **One platform backend at a time?** A build for two stores at once needs
   `platform` to name which backend answers. Nothing has asked.
2. **`RestartAppIfNecessary` for a game that wants it.** It has to run
   before `SteamAPI_InitFlat`, which the plugin calls as it loads, so a game
   needing it calls it from its own `main` through `balaur_steam::sdk`.
