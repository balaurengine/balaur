> **Status:** not started. Written down on 2026-09-03 so the order was decided
> before the first line: an APK that can hold Java first, because every Google
> service on Android is a Java library and the template declares
> `android:hasCode="false"`; the application id and the app bundle with it,
> because Play resolves an OAuth client against the package name and the
> signing certificate, and accepts an AAB rather than an APK; Play Games
> Services next, because sign-in, achievements, leaderboards and saved games
> are one SDK and one dependency; Play Billing after, because money is the
> part that must not be first.

# Plan: Google platform services

Play Games Services, Sign in with Google, Play Billing, Play Integrity and
Play Asset Delivery on Android. `docs/PLAN-apple.md` is the same document for
iOS and macOS, `docs/PLAN-steam.md` for the desktop stores; §1 is
deliberately the same design in all three.

## 0. Where the tree is today

Built, and not built for this:

| Have | Where |
| --- | --- |
| An APK layout out of `balaur export --target android`, assembled and debug-signed | `crates/balaur_export/src/bundle.rs`, `scripts/assemble_apk.sh` |
| A NativeActivity entry point holding the `AndroidApp` handle | `crates/balaur_android` |
| The pack as an APK asset, read through the asset manager | `android_main`, `PACK_ASSET` |
| Work off the frame landing on a tick boundary, recorded and replayable | `ExternalIo`, `Stage::First`, `balaur_core::handler` |
| A server with login, REST and hooks to verify a token against | `balaur_gamend` |
| A save file, atomic, versioned, in the user data directory | `balaur_core::save` |

Missing, and each one blocks everything below it:

- **A dex.** `AndroidManifest.xml` declares `android:hasCode="false"` and the
  APK carries no `classes.dex`. Every API in this plan is Java or Kotlin.
- **A dependency resolver.** `assemble_apk.sh` is `aapt2 link`, `zip`,
  `zipalign` and `apksigner` over a hand-written manifest. Play services ship
  as AARs on Google's Maven repository, with transitive dependencies,
  manifest fragments to merge and resources to compile.
- **An application id the developer owns.** The template is
  `package="org.balaur.template"` and the exporter never rewrites it — an
  Android game exported today is literally the template's package. Play
  Games, Billing and Integrity all resolve against the package name plus the
  signing certificate.
- **An app bundle.** Play takes an AAB for a new app; the export produces an
  APK layout. `bundletool` is the missing step, and Play Asset Delivery is
  only reachable through an AAB.
- **16 KB page alignment.** Android 15 requires it of native libraries, and
  nothing in `.cargo/config.toml` sets `max-page-size` for
  `aarch64-linux-android`. This is not a Google-services problem — it is a
  today problem for the existing template, and it belongs in step 1.
- **Java of our own, and a way to find it.** `jni` and `ndk-context` are
  already in the graph through `android-activity` — at two major versions,
  0.21 and 0.22, so a new crate has to pick the one it shares objects with.
  What is missing is any class of ours to call, and anything caching the
  activity's class loader, without which `FindClass` from an engine thread
  sees only the platform's classes.
- **One ABI.** The template builds `arm64-v8a` alone. Play wants `x86_64`
  too for Chromebooks, emulators and Play Games on PC.

## 1. Design

**The mechanism already exists, and this adds none.** Every Google API here
is asynchronous and calls back on the Android main thread. That is the shape
`balaur_http` already solved: the call returns an id immediately, the
completion crosses a channel, and `ExternalIo` delivers it at `Stage::First`
of a later tick, in arrival order, recorded in the tick's snapshot. Handlers
run from the script, never from a Java thread, and a replay re-feeds the
recorded arrival without touching Play services. `crates/balaur_google` is
`balaur_http` with a JNI call where ureq is.

**Two modules, not one.** `platform.*` carries the verbs every store shares —
sign in, the current player, unlock, progress, submit a score, read scores,
cloud read and write, presence — and resolves to whichever platform plugin is
loaded. `google.*` carries what only Play has: `google.integrity_verdict`,
`google.review_prompt`, `google.update_check`, `google.asset_pack_*`. With no
platform plugin loaded — the editor, a desktop dev run, a replay — every
`platform` call resolves to a `kind = "unsupported"` event, so the script
still runs. `crates/balaur_platform` owns `platform`, built with
`docs/PLAN-apple.md`; this plan implements the Play backend behind it.

**Java on one side, JNI in the middle, a channel on the other.** The Rust
side never touches a Play class directly. A small Java shim — one class per
service, compiled into the APK's dex — takes the arguments as primitives,
calls the SDK, and reports the result back through a `native` method that
pushes onto the same `Sender` the HTTP worker uses. That keeps every listener,
`Task`, and `ActivityResultLauncher` on the Java side where they are cheap,
and leaves one crossing per completion. The class loader is cached at
`android_main`, because a `FindClass` from any other thread sees only the
platform's classes.

**The template becomes a Gradle build.** This is the decision with the
largest blast radius, and there is no way around it: resolving
`com.google.android.gms:play-services-games-v2` by hand means resolving its
transitive graph by hand, merging manifest fragments by hand, and running
`d8` and `aapt2` over the result — which is Gradle, written badly. So
`scripts/package_template.sh android` gains a Gradle project whose only
Kotlin/Java is the shim, whose native library is the one cargo already
builds, and whose output is both an APK (installable, what CI checks) and an
AAB (uploadable). `scripts/assemble_apk.sh` stays for the no-Google path: a
game that declares no Play capabilities exports through the existing
aapt2 route and carries no dex at all.

**Capabilities are declared in `project.toml` and written at export.**

```toml
[android]
application_id = "com.studio.game"
min_sdk = 26
target_sdk = 35
capabilities = ["play-games", "play-billing", "play-integrity"]
# Play Games and Sign in with Google resolve against this; it comes from the
# Play Console, and Play App Signing may rewrite the certificate behind it.
games_project_id = "1234567890"
```

The exporter rewrites the manifest's package, label and the `meta-data` each
capability needs, and refuses a capability whose configuration is missing —
a Play Games build with no project id fails at export rather than at the
player's first launch.

**Determinism.** A platform result is external input: recorded per tick,
re-fed on replay. Two rules, both easy to get wrong:

- A call that changes the outside world — unlock, submit, purchase — goes
  through `ExternalIo::start`, which already refuses to spawn while a
  recording plays or a tick is being re-simulated (`replay::suppressed`).
- That suppression is right for a socket and subtly wrong for an unlock: the
  *predicted* run is the one that fired it. So `platform.unlock` and
  `platform.submit_score` are dispatched from a tick the rollback session has
  confirmed. With no rollback session, every tick is confirmed and the rule
  costs nothing.

**Where the identity ends up.** A Play Games sign-in gives the device a
player id, and `requestServerSideAccess` gives it a one-time code that only a
server can exchange. The verification belongs in `balaur_gamend`: a hook
takes the code (Play Games) or the ID token (Sign in with Google), calls
Google, and returns a Gamend session. The engine carries the proof and
decides nothing.

## 2. The surface

Every Google service an Android game reaches for, and where each stands here.

| Service | Decision |
| --- | --- |
| Play Games Services v2 sign-in | Step 3. Automatic at launch, no button, no sign-out flow; the game asks who the player is and gets an answer or a refusal |
| Play Games achievements, leaderboards, events | Step 3, behind `platform.unlock` / `platform.progress` / `platform.submit_score` / `platform.scores` |
| Play Games saved games (Snapshots) | Step 4, behind `platform.cloud_read` / `cloud_write`, with `save`'s file as the payload and its version as the conflict key |
| Play Games server-side access (`requestServerSideAccess`) | Step 3, exchanged by a Gamend hook |
| Play Games friends, player search, recall across devices | Step 4, on the same client |
| Sign in with Google via Credential Manager (`androidx.credentials` + the Google ID option) | Step 5. The non-game identity: an ID token for a game whose account is not a Play Games account |
| Play Billing Library 8 or later: one-time products, subscriptions, `queryPurchases`, acknowledgement | Step 6. Version 8 is the floor Play now requires of a new app or an update, so there is no older-library option to weigh |
| Purchase verification (Google Play Developer API), Real-time developer notifications | Step 6, server side, as a Gamend hook. A purchase the client alone has checked is not checked |
| Play Billing Lab / license testers | Step 6 — the only way a purchase is tested without money |
| Play Integrity API | Step 7. The verdict is a signed token for the *server* to read; a client that reads its own verdict has proved nothing |
| Play Asset Delivery (install-time, fast-follow, on-demand packs) | Step 8, and the one Google service that touches engine architecture: an on-demand pack is the asset-streaming row of the roadmap wearing Play's clothes. Needs the AAB from step 1 |
| Play Feature Delivery (dynamic feature modules) | Not planned. A feature module is Java code delivered late; the engine's late-delivered thing is assets |
| In-app review, in-app updates, install referrer | Step 9. Three small libraries, one event each |
| Firebase Crashlytics with the NDK reporter | Step 9, and worth more here than in most engines: a Rust panic in a NativeActivity is otherwise a line in `logcat` nobody sees. It overlaps the roadmap's self-reproducing crash report, which is the better answer where a recording exists |
| Firebase Analytics, Remote Config, Cloud Messaging, Auth, Firestore | Not planned as engine code. Each is a Java library with no engine seam to sit on, and a game that wants one wants its own; Gamend is where this engine's server-side answers live. Revisit if `google.call(class, method, args)` in step 9 proves too thin |
| AdMob and every other ads SDK | Not planned. Ads decide a game's data policy, its store listing and its privacy manifest; that is a game's decision to declare, not an engine's to link |
| Google Play Games on PC | Have, once step 3 lands: the same APIs, an `x86_64` ABI from step 1, and a keyboard-and-mouse input map the engine already has |
| Android App Bundle, `bundletool`, Play App Signing | Step 1 |
| 16 KB page alignment, `x86_64` ABI, target SDK upkeep | Step 1. None of it is Google-services work; all of it is why an exported game would be rejected today |
| GameActivity in place of NativeActivity | Not planned yet. It buys text input and motion-event handling, and it costs a kiss3d and `android-activity` change — a window decision, not a services one |
| Google Play Games Services v1 | Not planned. v2 is what a new integration gets, and v1's explicit sign-in flow is the thing v2 removed |

## 3. Steps

Ordered so the APK is shippable before it is clever, and each step leaves
something that works.

1. **An APK a developer can publish.** The `[android]` table; the exporter
   rewriting package, label and version; `max-page-size=16384` in
   `.cargo/config.toml`; an `x86_64` ABI in the template; and `bundletool`
   producing an AAB beside the APK. No Google code yet, and every existing
   game gets better.
2. **A dex, and one call across it.** The Gradle template, one Java class,
   `jni` and `ndk-context` in `crates/balaur_google`, the class loader cached
   at `android_main`, and one round trip — Java to Rust to a script handler —
   proving the channel. The no-capabilities export keeps the old aapt2 path.
3. **Play Games: player, achievements, leaderboards.** The v2 client, the
   automatic sign-in at launch, `requestServerSideAccess` exchanged by a
   Gamend hook, and the four portable verbs. Ends with: an unlock visible in
   the Play Games app.
4. **Saved games and friends.** Snapshots behind `platform.cloud_read` /
   `cloud_write`, with conflict resolution that refuses a newer file rather
   than overwriting it — the rule `save` already follows.
5. **Sign in with Google.** Credential Manager, the ID token, the same Gamend
   hook shape as step 3.
6. **Play Billing.** Products, purchase, acknowledgement, restore, and the
   Developer API verification hook in Gamend. Ends with: a licensed-tester
   purchase that survives a reinstall.
7. **Play Integrity.** A verdict token requested by the client and read by
   the server, and a Gamend hook that says yes or no.
8. **Play Asset Delivery.** On-demand packs behind an asset-source that the
   pack loader already has a seam for. This is the step that should wait for
   the roadmap's asset-streaming work rather than lead it.
9. **The long tail.** In-app review and updates, the install referrer,
   Crashlytics NDK, and `google.call` for the game that wants a Java library
   this plan did not name.

## 4. What CI can prove, and what it cannot

`scripts/export_mobile.sh` already draws this line and this plan does not
move it. What a runner can check:

- the Gradle template builds, the dex is in the APK, and the APK installs on
  an emulator far enough to log a line
- an exported game carries the project's application id, its capabilities'
  manifest entries, and the pack in `assets/`
- `bundletool build-apks` accepts the AAB, and every native library in it is
  16 KB aligned
- the plugin's replay test passes: a recorded session of canned platform
  events replays to a bit-identical digest with no Play services present

What it cannot: a real sign-in, unlock, purchase or integrity verdict. All
four need a Play Console app, an OAuth client bound to a signing certificate,
and — for Play Games — a build on a test track. The bar stays the one mobile
export holds: everything up to the point Play takes over, plus the
canned-event replay, which is the part that protects the simulation.

## 5. Open questions

1. **Does the Gradle template replace `assemble_apk.sh` or sit beside it?**
   Beside it is written above, because a game with no Play capabilities
   should not need a JDK to export. The cost is two Android paths to keep
   working, and CI has to build both.
2. **Where does the Java shim's source live?** In `crates/balaur_google` next
   to the Rust that calls it reads best and builds worst; under the template's
   Gradle project builds best and separates the two halves of one call. No
   decision.
3. **Does the exporter compile Java?** If a game may ship its own Java — an
   ads SDK, a platform library this plan did not name — then export needs a
   JDK and the template stops being prebuilt. The narrower answer, and the
   one to start with: the template's dex is fixed, and a game that needs more
   builds its own template.
4. **Play App Signing rewrites the certificate**, so the SHA-1 an OAuth
   client is bound to is the one Play holds, not the one the developer
   signed with. Nothing in the engine can check this, and it is the single
   most common way a Play Games integration fails. It belongs in the manual
   with a name, not in a plan with a shrug.
