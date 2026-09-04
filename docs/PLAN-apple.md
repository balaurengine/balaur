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
