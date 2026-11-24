# Google platform services

Play Games Services, Sign in with Google, Play Billing, Play Integrity and
Play Asset Delivery on Android, as a backend behind the `platform` module and
a `google` module beside it for what only Play has. The export already writes
a signed APK and AAB, and the template carries one `classes.dex` of the Java
shims in `crates/balaur_google/java` (step 1).

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
3. **Saved games and friends.** Snapshots behind `platform::cloud_read` and
   `cloud_write`, refusing a newer file rather than overwriting it as `save`
   does, and friends on the same client.
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
   reaches Play services' own sign-in there. Custom Tabs are still to do.
5. **Play Billing.** Billing Library 8 or later: products, purchase,
   acknowledgement and restore, verified by a Gamend hook over the Play
   Developer API. Done: a license tester's purchase that survives a
   reinstall.
6. **Play Integrity.** A verdict token the client requests and a Gamend hook
   reads. A client reading its own verdict proves nothing.
7. **Play Asset Delivery.** On-demand asset packs as an asset source, which
   needs the AAB. It waits for the roadmap's asset streaming rather than
   leading it.
8. **The long tail.** In-app review and updates, the install referrer,
   Crashlytics with the NDK reporter, and `google.call` for a Java library
   this plan does not name.

CI proves what needs no Play Console app. The Gradle template builds, the
dex is in the APK, and the APK installs on an emulator. The manifest carries
each capability's entries, and canned platform events replay to a
bit-identical digest.

Not planned: Play Feature Delivery, since the engine delivers assets late
and not code. Nor are Firebase beyond Crashlytics, ads SDKs (a game's
data-policy decision) or Play Games v1. GameActivity waits for a window
change that asks for it.

## Open questions

1. **Where does the Java shim's source live?** Beside the Rust that calls it
   in `crates/balaur_google`, or under the template's Gradle project.
2. **Does the exporter compile Java?** If a game ships its own Java, export
   needs a JDK and the template stops being prebuilt. Start with a fixed dex:
   a game that needs more builds its own template.
3. **Play App Signing rewrites the certificate** an OAuth client is bound
   to. Nothing in the engine can check it, so the manual names it.
