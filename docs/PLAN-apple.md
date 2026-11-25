# Apple platform services

What is left of the Apple backend, `crates/balaur_apple`, and the `[apple]`
export table. ARCHITECTURE.md's "Store and platform services" holds what is
built.

## Steps

1. **Present Game Center's sign-in sheet.** When GameKit hands
   `crates/balaur_apple/src/gamekit.rs` a view controller, the sign-in is
   reported rather than shown. Done: a sign-in with no player signed in shows
   Apple's sheet and answers once the player finishes it.
2. **Run against Apple's servers.** Game Center, iCloud, StoreKit, the
   sign-in page (`platform.web_sign_in`), and the
   arrivals `invite_accepted`, `match_requested` and
   `cloud_changed`, have only been compiled and tested through a canned
   backend. Done: each answers on a device signed with an Apple ID and a
   provisioning profile.
3. **Run the device seam on hardware.** Local notifications are
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

4. **Push from Gamend to a device.** `platform.register_for_push` asks
   APNs through the application delegate (`push.rs`) and answers
   `push_registered` with the hex `token`, `provider = "apns"` and
   `platform = "ios"`, a Mac's too, since Gamend's push tokens have one Apple
   platform. A token the OS hands over with no call waiting, and different from
   the last, is `push_token_changed`. A push shown in front is
   `push_received`, told apart from a local notification by its
   `UNPushNotificationTrigger`, and a tap is `notification_opened` with the
   payload less `aps` as `data`; a silent push reaches the delegate alone and
   is `push_received` with no title. `[apple] capabilities =
   ["push-notifications"]` writes `aps-environment` (macOS:
   `com.apple.developer.aps-environment`): `development` for a build signed by
   an `Apple Development`, `iPhone Developer` or `Mac Developer` identity, ad
   hoc or not at all, `production` for any other. Without it the call answers
   `push_failed` naming the capability: a Mac reads its own signature's
   entitlement first, and iOS maps APNs' refusal, which names
   `aps-environment`. Tested through a canned OS and the export's
   entitlements only. Still to check: an APNs key in Gamend, a push sent to a
   signed device, a tap that launches the game, which the notification
   centre's delegate hears only when it is set before launch ends, and a Mac
   build carrying the entitlement.
5. **Sign a Mac build against its profile.** The export embeds
   `Contents/embedded.provisionprofile` and leaves out a capability the
   profile does not grant, as a Developer ID profile grants no Sign in with
   Apple, and `docker/sign.sh` does the same with a mounted
   `macos_provisioning_profile`. Tested with a profile's plist in a stand-in
   envelope, and the parser against a real Developer ID profile. Done: a
   Developer ID build with `sign-in-with-apple` and `push-notifications`,
   signed against its profile, launches on a Mac, answers `unsupported` to
   `apple.sign_in`, registers for push and passes notarization.

## Open questions

- Buying is portable now: `platform.products`, `purchase`,
  `finish_purchase`, `restore_purchases` and `entitlements` over StoreKit and
  Play Billing, with Steam answering `unsupported`. A transaction's `token`
  is StoreKit's JWS. Left open: whether `finish_purchase` should refuse a
  transaction StoreKit could not verify, rather than leave that to the server.
