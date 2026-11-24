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
3. **Run against Apple's servers.** Game Center, iCloud, StoreKit, the
   sign-in page (`platform.web_sign_in`), and the
   arrivals `push_received`, `invite_accepted`, `match_requested` and
   `cloud_changed`, have only been compiled and tested through a canned
   backend. Done: each answers on a device signed with an Apple ID and a
   provisioning profile.
4. **Run the device seam on hardware.** Local notifications are
   `platform.notify` now, served by `crates/balaur_apple/src/notify.rs`
   through the device seam whichever store is loaded, and a tap or one shown
   in front is the `platform_event` event. The iPhone's feedback generators,
   the share sheet (`UIActivityViewController`, `NSSharingServicePicker`) and
   `SKStoreReviewController` (`device.rs`) are compiled for iOS and macOS and
   tested through a canned OS only. `platform.is_available` answers Game
   Center, Sign in with Apple, notifications and push from the bundle id,
   iCloud from `ubiquityIdentityToken`, purchases from `canMakePayments`.
   Done: a notification scheduled, tapped and heard, a file shared and a
   haptic felt on an iPhone.

## Open questions

- Buying is portable now: `platform.products`, `purchase`,
  `finish_purchase`, `restore_purchases` and `entitlements` over StoreKit and
  Play Billing, with Steam answering `unsupported`. A transaction's `token`
  is StoreKit's JWS. Left open: whether `finish_purchase` should refuse a
  transaction StoreKit could not verify, rather than leave that to the server.
