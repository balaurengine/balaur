//! `[apple]`'s plist, entitlements and refusals.

use super::{AppleConfig, Build, Capability, Platform};
use crate::profile::Profile;
use crate::profile::tests::profile_granting;

fn config(table: &str) -> AppleConfig {
    #[derive(serde::Deserialize)]
    struct Manifest {
        #[serde(default)]
        apple: AppleConfig,
    }
    AppleConfig {
        title: "Game".into(),
        ..toml::from_str::<Manifest>(table)
            .expect("the table parses")
            .apple
    }
}

#[test]
fn the_apple_table_names_its_signing_identities() {
    let apple = config(
        "[apple]\nmacos_identity = \"Developer ID Application: Studio\"\nnotarize = true\n\
         ios_provisioning_profile = \"signing/game.mobileprovision\"\n",
    );
    assert_eq!(apple.macos_identity, "Developer ID Application: Studio");
    assert!(apple.notarize);
    assert_eq!(
        apple.ios_provisioning_profile,
        "signing/game.mobileprovision"
    );
}

// `[window] orientation` lands in the plist iOS reads, and a project
// that names the key itself keeps its own answer.
#[test]
fn an_orientation_reaches_the_plist() {
    let held = AppleConfig {
        orientation: balaur::project::Orientation::Portrait,
        ..config("[apple]\nbundle_id = \"com.studio.game\"\n")
    };
    let text = held.info_plist(Platform::Ios, "Balaur", "").unwrap();
    assert!(
        text.contains("<key>UISupportedInterfaceOrientations</key>"),
        "{text}"
    );
    assert!(text.contains("UIInterfaceOrientationPortrait"), "{text}");
    assert!(!text.contains("LandscapeLeft"), "{text}");

    let none = config("[apple]\nbundle_id = \"com.studio.game\"\n")
        .info_plist(Platform::Ios, "Balaur", "")
        .unwrap();
    assert!(!none.contains("UISupportedInterfaceOrientations"), "{none}");

    let named = AppleConfig {
        orientation: balaur::project::Orientation::Portrait,
        ..config(
            "[apple]\nbundle_id = \"com.studio.game\"\n[apple.plist]\n\
             UISupportedInterfaceOrientations = [\"UIInterfaceOrientationLandscapeLeft\"]\n",
        )
    };
    let text = named.info_plist(Platform::Ios, "Balaur", "").unwrap();
    assert!(text.contains("LandscapeLeft"), "{text}");
    assert!(!text.contains("UIInterfaceOrientationPortrait"), "{text}");
}

#[test]
fn every_example_exports_to_ios() {
    let examples = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let mut checked = 0;
    for entry in std::fs::read_dir(&examples).unwrap() {
        let project = entry.unwrap().path();
        let Ok(text) = std::fs::read_to_string(project.join("project.toml")) else {
            continue;
        };
        let manifest: toml::Table = toml::from_str(&text).unwrap();
        AppleConfig::from_manifest(&manifest, &project)
            .and_then(|config| config.check(Platform::Ios))
            .unwrap_or_else(|err| panic!("{}: {err}", project.display()));
        checked += 1;
    }
    assert!(checked > 0, "no example under {}", examples.display());
}

// Nothing is invented: each key missing is refused by name, in the order
// a project would fill them in.
#[test]
fn a_project_with_no_apple_table_is_refused_key_by_key() {
    let refusal = |table: &str| config(table).check(Platform::Ios).unwrap_err().to_string();
    assert!(refusal("name = \"hello\"\n").contains("bundle_id is not set"));
    let id = "[apple]\nbundle_id = \"com.studio.game\"\n";
    assert!(refusal(id).contains("version is not set"));
    assert!(refusal(&format!("{id}version = \"1.0\"\n")).contains("build_number is not set"));
    let untitled = AppleConfig {
        title: String::new(),
        ..config(id)
    };
    let err = untitled.check(Platform::Ios).unwrap_err().to_string();
    assert!(err.contains("[application] name"), "{err}");
    let named = config(&format!("{id}version = \"1.0\"\nbuild_number = \"1\"\n"));
    named
        .check(Platform::Ios)
        .expect("a named game with no capabilities");
    assert!(
        named
            .entitlements(Platform::Ios, Build::Development, false, false)
            .is_none()
    );
}

#[test]
fn a_capability_without_a_bundle_id_is_refused() {
    let config = config("[apple]\ncapabilities = [\"game-center\"]\n");
    let err = config
        .check(Platform::Ios)
        .expect_err("an entitlement on an identifier no account owns")
        .to_string();
    assert!(err.contains("bundle_id"), "{err}");
}

#[test]
fn a_misspelled_capability_names_the_ones_that_exist() {
    // `Debug`: `expect_err` prints the Ok value when there is one.
    #[derive(Debug, serde::Deserialize)]
    struct Manifest {
        #[allow(dead_code, reason = "the parse is what this test reads")]
        apple: AppleConfig,
    }
    let err = toml::from_str::<Manifest>("[apple]\ncapabilities = [\"gamecenter\"]\n")
        .expect_err("an unknown capability")
        .to_string();
    assert!(err.contains("game-center"), "{err}");
}

#[test]
fn the_icloud_store_identifier_needs_a_team_because_it_carries_one() {
    let config = config(
        "[apple]\nbundle_id = \"com.studio.game\"\nversion = \"1.0\"\nbuild_number = \"1\"\ncapabilities = [\"icloud-kv\"]\n",
    );
    let err = config
        .check(Platform::Ios)
        .expect_err("nothing expands $(TeamIdentifierPrefix) here")
        .to_string();
    assert!(err.contains("team_id"), "{err}");
}

#[test]
fn game_center_below_the_version_its_identity_signature_needs_is_refused() {
    let config = config(
        "[apple]\nbundle_id = \"com.studio.game\"\nversion = \"1.0\"\nbuild_number = \"1\"\nmin_ios = \"13.0\"\n\
         capabilities = [\"game-center\"]\n",
    );
    let err = config
        .check(Platform::Ios)
        .expect_err("13.0 predates fetchItemsForIdentityVerificationSignature")
        .to_string();
    assert!(err.contains("13.5"), "{err}");
}

#[test]
fn every_capability_writes_its_own_entitlement() {
    let config = config(
        "[apple]\nbundle_id = \"com.studio.game\"\nversion = \"1.0\"\nbuild_number = \"1\"\nteam_id = \"AB12CD34EF\"\n\
         min_ios = \"15.0\"\n\
         capabilities = [\"sign-in-with-apple\", \"game-center\", \"icloud-kv\"]\n",
    );
    config.check(Platform::Ios).expect("a complete table");
    let text = config
        .entitlements(Platform::Ios, Build::Development, false, false)
        .expect("three capabilities");
    assert!(text.contains("com.apple.developer.applesignin"), "{text}");
    assert!(text.contains("com.apple.developer.game-center"), "{text}");
    assert!(
        text.contains("<string>AB12CD34EF.com.studio.game</string>"),
        "{text}"
    );
}

#[test]
fn the_plist_carries_the_projects_identifier_and_its_own_keys() {
    let config = config(
        "[apple]\nbundle_id = \"com.studio.game\"\ndisplay_name = \"My Game\"\n\
         version = \"2.1\"\nbuild_number = \"7\"\nmin_ios = \"15.0\"\n\
         [apple.plist]\nITSAppUsesNonExemptEncryption = false\n\
         UISupportedInterfaceOrientations = [\"UIInterfaceOrientationLandscapeLeft\"]\n",
    );
    let text = config.info_plist(Platform::Ios, "Balaur", "").unwrap();
    assert!(text.contains("<key>CFBundleIdentifier</key><string>com.studio.game</string>"));
    assert!(text.contains("<key>CFBundleDisplayName</key><string>My Game</string>"));
    assert!(text.contains("<key>CFBundleShortVersionString</key><string>2.1</string>"));
    assert!(text.contains("<key>MinimumOSVersion</key><string>15.0</string>"));
    assert!(text.contains("<key>ITSAppUsesNonExemptEncryption</key><false/>"));
    assert!(text.contains("<string>UIInterfaceOrientationLandscapeLeft</string>"));
}

#[test]
fn a_macos_plist_declares_its_own_minimum_and_no_iphone_keys() {
    let config = config("[apple]\nbundle_id = \"com.studio.game\"\nmin_macos = \"13.0\"\n");
    let text = config.info_plist(Platform::Macos, "game", "").unwrap();
    assert!(text.contains("<key>LSMinimumSystemVersion</key><string>13.0</string>"));
    assert!(!text.contains("LSRequiresIPhoneOS"), "{text}");
}

#[test]
fn in_app_purchase_writes_no_entitlement_and_still_checks_the_version() {
    let below = config(
        "[apple]\nbundle_id = \"com.studio.game\"\nversion = \"1.0\"\nbuild_number = \"1\"\nmin_ios = \"14.0\"\n\
         capabilities = [\"in-app-purchase\"]\n",
    );
    let err = below
        .check(Platform::Ios)
        .expect_err("StoreKit 2 starts at 15.0")
        .to_string();
    assert!(err.contains("15.0"), "{err}");

    let config = config(
        "[apple]\nbundle_id = \"com.studio.game\"\nversion = \"1.0\"\nbuild_number = \"1\"\ncapabilities = [\"in-app-purchase\"]\n",
    );
    config.check(Platform::Ios).expect("the default is 15.0");
    assert!(
        config
            .entitlements(Platform::Ios, Build::Development, false, false)
            .is_none(),
        "in-app purchase needs no entitlement of its own"
    );
}

#[test]
fn a_capability_is_spelled_the_way_the_entitlement_is() {
    assert_eq!(Capability::GameCenter.name(), "game-center");
}

// The name a message prints is the one a project writes: the settings
// screen offers these, and a spelling that did not read back would be
// refused at export.
#[test]
fn every_capability_reads_back_from_the_name_it_prints() {
    for capability in [
        Capability::Applesignin,
        Capability::GameCenter,
        Capability::IcloudKv,
        Capability::InAppPurchase,
        Capability::PushNotifications,
    ] {
        let text = format!("capabilities = [\"{}\"]", capability.name());
        let read: toml::Table = toml::from_str(&text).unwrap();
        let parsed: Vec<Capability> = read["capabilities"].clone().try_into().unwrap();
        assert_eq!(parsed, [capability]);
    }
}

fn pushing() -> AppleConfig {
    config(
        "[apple]\nbundle_id = \"com.studio.game\"\nversion = \"1.0\"\nbuild_number = \"1\"\n\
         capabilities = [\"push-notifications\"]\n",
    )
}

#[test]
fn push_writes_aps_environment_for_the_build_its_identity_signs() {
    let config = pushing();
    config.check(Platform::Ios).expect("a complete table");
    for (identity, aps) in [
        (None, "development"),
        (Some("Apple Development: Leo (AB12CD34EF)"), "development"),
        (Some("iPhone Developer: Leo (AB12CD34EF)"), "development"),
        (
            Some("Apple Distribution: Studio (AB12CD34EF)"),
            "production",
        ),
        (
            Some("iPhone Distribution: Studio (AB12CD34EF)"),
            "production",
        ),
    ] {
        let text = config
            .entitlements(Platform::Ios, Build::of(identity), false, false)
            .expect("push writes an entitlement");
        assert!(
            text.contains(&format!(
                "<key>aps-environment</key>\n  <string>{aps}</string>"
            )),
            "{identity:?}: {text}"
        );
    }
}

#[test]
fn a_mac_writes_push_under_its_own_key() {
    let config = pushing();
    config.check(Platform::Macos).expect("a complete table");
    let release = config
        .entitlements(
            Platform::Macos,
            Build::of(Some("Developer ID Application: Studio (AB12CD34EF)")),
            false,
            false,
        )
        .unwrap();
    assert!(
        release.contains(
            "<key>com.apple.developer.aps-environment</key>\n  <string>production</string>"
        ),
        "{release}"
    );
    let ad_hoc = config
        .entitlements(Platform::Macos, Build::of(Some("-")), false, false)
        .unwrap();
    assert!(ad_hoc.contains("<string>development</string>"), "{ad_hoc}");
    assert!(!ad_hoc.contains("<key>aps-environment</key>"), "{ad_hoc}");
}

#[test]
fn push_below_the_os_that_shows_one_in_front_is_refused() {
    let config = AppleConfig {
        min_macos: "10.13".into(),
        ..pushing()
    };
    let err = config.check(Platform::Macos).unwrap_err().to_string();
    assert!(
        err.contains("push-notifications") && err.contains("10.14"),
        "{err}"
    );
}

#[test]
fn a_game_without_push_writes_no_aps_environment() {
    let config = config(
        "[apple]\nbundle_id = \"com.studio.game\"\nversion = \"1.0\"\nbuild_number = \"1\"\n\
         capabilities = [\"game-center\"]\n",
    );
    let text = config
        .entitlements(Platform::Ios, Build::Distribution, false, false)
        .unwrap();
    assert!(!text.contains("aps-environment"), "{text}");
}

// Each capability with an entitlement is one a profile must grant; in-app
// purchase writes none and needs nothing of a profile.
#[test]
fn what_a_profile_must_grant_is_what_the_capabilities_write() {
    let asked = config(
        "[apple]\nbundle_id = \"com.studio.game\"\n\
         capabilities = [\"in-app-purchase\", \"game-center\", \"push-notifications\"]\n",
    );
    assert_eq!(
        asked.restricted(Platform::Macos),
        [
            ("game-center", "com.apple.developer.game-center"),
            ("push-notifications", "com.apple.developer.aps-environment"),
        ]
    );
    assert_eq!(
        asked.restricted(Platform::Ios)[1],
        ("push-notifications", "aps-environment")
    );
    let err = asked
        .needs_profile(Platform::Macos)
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("game-center, push-notifications")
            && err.contains("macos_provisioning_profile"),
        "{err}"
    );
    let buying =
        config("[apple]\nbundle_id = \"com.studio.game\"\ncapabilities = [\"in-app-purchase\"]\n");
    buying
        .needs_profile(Platform::Macos)
        .expect("StoreKit needs no profile");
}

#[test]
fn a_profile_is_checked_for_every_entitlement_and_names_the_ones_it_lacks() {
    let config = config(
        "[apple]\nbundle_id = \"com.studio.game\"\n\
         capabilities = [\"sign-in-with-apple\", \"push-notifications\"]\n",
    );
    let path = std::path::Path::new("signing/game.provisionprofile");
    let partial = Profile::parse(&profile_granting(
        "    <key>com.apple.developer.applesignin</key><array><string>Default</string></array>\n",
    ))
    .unwrap();
    let err = config
        .check_profile(Platform::Macos, &partial, path)
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("push-notifications (com.apple.developer.aps-environment)")
            && err.contains("signing/game.provisionprofile")
            && !err.contains("sign-in-with-apple"),
        "{err}"
    );
    let whole = Profile::parse(&profile_granting(
        "    <key>com.apple.developer.applesignin</key><array><string>Default</string></array>\n\
         \x20   <key>com.apple.developer.aps-environment</key><string>development</string>\n",
    ))
    .unwrap();
    config
        .check_profile(Platform::Macos, &whole, path)
        .expect("a profile granting both");
    assert_eq!(
        Build::granted(&whole, Platform::Macos),
        Some(Build::Development)
    );
    assert_eq!(Build::granted(&whole, Platform::Ios), None);
}

// The template's floor is the one a plist may not go under, capability
// or none, as `[android] min_sdk` may not go under the template's.
#[test]
fn a_deployment_target_under_the_templates_is_refused() {
    let low = config("[apple]\nbundle_id = \"com.studio.game\"\nmin_ios = \"14.0\"\n");
    let err = low
        .check_floor(Platform::Ios, (15, 0))
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("min_ios = \"14.0\"") && err.contains("15.0"),
        "{err}"
    );
    low.check_floor(Platform::Ios, (13, 0))
        .expect("over a lower floor");
    let mac = config("[apple]\nbundle_id = \"com.studio.game\"\nmin_macos = \"12.0\"\n");
    mac.check_floor(Platform::Macos, (12, 0))
        .expect("the floor itself");
    let err = mac
        .check_floor(Platform::Macos, (13, 1))
        .unwrap_err()
        .to_string();
    assert!(err.contains("min_macos") && err.contains("13.1"), "{err}");
}

// The Developer ID profile Apple makes for an App ID with Push, In-App
// Purchase and Sign in with Apple grants push alone: a Mac build keeps
// push and leaves Sign in with Apple out.
#[test]
fn a_mac_build_leaves_out_what_its_profile_does_not_grant() {
    let declared = config(
        "[apple]\nbundle_id = \"com.studio.game\"\n\
         capabilities = [\"sign-in-with-apple\", \"in-app-purchase\", \"push-notifications\"]\n",
    );
    let developer_id = Profile::parse(&profile_granting(
        "    <key>com.apple.developer.aps-environment</key><string>production</string>\n",
    ))
    .unwrap();
    let path = std::path::Path::new("game.provisionprofile");
    let kept = declared.granted_by(Platform::Macos, &developer_id, path);
    assert_eq!(
        kept.capabilities,
        [Capability::InAppPurchase, Capability::PushNotifications]
    );
    let text = kept
        .entitlements(Platform::Macos, Build::Distribution, false, false)
        .unwrap();
    assert!(!text.contains("applesignin"), "{text}");
    assert!(
        declared
            .check_profile(Platform::Ios, &developer_id, path)
            .is_err(),
        "iOS stays strict"
    );
}

fn linked(extra: &str) -> AppleConfig {
    config(&format!(
        "[apple]\nbundle_id = \"com.studio.game\"\nversion = \"1.0\"\nbuild_number = \"1\"\n{extra}"
    ))
}

#[test]
fn schemes_are_the_url_types_both_platforms_open_the_game_with() {
    let config = linked("schemes = [\"com.studio.game\", \"tide\"]\n");
    config.check(Platform::Ios).expect("two schemes");
    for platform in [Platform::Ios, Platform::Macos] {
        let text = config.info_plist(platform, "game", "").unwrap();
        assert!(
            text.contains(
                "<key>CFBundleURLTypes</key>\n  <array><dict><key>CFBundleURLName</key>\
                 <string>com.studio.game</string><key>CFBundleURLSchemes</key><array>\
                 <string>com.studio.game</string><string>tide</string></array></dict></array>"
            ),
            "{text}"
        );
    }
    let none = linked("").info_plist(Platform::Ios, "game", "").unwrap();
    assert!(!none.contains("CFBundleURLTypes"), "{none}");
}

#[test]
fn a_scheme_no_url_type_names_is_refused_by_androids_rule() {
    for bad in ["Tide", "1tide", "ti de", "https", ""] {
        let err = linked(&format!("schemes = [\"{bad}\"]\n"))
            .check(Platform::Ios)
            .unwrap_err()
            .to_string();
        assert!(err.contains("[apple] schemes"), "{bad}: {err}");
    }
}

#[test]
fn associated_domains_are_an_entitlement_a_profile_must_grant() {
    let config = linked(
        "associated_domains = [\"applinks:game.studio.com\", \"webcredentials:studio.com?mode=developer\"]\n",
    );
    config.check(Platform::Macos).expect("two domains");
    let text = config
        .entitlements(Platform::Ios, Build::Development, false, false)
        .expect("the domains write an entitlement");
    assert!(
        text.contains(
            "<key>com.apple.developer.associated-domains</key>\n  <array>\
             <string>applinks:game.studio.com</string>\
             <string>webcredentials:studio.com?mode=developer</string></array>"
        ),
        "{text}"
    );
    assert_eq!(
        config.restricted(Platform::Macos),
        [(
            "associated_domains",
            "com.apple.developer.associated-domains"
        )]
    );
    let err = config
        .needs_profile(Platform::Macos)
        .unwrap_err()
        .to_string();
    assert!(err.contains("associated_domains"), "{err}");
    for bad in [
        "game.studio.com",
        "links:game.studio.com",
        "applinks:",
        "applinks:https://x",
    ] {
        let err = linked(&format!("associated_domains = [\"{bad}\"]\n"))
            .check(Platform::Ios)
            .unwrap_err()
            .to_string();
        assert!(err.contains("[apple] associated_domains"), "{bad}: {err}");
    }
}

fn store(extra: &str) -> AppleConfig {
    AppleConfig {
        app_store: true,
        ..config(&format!(
            "[apple]\nbundle_id = \"com.studio.game\"\nversion = \"1.0\"\nbuild_number = \"1\"\n{extra}"
        ))
    }
}

// The store requires its sandbox and checks the identifiers Xcode writes;
// the network is how a game reaches its server; the hardened runtime's
// exceptions are a download's, never asked for here.
#[test]
fn a_mac_app_store_build_is_sandboxed_and_names_its_team() {
    let config = store(
        "team_id = \"AB12CD34EF\"\ncategory = \"public.app-category.education\"\n\
         capabilities = [\"sign-in-with-apple\", \"in-app-purchase\"]\n",
    );
    config.check(Platform::Macos).expect("a complete table");
    let text = config
        .entitlements(Platform::Macos, Build::Distribution, false, false)
        .unwrap();
    for key in [
        "<key>com.apple.security.app-sandbox</key>\n  <true/>",
        "<key>com.apple.security.network.client</key>\n  <true/>",
        "<key>com.apple.application-identifier</key>\n  <string>AB12CD34EF.com.studio.game</string>",
        "<key>com.apple.developer.team-identifier</key>\n  <string>AB12CD34EF</string>",
        "com.apple.developer.applesignin",
    ] {
        assert!(text.contains(key), "{key}: {text}");
    }
    assert!(!text.contains("com.apple.security.cs."), "{text}");
    let ios = config
        .entitlements(Platform::Ios, Build::Distribution, false, false)
        .unwrap();
    assert!(!ios.contains("app-sandbox"), "{ios}");
}

#[test]
fn a_mac_app_store_build_needs_a_category_and_a_team() {
    let err = store("team_id = \"AB12CD34EF\"\n")
        .check(Platform::Macos)
        .unwrap_err()
        .to_string();
    assert!(err.contains("[apple] category"), "{err}");
    let err = store("category = \"public.app-category.games\"\n")
        .check(Platform::Macos)
        .unwrap_err()
        .to_string();
    assert!(err.contains("[apple] team_id"), "{err}");
}

#[test]
fn the_installer_identity_is_named_or_the_application_ones_team() {
    let config = store("");
    assert_eq!(
        config
            .installer_identity(None, "Apple Distribution: Studio (AB12CD34EF)")
            .unwrap(),
        "3rd Party Mac Developer Installer: Studio (AB12CD34EF)"
    );
    assert_eq!(
        config
            .installer_identity(None, "Developer ID Application: Studio (AB12CD34EF)")
            .unwrap(),
        "Developer ID Installer: Studio (AB12CD34EF)"
    );
    assert_eq!(
        config
            .installer_identity(Some("Mine"), "Apple Distribution: Studio")
            .unwrap(),
        "Mine"
    );
    let named = store("macos_installer_identity = \"3rd Party Mac Developer Installer: X\"\n");
    assert_eq!(
        named.installer_identity(None, "ABCDEF0123").unwrap(),
        "3rd Party Mac Developer Installer: X"
    );
    let err = config
        .installer_identity(None, "ABCDEF0123")
        .unwrap_err()
        .to_string();
    assert!(err.contains("--installer-identity"), "{err}");
}
