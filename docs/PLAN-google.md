# Google platform services

`crates/balaur_google` is the Play backend behind the `platform` module on
Android: Play Games (sign-in, achievements, leaderboards, saved games),
Sign in with Google through Credential Manager, Play Billing, Firebase Cloud
Messaging and in-app review, each a Java shim under `crates/*/java` or
`crates/balaur_google/play/<capability>` that an export keeps for the
`[android] capabilities` it declares. None of it has run with the engine's
own library on a device or against a Play Console app; that run, a smaller
dex, and the services not yet wired are what is left.

## Steps

1. **A run against Play.** An export with the engine's own library, on a
   device or a `google_apis_playstore` emulator image signed in to Google,
   against a Play Console app and a Firebase project with this app's id.
   Exchanging Play Games' one-time code for a session and verifying a Play
   purchase token over the Play Developer API are Gamend hooks, Gamend's to
   build. Done: one round trip from Java to a script handler; an unlock
   visible in the Play Games app; a saved game written on one device and read
   on another; a license tester's purchase that survives a reinstall; and a
   push from Gamend reaching the game.
2. **A smaller dex.** A game keeps 6.4 MB of unshrunk dex for `play-games`,
   6.9 MB for `in-app-purchase` and 5.8 MB for `in-app-review`, most of it
   Kotlin's standard library, coroutines and androidx.core. The exporter
   compiles no Java and picks prebuilt files; merging and shrinking the
   picked dex with d8 or R8 needs only the build-tools the export already
   reads. Done: an export with all three capabilities whose dex is shrunk,
   installing and passing `verify` on an emulator.
3. **Friends.** Play Games' friends list on the same client as the
   leaderboards. Done: a signed-in player's friends answered to a script.
4. **Play Integrity.** A verdict token the client requests and a Gamend hook
   reads. A client reading its own verdict proves nothing. Done: a verdict
   Gamend accepts from a Play-installed build and refuses from a sideloaded
   one.
5. **Play Asset Delivery.** On-demand asset packs as an asset source, which
   needs the AAB. It waits for the roadmap's asset streaming rather than
   leading it. Done: an asset pack fetched on demand loads as assets.
6. **The long tail.** In-app updates, the install referrer, Crashlytics with
   the NDK reporter, and `google.call` for a Java library this plan does not
   name. In-app updates answer through an activity result, which needs an
   unseen activity of their own, as the share sheet's has. Done: each answers
   a script on a device.

CI proves what needs no device: the shims compile under `-Werror` against
the pinned jars, the export's choice of files and its refusals hold, and the
answers route through stand-ins and replay the same. Still to put in CI: the
APK installing on an emulator.

Not planned: Play Feature Delivery, since the engine delivers assets late
and not code. Nor are Firebase beyond Crashlytics and Cloud Messaging, ads
SDKs (a game's data-policy decision) or Play Games v1. GameActivity waits for
a window change that asks for it.
