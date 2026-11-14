# Apple platform services

What is left of the Apple backend, `crates/balaur_apple`, and the `[apple]`
export table. ARCHITECTURE.md's "Store and platform services" holds what is
built.

## Steps

1. **Clamp `min_ios` and `min_macos` to the template.** `AppleConfig`'s check
   in `crates/balaur_export/src/apple.rs` compares the declared version with
   each capability's minimum only, and returns early when no capability is
   declared. The iOS template's `Info.plist` carries the deployment target
   `scripts/package_runtime.sh` built it at. Done: an export declaring less
   fails with an error naming that floor, as `[android] min_sdk` does in
   `crates/balaur_export/src/android.rs`.
2. **Present Game Center's sign-in sheet.** When GameKit hands
   `crates/balaur_apple/src/gamekit.rs` a view controller, the sign-in is
   reported rather than shown. Done: a sign-in with no player signed in shows
   Apple's sheet and answers once the player finishes it.
3. **Run against Apple's servers.** Game Center, iCloud and StoreKit, and the
   arrivals `notification_received`, `push_received`, `invite_accepted`,
   `match_requested` and `cloud_changed`, have only been compiled and tested
   through a canned backend. Done: each answers on a device signed with an
   Apple ID and a provisioning profile.

## Open questions

- Should buying become portable? `apple.purchase` is Apple's own, because no
  second store here implements purchases. Design the `platform` verb once Play
  Billing or Steam's inventory exists beside it.
