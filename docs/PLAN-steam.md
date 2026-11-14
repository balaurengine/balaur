# Steam

Steamworks on Windows, macOS and Linux, as a backend behind the `platform`
module the Apple backend already fills, and a `steam` module beside it for
what only Steam has. A game can ship on Steam after step 4.

## Steps

1. **The client, initialised and pumped.** `crates/balaur_steam` over the
   `steamworks` crate, a `steam` feature on the facade, and
   `balaur_steam::init()` called from `main` before the window, since
   `SteamAPI_RestartAppIfNecessary` may exit the process.
   `SteamAPI_RunCallbacks` runs at `Stage::First` and feeds the `ExternalIo`
   channel, so every arrival is recorded and replays with no client. At
   export, `[steam] app_id` copies the redistributable beside the executable
   (on macOS into `Contents/Frameworks`, signed before the bundle), and only
   a `--dev` export writes `steam_appid.txt`. Done: a game launched from the
   Steam client reports its app id, language and whether it runs on a Deck;
   with no client, every `platform` call answers `unsupported`.
2. **Sign-in.** `platform::sign_in` and `platform::player` from
   `ISteamUser`, and a `GetAuthTicketForWebApi` ticket that a Gamend hook
   verifies with `ISteamUserAuth/AuthenticateUserTicket`. Done: a Gamend
   session whose player id came from Steam.
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

CI proves what needs no client. The feature compiles on and off, and an
export with `[steam]` carries the redistributable and no `steam_appid.txt`.
The macOS `.app` passes `codesign --verify --deep`, and canned Steam events
replay to a bit-identical digest.

Not planned: the game-server interfaces (Gamend is the server), music
control, the HTML surface, the DRM wrapper (it rewrites the executable a pack
is appended to), VAC and Easy Anti-Cheat.

## Open questions

1. **Does `steamworks-sys` build without the SDK present?** It decides
   whether CI can compile the feature. Check before adding the dependency.
2. **One platform backend at a time?** A build for two stores at once needs
   `platform` to name which backend answers. Nothing has asked.
3. **Where does `init` live for a game with its own `main`?** A documented
   first line, unless `balaur::run` grows a hook that runs before the window.
