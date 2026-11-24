# Google platform services

Play Games Services, Sign in with Google, Play Billing, Play Integrity and
Play Asset Delivery on Android, as a backend behind the `platform` module and
a `google` module beside it for what only Play has. The export already writes
a signed APK and AAB. The template carries one `classes.dex` of the Java
shims every game gets (step 1), and Google's libraries under `play/`, which
an export keeps for the `[android] capabilities` it declares (step 2).

## Steps

1. **A dex, and one call across it.** A Gradle project in
   `scripts/package_runtime.sh android` whose only Java is a shim, one class
   per service, built around the native library cargo already builds. The
   shim reports each completion through a `native` method onto the
   `ExternalIo` channel, so it lands at `Stage::First`, recorded.
   `crates/balaur_google` holds the JNI side over `jni` and `ndk-context`, and
   caches the activity's class loader at `android_main`, since `FindClass`
   from another thread sees only the platform's classes. A game declaring no
   Play capability keeps the aapt2 path and carries no dex. Done: one round
   trip from Java to a script handler, and a replay of it with no Play
   services present.
   *Built 2026-11-24, without Gradle*: the one shim so far needs only the
   platform, so `package_runtime.sh android` runs javac and d8 against
   `android.jar` and stages `classes.dex`; the manifest says `hasCode="true"`,
   and the export puts the dex at an APK's root and under a bundle's `dex/`.
   `crates/balaur_google/src/android.rs` loads the shim through the
   activity's class loader and registers its native `answer` on it, which
   sends a JSON map onto the call's `ExternalIo` channel. Gradle waits for the
   first service that needs a Maven library (Play Games, Billing). Still to
   check: the round trip on a device or an emulator.
2. **Play Games: player, achievements, leaderboards.** The v2 client with its
   automatic sign-in, `platform::sign_in`, `player`, `unlock`, `progress`,
   `submit_score` and `scores`, each write held until its tick settles.
   `requestServerSideAccess` hands a one-time code to a Gamend hook that
   returns a session. `[android]` gains `capabilities` and
   `games_project_id`, and export refuses a capability whose configuration is
   missing. Done: an unlock visible in the Play Games app.
   *Built 2026-11-24, without Gradle*: `crates/balaur_google/play/libraries.json`
   pins each capability's root (play-services-games-v2 22.1.0, billing 9.1.0,
   review 2.0.2) and the 50 artifacts their POMs pull in, each with its
   repository, SHA-256 and licence; `scripts/play_libraries.py resolve`
   writes it, `--check` fails a root bumped without it, and `stage` (run by
   `package_runtime.sh android`) downloads each file against its pin, dexes
   every library on its own with d8, links their resources once to fix the
   ids their R classes are generated with, compiles each capability's shim
   (`play/<capability>/`) against its libraries, and merges the libraries'
   manifests for every set of capabilities. `play/` is 8.6 MB staged, 2.7 MB
   in the template's tarball. An export keeps a capability's dex files as
   `classes2.dex` on, its resources linked `--stable-ids`, and its manifest
   entries; a game declaring none carries none of it, and `[android]
   games_project_id` becomes Play Games' `APP_ID` through a string resource.
   Unshrunk, the dex a game keeps is 6.4 MB for `play-games`, 6.9 MB for
   `in-app-purchase` and 5.8 MB for `in-app-review`, most of it Kotlin's
   standard library, coroutines and androidx.core: R8 at export is the next
   cut. `Games.java` signs in by itself as the game starts (a request-0
   `signed_in`), and answers `sign_in`, `unlock`, `progress` (an incremental
   achievement's share of its steps), `submit_score`, `scores` (paged 25 at a
   time from `start`) and `identity` (`requestServerSideAccess`'s one-time
   `code` for `[google] server_client_id`) as the maps `platform.*` waiters
   receive. Exchanging that code for a session is a Gamend hook, Gamend's
   to build. Checked: the shims compile under `-Werror` against the pinned
   jars, the routing and the answers through stand-ins, a canned store and a
   replay, the export's choice of files and its refusals. `hello` exported
   with all three capabilities is a 3.4 MB APK against 370 KB with none
   (a stand-in `libmain.so` in both), its R classes hold the ids aapt2 gave
   (`google_play_services_version` is 0x7f060000 in both), and on an Android
   35 emulator with Google APIs it installs, its 52 dex files pass `verify`,
   and the libraries' providers start before the native library loads. Not
   yet run with the engine's library, on a device, or against a Play Console
   app: that emulator image has Play services but no Play Store, so Billing
   needs a `google_apis_playstore` image.
3. **Saved games and friends.** Snapshots behind `platform::cloud_read` and
   `cloud_write`, refusing a newer file rather than overwriting it as `save`
   does, and friends on the same client.
   *Saved games built 2026-11-24*: a key is a snapshot of that name. A read
   resolves a conflict to the save modified last and answers nil for one
   never written; a write opens with manual resolution and answers `error`
   on a conflict rather than overwriting the other device's save. Friends
   are still to do, and none of it has run against Play.
4. **Sign in with Google.** Credential Manager with the Google ID option, its
   ID token verified by the same Gamend hook shape as step 2. Custom Tabs
   behind `platform.web_sign_in`, which answers `unsupported` on Android until
   then.
   *Sign-in built 2026-11-24*: `google.sign_in()` runs the system's own
   Credential Manager (Android 14 and later) with the bundle keys googleid
   1.2.1's `GetGoogleIdOption` writes, read off its classes, so no Google
   library ships. The accounts already signed in to the app come first,
   picked without a tap when there is one, then every account. It answers
   `signed_in` with `token`, `email` and `name`, `cancelled`, `error`, or
   `unsupported` off Android, below 14, with no `[google]
   server_client_id` (the server's web client id) or with no Google account
   on the device. No nonce: Gamend's
   `tokeninfo` check reads none. A stand-in answers `google.sign_in` in a
   test. Below Android 14 a game opens Google's page, since only androidx
   reaches Play services' own sign-in there; `platform.is_available(
   SERVICE_GOOGLE_SIGN_IN)` says which, from the client id and Credential
   Manager being on the device. Custom Tabs are built (ARCHITECTURE.md).
5. **Play Billing.** Billing Library 8 or later: products, purchase,
   acknowledgement and restore, verified by a Gamend hook over the Play
   Developer API. Done: a license tester's purchase that survives a
   reinstall.
   *Built 2026-11-24 on Billing 9.1.0*: the purchase verbs are portable,
   `platform.products`, `purchase`, `finish_purchase` (acknowledge, or
   consume with `consume = true`), `restore_purchases` and `entitlements`
   (`queryPurchasesAsync` for both product types), StoreKit's moved onto them
   from `apple.*`. A Play transaction's `id` and `token` are its purchase
   token; a purchase landing with no flow open, a pending one clearing, is
   `transaction` on `platform_event`. Verifying the token over the Play
   Developer API is a Gamend hook, Gamend's to build. Not run on a device.
6. **Play Integrity.** A verdict token the client requests and a Gamend hook
   reads. A client reading its own verdict proves nothing.
7. **Play Asset Delivery.** On-demand asset packs as an asset source, which
   needs the AAB. It waits for the roadmap's asset streaming rather than
   leading it.
8. **The long tail.** In-app updates, the install referrer, Crashlytics with
   the NDK reporter, and `google.call` for a Java library this plan does not
   name. In-app review is built: `device.request_review` runs Play's review
   flow where the export declares `in-app-review`, answering `done` whether
   or not Play showed it, and `unsupported` without it. In-app updates wait:
   their flow answers through an activity result, which needs an unseen
   activity of its own, as the share sheet's has.

CI proves what needs no Play Console app. The template builds, the dex is
in the APK, and the APK installs on an emulator. The manifest carries
each capability's entries, and canned platform events replay to a
bit-identical digest.

Not planned: Play Feature Delivery, since the engine delivers assets late
and not code. Nor are Firebase beyond Crashlytics, ads SDKs (a game's
data-policy decision) or Play Games v1. GameActivity waits for a window
change that asks for it.

## Open questions

1. **Where does the Java shim's source live?** Settled: beside the Rust that
   calls it, `crates/*/java` for what every game carries and
   `crates/balaur_google/play/<capability>` for what a capability's
   libraries carry. There is no Gradle project.
2. **Does the exporter compile Java?** Settled for now: no. The template is
   prebuilt with every capability's dex, and an export picks files. A game
   that ships its own Java builds its own template. Merging or shrinking the
   picked dex with d8 or R8 at export would need only the build-tools the
   export already reads, and is the next size cut.
3. **Play App Signing rewrites the certificate** an OAuth client is bound
   to. Nothing in the engine can check it, so the manual names it.
4. **Firebase Cloud Messaging** (phase 4) takes the same path: a
   `push` capability whose root is `firebase-messaging`, resolved into the
   same lock and staged by the same script. Its `google-services.json`
   values become string resources the export writes, as
   `games_project_id` does.
