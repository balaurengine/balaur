> **Status:** not started. Written down on 2026-09-03 so the order was decided
> before the first line: Steam comes first of the three platform plans,
> because it is the only one with no bundle identity, entitlement or dex work
> in front of it — the desktop export already ships a binary the developer
> signs, and the SDK is a dylib that sits beside it. Apple went first in the
> end and built the portable `platform` module with it, so what is left here
> is the Steam backend behind it. Achievements and cloud saves
> before anything social, because they are what a first Steam release needs;
> Steam Input and Steam networking last, because each replaces something the
> engine already has and must not quietly change what a recording reproduces.

# Plan: Steam

Steamworks on Windows, macOS and Linux: sign-in, achievements, leaderboards,
Cloud, rich presence, the overlay, Workshop, Steam Input and Steam networking.
`docs/PLAN-apple.md` and `docs/PLAN-google.md` are the same document for
mobile; §1 is deliberately the same design in all three.

## 0. Where the tree is today

Built, and not built for this:

| Have | Where |
| --- | --- |
| A desktop game as one file: the template binary with the pack appended | `balaur::standalone`, `crates/balaur_cli` |
| A signed macOS `.app` with the pack as a resource | `bundle.rs::export_macos_app` |
| Work off the frame landing on a tick boundary, recorded and replayable | `ExternalIo`, `Stage::First`, `balaur_core::handler` |
| A transport trait with reliable streams and unreliable datagrams | `balaur_core::transport::Transport` |
| Gamepads, and an action layer over them whose bindings a recording carries | `balaur_input`, gilrs, `App::add_replay_setup` |
| A save file, atomic, versioned, in the user data directory | `balaur_core::save` |
| A server with login, REST and hooks to verify a ticket against | `balaur_gamend` |
| A digest per tick, and a rollback session that knows which ticks are confirmed | `digest`, `rollback` |

Missing:

- **The SDK.** No `steamworks` in `Cargo.lock`, and the Steamworks SDK is not
  in this repo and will not be: it comes from the Steamworks site under an
  agreement each developer accepts. What ships beside a game is the
  redistributable — `steam_api64.dll`, `libsteam_api.dylib`,
  `libsteam_api.so` — and nothing in the export path puts a second file next
  to the executable.
- **A place for `SteamAPI_Init`.** It has to run before the window and after
  the process decides whether to relaunch itself through Steam
  (`SteamAPI_RestartAppIfNecessary`), which is earlier than any plugin's
  `build` runs today.
- **A callback pump.** Steam dispatches on the thread that calls
  `SteamAPI_RunCallbacks`, once per frame. Nothing calls it.
- **A signed `.app` that stays signed.** `export_macos_app` runs `codesign`
  over a bundle with one executable in it; a dylib added to
  `Contents/Frameworks` has to be signed before the bundle is, or the result
  is a game Gatekeeper refuses.
- **A second input backend.** gilrs reads the pad directly, so a game shipped
  with Steam Input gets Steam's remapping and the engine's raw reading of the
  same device.
- **An upload.** `docs/PLAN-release.md` builds and signs; nothing runs
  SteamPipe.

## 1. Design

**The mechanism already exists, and this adds none — with one twist.** Every
Steamworks result arrives as a callback or a call-result, and Steam delivers
both from `SteamAPI_RunCallbacks`. So unlike HTTP there is no worker thread:
the pump *is* the delivery. What does not change is where the result enters
the simulation. The pump runs at `Stage::First`, pushes each callback onto the
`ExternalIo` channel, and drains it in the same tick — so the arrival is
recorded in the tick's snapshot, replays without a Steam client, and reaches a
script as a handler call or an awaited token, exactly like an HTTP response.

**Two modules.** `crates/balaur_platform` already carries the first, built
with `docs/PLAN-apple.md`; what this plan adds is the Steam backend behind it
and `steam.*` beside it.

- `platform.*` — the verbs every store shares, and the module the Apple and
  Google plans implement behind: `sign_in`, `player`, `unlock`, `progress`,
  `submit_score`, `scores`, `cloud_read`, `cloud_write`, `set_presence`. With
  no platform plugin loaded — the editor, a dev run, a replay — every call
  resolves to a `kind = "unsupported"` event rather than an error, so a script
  written against it runs everywhere.
- `steam.*` — what only Steam has: the overlay, Workshop, lobbies, the
  inventory, DLC, the Deck.

The portable module is not a lowest common denominator, because the native
module is always beside it. A game calls `platform.unlock("first_blood")`
once and `steam.workshop_subscribed()` where it means it.

```rune
pub async fn init(this) {
    let player = task::wait(platform::sign_in()).await;
    if player["kind"] == "unsupported" {
        log::info("no store here; playing offline");
    }
    platform::unlock(this.node, "first_blood");
}

pub fn on_platform(this, e) {
    if e["kind"] == "unlocked" { log::info(`achievement ${e["id"]}`); }
}
```

**Where the identity ends up.** A Steam id read on the client is a claim.
`GetAuthTicketForWebApi` produces a ticket only Valve can check, so the
verification belongs in `balaur_gamend`: a hook takes the ticket, calls
`ISteamUserAuth/AuthenticateUserTicket`, and returns a Gamend session. The
engine carries the proof and decides nothing.

**Determinism.** A platform result is external input: recorded per tick,
re-fed on replay. Two rules, both easy to get wrong:

- A call that changes the outside world — unlock, submit, write a cloud file
  — goes through `ExternalIo::start`, which already refuses to spawn while a
  recording plays or a tick is being re-simulated (`replay::suppressed`).
- That suppression is right for a socket and subtly wrong for an unlock: the
  *predicted* run is the one that fired it, and a rollback cannot take an
  achievement back. So `platform.unlock` and `platform.submit_score` are
  dispatched from a tick the rollback session has confirmed. With no rollback
  session, every tick is confirmed and the rule costs nothing.
- Anything that feeds *simulation* state — Steam Input, a lobby message —
  enters through the layer that is already recorded (`balaur_input`'s
  snapshot, the transport's arrivals), never by reading Steam mid-tick.

**Shipping the redistributable.** `[steam] app_id` in `project.toml` turns the
capability on. `balaur export` copies the redistributable beside the
executable, writes `steam_appid.txt` only for a `--dev` export — a shipped
build must not carry one, because it overrides the app id Steam launched with
— and on macOS puts the dylib in `Contents/Frameworks`, signs it, then signs
the bundle. A game exported without the table links nothing and loads nothing.

```toml
[steam]
app_id = 480
# Optional: what Steam Cloud syncs, if Auto-Cloud rather than the API is used.
cloud = ["saves/*.toml"]
```

**Init runs before the window.** `SteamAPI_RestartAppIfNecessary` may exit the
process, so it belongs at the top of `main`, not in a plugin's `build`. The
plugin takes over from an already-initialised client; the crate exposes
`balaur_steam::init()` for `balaur_cli`'s `main` and for a game's own.

## 2. The surface

Every Steamworks interface, and where each stands here.

| Interface | Decision |
| --- | --- |
| `ISteamUser`: Steam id, auth ticket for a web API, ban state | Step 2, behind `platform.sign_in`, verified by a Gamend hook |
| `ISteamUserStats`: achievements, stats, leaderboards, global stats | Step 3, behind `platform.unlock` / `progress` / `submit_score` / `scores` |
| `ISteamRemoteStorage`: Cloud files, quota | Step 4, behind `platform.cloud_read` / `cloud_write` |
| Steam Auto-Cloud (path patterns, no code) | Step 4, and the one to reach for first: `save` already writes to a directory, and a pattern in the partner site syncs it with no engine code at all. The API is for a game that needs conflict resolution of its own |
| `ISteamFriends`: persona, friends, rich presence, invite dialog, overlay activation | Step 5, behind `platform.set_presence` and `steam.overlay_*` |
| `ISteamUtils`: overlay notification position, Big Picture, Steam Deck detection, country, gamepad text input | Step 5. Deck detection is what a game branches its control hints on |
| `ISteamApps`: DLC installed, build id, branch, launch command line, purchase time | Step 6 |
| `ISteamMatchmaking`: lobbies, lobby chat, invites, favourites | Step 7. A lobby is the cheapest session layer this engine could have: `docs/PLAN-networking.md` §1's session layer asks for a member list and a way to send to each, and a lobby is both |
| `ISteamNetworkingSockets`, `ISteamNetworkingMessages`: P2P, listen sockets, the Steam Datagram Relay | Step 8, as a `Transport` implementation. It is the only transport that gives peer-to-peer through NAT with no server and no certificate, which is exactly what WebTransport cannot do; `docs/PLAN-networking.md` §2's table gains a row |
| `ISteamUGC`: Workshop items, subscriptions, downloads, creating and updating | Step 9, and it fits the engine's shape better than most: a pack is content-addressed and the asset source already knows `embedded+files`, so a subscribed item is a directory the loader is told about |
| `ISteamInput`: action sets, glyphs, gyro, haptics, Deck controls | Step 10, as a second backend under `balaur_input` — feeding the recorded input snapshot, never read directly. It replaces gilrs on a Steam build rather than sitting beside it, because two readings of one pad is a bug with a name |
| `ISteamInventory`: item definitions, drops, microtransactions, trading | Step 11 |
| `ISteamScreenshots` | Step 11. The offscreen render path already exists (ARCHITECTURE.md "Camera and screenshots"); this is the hook that tells Steam a screenshot happened |
| `ISteamTimeline`: markers and phases in Steam's game recording | Step 11. Cheap, and it pairs with the recording the engine already writes |
| `ISteamGameServer`, `ISteamGameServerStats`, `ISteamMatchmakingServers` | Not planned yet. A dedicated server on Steam's server list is a shape this engine has not been asked for; Gamend is where a server already lives |
| `ISteamMusic`, `ISteamMusicRemote` | Not planned. Controlling the player's music from a game is a feature approximately no game wants |
| `ISteamHTMLSurface` | Not planned. A browser in the engine is a dependency an order of magnitude larger than everything else here |
| `ISteamParentalSettings`, `ISteamVideo`, `ISteamAppList` | Not planned; nothing has asked |
| `ISteamController` | Not planned. Superseded by Steam Input |
| Remote Play Together, Family Sharing | Have, with no code: both are Steam features over a normal build |
| VAC, Easy Anti-Cheat | Not planned. VAC is a partner-site setting on a server the engine does not run, and EAC is a separate SDK with its own agreement |
| Steam DRM wrapper | Not planned. It rewrites the shipped executable, which is the file the exporter appends a pack to; the two do not compose, and the wrapper is optional |
| SteamPipe upload (`steamcmd`) | Step 12, as a destination in `docs/PLAN-deploy.md` — one uploader, one credential store, one progress stream for every store |
| Steam Deck Verified: input, text entry, suspend and resume, one-window fullscreen | Step 12 as a checklist, not code. Suspend and resume is the one that can find a real bug — a fixed 60 Hz tick and a process frozen for an hour |

## 3. Steps

Ordered so a game can ship on Steam after step 4, and each step leaves
something that works.

1. **The client, initialised and pumped.** `crates/balaur_steam`, the `steam`
   feature on the facade, `balaur_steam::init()` called from `main` before the
   window, `SteamAPI_RunCallbacks` at `Stage::First`, and shutdown on exit.
   `[steam] app_id`, the redistributable copied beside the executable at
   export, `steam_appid.txt` for a `--dev` export only, and the macOS bundle
   signed in the right order. `steam.app_id`, `steam.language`,
   `steam.on_deck` to prove the loop. Ends with: a game that launches from
   the Steam client and says who it is.
2. **Sign-in.** The Steam backend behind `platform.*`, `sign_in` and
   `player`, and the auth ticket a server checks. Ends with: a Gamend session
   whose player id came from Steam.
3. **Achievements and leaderboards.** `unlock`, `progress`, `submit_score`,
   `scores`, with the confirmed-tick rule from §1. Ends with: an achievement
   in the player's profile.
4. **Cloud saves.** Auto-Cloud first — a documented path pattern over what
   `save` already writes — then `ISteamRemoteStorage` behind
   `platform.cloud_read` / `cloud_write` for a game that wants to resolve its
   own conflicts. A newer file is refused, as `save` already refuses one.
5. **Presence, the overlay and friends.** `platform.set_presence`, the
   overlay's own events (opened, closed, so a game can pause), the invite
   dialog, and Deck and Big Picture detection.
6. **DLC and app state.** Installed DLC, build id, branch, launch parameters.
7. **Lobbies.** Create, join, leave, lobby data and chat, and an invite that
   launches the game. A session layer under `docs/PLAN-networking.md` that
   needs no server of the developer's.
8. **Steam networking as a `Transport`.** Reliable messages and unreliable
   ones over `ISteamNetworkingSockets`, peer-to-peer through the relay,
   behind the trait the rollback session already speaks to. The transport
   table in the networking plan gains its row here.
9. **Workshop.** Subscribed items as an asset source, downloads reported as
   events, and item creation and update for a game that ships an editor —
   which this engine does.
10. **Steam Input.** A second backend under `balaur_input`, replacing gilrs on
    a Steam build, feeding the same recorded snapshot; action sets generated
    from `[input.actions]`, glyphs for the on-screen hints.
11. **The long tail.** Inventory and microtransactions, screenshots, timeline
    markers.
12. **Shipping.** SteamPipe upload through `balaur deploy`, and the Deck
    Verified checklist — with suspend and resume actually tested, because a
    fixed-step engine resuming after an hour is the one on that list that can
    be a real defect.

## 4. What CI can prove, and what it cannot

A runner has no Steam client and no logged-in account, so:

- the crate compiles for every desktop target with the `steam` feature on,
  and the workspace still compiles with it off
- an export with `[steam]` puts the redistributable beside the executable,
  ships no `steam_appid.txt`, and produces a macOS `.app` that `codesign
  --verify --deep` accepts
- the plugin's replay test passes: a recorded session of canned Steam events
  replays to a bit-identical digest with no client running
- a game with no Steam client running starts, logs one line saying so, and
  every `platform` call resolves to `unsupported` — the case most engines get
  wrong and every developer hits on the first run

What it cannot: a real sign-in, unlock, leaderboard write, lobby or Workshop
item. Those need a Steam client, an account, and an app id — Valve's test app
480 is the usual answer for a developer's machine, and it is still not
something a runner has. The honest bar is the same as mobile export's:
everything up to the point Steam takes over, plus the canned-event replay,
which is the part that protects the simulation.

## 5. Open questions

1. **Does `steamworks` build without the SDK present?** It loads the
   redistributable dynamically at run time, which is settled; what decides
   whether CI can compile the feature at all is whether `steamworks-sys`
   carries the SDK headers it binds or expects a downloaded copy. Check that
   before adding the dependency — it is the difference between a feature CI
   compiles and one only a developer's machine can.
2. **One platform plugin at a time, or several?** A game shipped on Steam and
   on the App Store is one build per store today, and `platform` resolving to
   exactly one backend is the simple rule. Two at once — a Steam build that
   also signs into Game Center on macOS — needs `platform` to name which, and
   nothing has asked yet.
3. **Does Steam Input replace gilrs or sit beside it?** Replacing is written
   above and is the safer answer, since two readings of one pad differ in
   ways a recording will not explain. The cost is that a Steam build's input
   path is not the path CI tests.
4. **Where does `init` live for a game that embeds the engine?** `balaur_cli`
   can call it before the window; a Rust game using `balaur` as a library has
   its own `main`, and the restart-through-Steam call has to be the first
   thing in it. That is a documented line, not an API — unless
   `balaur::run` grows a hook that runs before the window opens.
