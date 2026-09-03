> **Status:** steps 1-4 and 6 built on 2026-09-03; steps 5, 7 and 8 are not
> started. What landed: the `[apple]` table, and the `Info.plist` and
> `.entitlements` an export writes from it
> (`crates/balaur_export/src/apple.rs`); `crates/balaur_platform`, which is
> the portable `platform.*` module and the backend seam the Google and Steam
> plans lean on; and `crates/balaur_apple`, which puts Game Center and the
> iCloud key-value store behind those verbs and adds `apple.identity` and
> `apple.sign_in`. What did not: Game Center's own UI (step 5), StoreKit
> (step 7), and arrivals — notifications and opened URLs (step 8).
>
> **What is proved, and what is only compiled.** The export half is tested:
> the plist and entitlements a table produces, and every refusal — a
> capability with no bundle id, a misspelled one, `icloud-kv` with no team,
> Game Center below the version its identity signature needs. The seam is
> tested through a canned backend: the `unsupported` answer with no store, a
> sign-in landing a player, a recorded session replaying with no backend
> behind it, and the deferral rule. The framework code compiles for macOS and
> for iOS and has never run against Apple's servers — that needs an Apple ID,
> a provisioning profile and hardware, which §4 said in advance and which is
> still true.
>
> **Where the implementation decided differently:**
>
> 1. **`platform.*` shipped here, not in the Steam plan.** §1 said whichever
>    store landed first would build it, and this one did. `crates/balaur_platform`
>    owns the module, the `PlatformBackend` trait, the `unsupported` answer
>    with no store, and the recording; `balaur_apple` is the first backend.
> 2. **The confirmed-tick rule became a horizon, and it lives in core.**
>    §1 says a write is dispatched from a tick the rollback session has
>    confirmed, and nothing in the engine exposed that. `rollback::Clock` now
>    carries the tick being run and the tick before which nothing can be taken
>    back — the snapshot ring's earliest, since a tick the ring has dropped
>    can never be re-run — and `Session::run_tick` writes it. A write waits in
>    `PlatformState::pending` until its tick is behind that horizon; a read
>    goes at once. A game with no session leaves the horizon at `u64::MAX`,
>    where every tick is final immediately, so the rule costs nothing.
> 3. **The exporter writes the plist whole rather than patching it.** Two
>    string replacements over a template's plist cannot add a key, and the
>    plan needs `[apple.plist]` to add several. The template's own plist is
>    now only read for `CFBundleExecutable`, because the binary inside the
>    bundle keeps the template's file name.
> 4. **Capabilities are a closed enum.** A misspelling fails at export with
>    serde naming the three that exist, which is the message the plan wanted
>    and no code of ours to write.
> 5. **`icloud-kv` requires `team`.** Xcode expands `$(TeamIdentifierPrefix)`;
>    nothing expands it here, so the key-value store identifier has to carry
>    the real team.
> 6. **`in-app-purchase` is not a capability yet.** It maps to no entitlement
>    on iOS, and accepting a name that writes nothing would read as support
>    for step 7, which is not built.
> 7. **Two calls go through raw messages rather than the typed bindings.**
>    `setAuthenticateHandler:` takes the platform's own view-controller type,
>    so the typed binding needs AppKit on macOS and UIKit on iOS for a
>    controller this code ignores; the block's ABI is the same either way.
>    The Sign in with Apple presentation anchor is informal for the same
>    reason, and comes from the shared application's key window.
> 8. **Open question 1 is not closed.** Sign in with Apple presents over
>    whatever window the application has, and refuses with a message when
>    there is none. Game Center's sheet is a different problem — the handler
>    is given a view controller to present, and nothing presents one — so a
>    sign-in that needs the sheet reports that rather than showing it. That
>    is step 5, and it is still open.
> 9. **The Apple templates carry the feature.** `scripts/package.sh` and
>    `scripts/package_template.sh` build the macOS and iOS templates with
>    `--features "window,apple"`, because a template is prebuilt and a game
>    exported onto one cannot link a framework afterwards.
> 10. **Verifying a player stays the game's server, not the engine's.**
>    Steps 3 and 4 name a Gamend hook; Gamend's hooks are a server's, and
>    this repo has the client. What landed is the proof a hook needs —
>    `apple.identity`'s url, signature, salt and timestamp, and
>    `apple.sign_in`'s identity token and authorization code, base64 because
>    their destination is JSON.

# Plan: Apple platform services

Sign in with Apple, Game Center, iCloud saves and StoreKit on iOS and macOS.
`docs/PLAN-google.md` is the same document for Android, `docs/PLAN-steam.md`
for the desktop stores; §1 is deliberately the same design in all three.

## 0. Where the tree is today

Built, and not built for this:

| Have | Where |
| --- | --- |
| An iOS `.app` and a signed macOS `.app` out of `balaur export` | `crates/balaur_export/src/bundle.rs`, `scripts/package_template.sh` |
| Work off the frame landing on a tick boundary, recorded and replayable | `ExternalIo`, `Stage::First`, `balaur_core::handler` |
| A result reaching a script as a method call, or as an awaited token | `Handler`, `handler_of`, `task::wait` |
| A server with login, REST and hooks to verify a token against | `balaur_gamend` |
| A save file, atomic, versioned, in the user data directory | `balaur_core::save`, `user_data_dir_of` |
| A C shim compiled into a platform crate by `build.rs` | `crates/balaur_http/{build.rs,shim/emscripten_http.c}` |
| Objective-C bindings, already in the graph | `objc2`, `objc2-foundation`, `objc2-ui-kit` (through winit) |
| Once-at-start state that a recording carries rather than re-reads | `App::add_replay_setup` |

Missing:

- **A bundle identifier the developer owns.** `name_the_app` string-replaces
  `CFBundleIdentifier` with `org.balaur.<name>`. Sign in with Apple, Game
  Center, iCloud and StoreKit all resolve against the identifier registered
  in App Store Connect, so every one of them fails on an exported game today,
  before any code is written.
- **Entitlements.** The export writes none, and there is no file to put them
  in. Each capability below needs its own key, and a provisioning profile
  that grants it.
- **An `Info.plist` a project can add to.** The template's is a fixed string
  in `package_template.sh` with two keys rewritten at export.
- **The three frameworks this needs.** `objc2`, `objc2-foundation` and
  `objc2-ui-kit` are already in the graph through winit, so the binding style
  is settled; `objc2-game-kit`, `objc2-store-kit` and
  `objc2-authentication-services` are not there. Neither is any `.m` in the
  tree, nor `swiftc` in any build script.
- **A deployment target new enough.** `ios_min=13.0`. StoreKit 2 is 15.0,
  and GameKit's identity signature is 13.5.
- **A view controller to present from.** Game Center's own UI is UIKit, and
  kiss3d owns the window.

## 1. Design

**The mechanism already exists, and this adds none.** Every Apple API here is
asynchronous and calls back on a queue the engine does not own. That is the
shape `balaur_http` already solved: the call returns an id immediately, the
completion crosses a channel, and `ExternalIo` delivers it at `Stage::First`
of a later tick, in arrival order, recorded in the tick's snapshot. Handlers
run from the script, never from a callback thread, and a replay re-feeds the
recorded arrival without touching the device. `crates/balaur_apple` is
`balaur_http` with GameKit where ureq is.

**Two modules, not one.** `platform.*` carries the verbs every store shares —
sign in, the current player, unlock, progress, submit a score, read scores,
cloud read and write, presence — and resolves to whichever platform plugin is
loaded. `apple.*` carries what only Apple has: Game Center's access point and
match-making UI, `apple.request_review`, App Clip and universal-link
arrivals. Neither is a compromise: a script that wants portable behaviour
gets it, and one that wants GameKit does not have to pretend GameKit is
generic. With no platform plugin loaded — the editor, a desktop dev run, a
replay — every `platform` call resolves to a `kind = "unsupported"` event, so
the script still runs. `crates/balaur_platform` owns `platform`, built with
this plan; the Apple backend sits behind it.

**Objective-C through `objc2`, Swift through a shim.** The `objc2` framework
crates (`objc2-game-kit`, `objc2-authentication-services`,
`objc2-ui-kit`, `objc2-foundation`) are autogenerated bindings that also
carry the `#[link(kind = "framework")]` attributes, so linking is a
dependency rather than a build script. What they cannot reach is StoreKit 2,
which is Swift-only with no Objective-C interface. That half gets a Swift
file exposing a C ABI, compiled into the crate the way
`shim/emscripten_http.c` already is — `cc` for the `.m`, `swiftc` for the
`.swift`, both driven from `build.rs` and both only when the target is
`ios` or `macos`.

**Capabilities are declared in `project.toml` and written at export.** A new
`[apple]` table names the bundle identifier, the team, the deployment target
and which capabilities the game uses; `balaur export` writes the matching
`Info.plist` keys and a `.entitlements` file beside the bundle. Signing stays
the developer's — as `docs/PLAN-mobile-export.md` already decided — but an
unsigned bundle that carries the right entitlements is signable in one
command, and one that does not is not signable at all.

```toml
[apple]
bundle_id = "com.studio.game"
team = "AB12CD34EF"
min_os = "15.0"
capabilities = ["applesignin", "game-center", "icloud-kv", "in-app-purchase"]
```

**The template decides what an exported game can call.** A template is a
prebuilt binary; the exporter copies it. So the iOS and macOS templates link
GameKit, AuthenticationServices and StoreKit unconditionally, and the
capabilities table decides only what the *bundle* declares. A game that never
signs in pays a few hundred kilobytes and no entitlement.

**Determinism.** A platform result is external input: it enters through
`ExternalIo`, is recorded per tick, and a replay re-feeds it. Two rules
follow, and both are worth stating because they are easy to get wrong:

- A call that changes the outside world — unlock, submit, purchase — must go
  through `ExternalIo::start`, which already refuses to spawn while a
  recording plays or a tick is being re-simulated (`replay::suppressed`).
- That suppression is right for a socket and subtly wrong for an unlock: the
  *predicted* run is the one that fired it, and the correction cannot take it
  back. So `platform.unlock` and `platform.submit_score` are dispatched from
  a tick the rollback session has confirmed, never from a predicted one. On a
  build with no rollback session, every tick is confirmed and the rule costs
  nothing.

**Where the identity ends up.** Sign in with Apple and Game Center both
produce a token the device cannot be trusted to have checked. The verification
belongs on the server, which is `balaur_gamend`: a Gamend hook takes the
identity token (Sign in with Apple) or the four-part identity signature
(Game Center) and returns a Gamend session. The engine never decides who a
player is; it carries the proof.

## 2. The surface

Every Apple service a game reaches for, and where each stands here.

| Service | Decision |
| --- | --- |
| Sign in with Apple (`ASAuthorizationAppleIDProvider`) | Step 3. `objc2-authentication-services`; entitlement `com.apple.developer.applesignin`; identity token verified by a Gamend hook against Apple's public keys |
| Game Center sign-in (`GKLocalPlayer.authenticateHandler`) | Step 4. The player id, alias and `fetchItems(forIdentityVerificationSignature:)` for the server |
| Game Center achievements (`GKAchievement`) | Step 4, behind `platform.unlock` / `platform.progress` |
| Game Center leaderboards (`GKLeaderboard`) | Step 4, behind `platform.submit_score` / `platform.scores` |
| Game Center's own UI: access point, dashboard (`GKAccessPoint`, `GKGameCenterViewController`) | Step 5. Needs a root view controller — see the open questions |
| Game Center challenges, achievement descriptions, player photos | Step 5, on the same UI seam |
| iCloud key-value store (`NSUbiquitousKeyValueStore`) | Step 6. The right size for a save: `save` already writes a small file, and the entitlement is one line |
| CloudKit records, iCloud Documents | Not planned. A record store and a document sync are a database, not a save file; `balaur_gamend` is where a game that needs one already has one |
| StoreKit 2: products, purchase, `currentEntitlements`, JWS transactions | Step 7, through a Swift shim |
| StoreKit original API (`SKPaymentQueue`) | Not planned. It is what `objc2-store-kit` reaches and it would avoid the shim, but its server story is the receipt Apple is retiring, and the shim is a hundred lines |
| StoreKit Testing in Xcode (`.storekit` configuration) | Step 7 — the only way a purchase is tested without money |
| App Store Server API / Server Notifications V2 | Step 7, server side, as a Gamend hook |
| Local notifications (`UNUserNotificationCenter`) | Step 8 |
| Remote push (APNs) | Step 8, device token only; the sending half is a server the game already needs |
| Game Controller framework (`GCController`) | Not planned here — it is an input backend, and `balaur_input` is where a second one belongs; gilrs does not cover iOS |
| Core Haptics | Not planned here; the roadmap's haptics row covers rumble across platforms and this is one backend of it |
| App Tracking Transparency | Not planned. Nothing in the engine collects an identifier to ask about |
| Universal links, custom URL schemes, App Clips | Step 8. One arrival shape: a URL the game is opened with, delivered as an event |
| Handoff, Live Activities, widgets, Siri intents | Not planned. All of them need an extension target, which is a second binary the exporter does not build |
| Family Sharing, Ask to Buy | Have, once StoreKit lands: both are App Store Connect settings, not API |
| App Store Connect API, TestFlight upload | `docs/PLAN-deploy.md`, as a destination; it needs step 1's bundle identity and provisioning profile before it can send anything |
| Notarization, and signing an exported game | `docs/PLAN-release.md`. Notarization already applies to the macOS `.app` |
| GameKit real-time and turn-based matches (`GKMatch`, `GKTurnBasedMatch`) | Not planned. `docs/PLAN-networking.md` §2 settles on WebTransport; a fourth transport for one platform is not worth its weight, and the turn-based service is a server this engine already has |
| Metal, MetalFX | Not planned here. wgpu is the renderer and speaks Metal already |
| ARKit, RealityKit, visionOS | The roadmap's XR row |
| tvOS as an export target | Not planned yet. The bundle work here is most of it, and the input model is the blocker: no touch, one remote |

## 3. Steps

Ordered so the piece nothing else can proceed without comes first, and each
step leaves something that works.

1. **A bundle a developer owns.** *Done.* The `[apple]` table, `Info.plist` keys
   written from it at export, `min_os` reaching the plist and the template
   build, and `--target macos` taking the same table. No entitlements yet.
   Ends with: an exported game installable under the studio's own identifier.
2. **Entitlements.** *Done.* A `.entitlements` file written from `capabilities`, the
   key set per capability, and `balaur export` refusing a capability the
   plist cannot satisfy. Ends with: `codesign --entitlements` succeeds and
   the app launches with the capability visible in its profile.
3. **Sign in with Apple.** *Done*, as `apple.sign_in`: `crates/balaur_apple`,
   the plugin, the `apple` feature on the facade,
   `objc2-authentication-services`, and the request → `ExternalIo` → handler
   path. The identity token and authorization code reach the script; the hook
   that checks them is the game's server, not the engine.
4. **Game Center: player, achievements, leaderboards.** *Done.* Authentication at
   launch, the identity signature to the same Gamend hook, and the four
   portable verbs implemented against `GKAchievement` and `GKLeaderboard`.
   Ends with: an unlock visible in the Game Center dashboard.
5. **Game Center's UI.** The access point and the dashboard, over whatever
   the open question below settles on for a root view controller.
6. **iCloud saves.** *Done.* `NSUbiquitousKeyValueStore` behind `platform.cloud_read`
   and `cloud_write`, with `save`'s file as the payload and its version as
   the conflict key. A newer file is refused the way `save` already refuses
   one, rather than silently overwriting.
7. **StoreKit 2.** The Swift shim, products, purchase, entitlement refresh,
   the `.storekit` test configuration in the repo, and the App Store Server
   Notifications hook in Gamend. Ends with: a purchase in the sandbox that
   survives a reinstall.
8. **Arrivals.** Local notifications, the APNs device token, and the URL the
   game was opened with, all as events on the same pump.

## 4. What CI can prove, and what it cannot

`scripts/export_mobile.sh` already draws this line for the bundle, and this
plan does not move it. What a runner can check:

- the crate compiles for `aarch64-apple-ios` and for macOS, and the shim
  compiles with it
- an exported bundle carries the identifier, the plist keys and the
  entitlements the project declared — a file comparison, not a device
- `codesign` accepts the entitlements file against an ad-hoc identity
- the plugin's replay test passes: a recorded session of canned platform
  events replays to a bit-identical digest with no device present

What it cannot: a real sign-in, a real unlock, a real purchase. All three
need an Apple ID, a provisioning profile and hardware. The honest bar is the
one mobile export already holds itself to — everything up to the point the
device takes over — plus a canned-event replay test, which is the part that
actually protects the simulation.

## 5. Open questions

1. **Which view controller does Game Center's UI present from?** kiss3d owns
   the window through winit. The candidates are reaching the key window's
   root controller through `objc2-ui-kit`, or the engine holding one it
   created. The second is more code and fewer surprises; neither is decided.
2. **Does `platform.sign_in` mean Game Center or Sign in with Apple?**
   They are different accounts with different lifetimes, and a game may want
   both. Likely answer: `platform.sign_in` is Game Center on Apple platforms,
   because it is the one that owns achievements, and Sign in with Apple stays
   `apple.sign_in`.
3. **Where do tokens live between runs?** The Keychain is the answer on
   Apple's side, and `save`'s directory is the answer everywhere else. A
   `platform.credential_*` pair over both is more surface; the alternative is
   that a token is not persisted and every launch signs in again, which
   Game Center does anyway.
4. **Does the macOS build get the same plugin?** GameKit and StoreKit are
   there, Sign in with Apple is there, and the `.app` is already signed. The
   cost is that a macOS game shipped outside the App Store must not link
   StoreKit expecting a receipt. Probably: same crate, capability table
   decides.
