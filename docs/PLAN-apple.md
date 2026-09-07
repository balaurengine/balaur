> **Status:** built on 2026-09-03 and 2026-09-04, every step — the `[apple]`
> export table, `crates/balaur_platform` and the portable `platform.*` module,
> and `crates/balaur_apple` for Game Center, iCloud, Sign in with Apple,
> StoreKit, notifications and opened URLs. ARCHITECTURE.md's platform sections
> are the record. `docs/PLAN-google.md` is the same document for Android,
> `docs/PLAN-steam.md` for the desktop stores. What is left is below.

# Plan: Apple platform services — what is still open

**Never run against Apple's servers.** The export half is tested and the seam
is tested through a canned backend, but the framework code — the Swift shim
included — only compiles for macOS and iOS. Game Center, iCloud and StoreKit
have never talked to Apple, which needs an Apple ID, a provisioning profile
and hardware.

1. **A URL the game was launched with is still not delivered.** It reaches the
   application delegate before the engine has booted, and the proxy goes up
   after. Reading it would mean the window layer holding the launch options
   for us, which is a change to kiss3d rather than to this crate.
2. **Should buying become portable?** `apple.purchase` is Apple's, because
   `platform.*` carries what every store shares and no second store here has
   implemented purchases yet. Play Billing and Steam's inventory are shaped
   differently enough that the portable verb should be designed against two of
   them, not one.
3. **`min_os` is free text over a floor the project cannot see.**
   `AppleConfig::min_os` is written into `MinimumOSVersion` exactly as given,
   while `scripts/package_template.sh` builds the iOS template at
   `IPHONEOS_DEPLOYMENT_TARGET=15.0`. A game that sets `min_os = "12.0"` ships
   a plist promising what its binary cannot do, and the failure lands on a
   player's device rather than in the build. The exporter should clamp, with an
   error that names the floor, and a game that wants lower rebuilds the
   template against a lower deployment target. `min_macos` has the same shape.
   `docs/PLAN-google.md` carries the matching note for `min_sdk` against the
   NDK level, so one check serves both platforms.
