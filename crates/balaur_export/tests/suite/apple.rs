//! What `[apple]` writes into a bundle beyond its plist: the provisioning
//! profile a signed build embeds and is checked against.

use std::path::{Path, PathBuf};

use balaur_export::Options;

const IDENTITY: &str = "Developer ID Application: Studio (AB12CD34EF)";

fn project(apple: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        format!(
            "[application]\nname = \"t\"\n\n[apple]\nbundle_id = \"com.studio.t\"\n\
             team_id = \"AB12CD34EF\"\nversion = \"1.0\"\nbuild_number = \"1\"\n{apple}"
        ),
    )
    .unwrap();
    dir
}

/// A provisioning profile granting `entitlements`: its plist, inside bytes
/// standing for the CMS envelope Apple's portal wraps it in.
fn profile(dir: &Path, entitlements: &[(&str, &str)]) -> PathBuf {
    let mut granted = String::new();
    for (key, value) in entitlements {
        granted.push_str(&format!("    <key>{key}</key>{value}\n"));
    }
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Entitlements</key>
  <dict>
    <key>com.apple.application-identifier</key><string>AB12CD34EF.com.studio.t</string>
{granted}  </dict>
  <key>Name</key><string>t</string>
</dict>
</plist>"#
    );
    let path = dir.join("t.provisionprofile");
    std::fs::write(
        &path,
        [&[0x30, 0x80, 0x06, 0x09][..], plist.as_bytes(), &[0, 0]].concat(),
    )
    .unwrap();
    path
}

fn mac_runtime(dir: &Path) -> PathBuf {
    let path = dir.join("balaur-runtime-macos-universal");
    crate::mac::runtime(&path);
    path
}

fn export_mac(dir: &Path, sign: Option<&str>, profile: Option<PathBuf>) -> anyhow::Result<PathBuf> {
    let app = dir.join("out").join("t.app");
    balaur_export::export(&Options {
        path: dir.to_path_buf(),
        output: Some(app.clone()),
        target: Some("macos-universal".into()),
        runtime: Some(mac_runtime(dir)),
        app: true,
        sign: sign.map(str::to_string),
        provisioning_profile: profile,
        ..Options::default()
    })?;
    Ok(app)
}

/// An iOS template: its plist, and a file standing for the executable.
fn ios_runtime(dir: &Path) -> PathBuf {
    let app = dir.join("Balaur.app");
    std::fs::create_dir_all(&app).unwrap();
    std::fs::write(
        app.join("Info.plist"),
        "<plist><dict><key>CFBundleExecutable</key><string>Balaur</string>\
         <key>MinimumOSVersion</key><string>15.0</string></dict></plist>",
    )
    .unwrap();
    std::fs::write(app.join("Balaur"), b"\0runtime\0").unwrap();
    app
}

fn export_ios(dir: &Path, profile: Option<PathBuf>) -> anyhow::Result<PathBuf> {
    let app = dir.join("out").join("t.app");
    balaur_export::export(&Options {
        path: dir.to_path_buf(),
        output: Some(app.clone()),
        target: Some("ios".into()),
        runtime: Some(ios_runtime(dir)),
        provisioning_profile: profile,
        ..Options::default()
    })?;
    Ok(app)
}

/// The entitlements an export wrote beside its bundle, named for the project.
fn entitlements_in(dir: &Path) -> PathBuf {
    std::fs::read_dir(dir.join("out"))
        .unwrap()
        .flatten()
        .map(|entry| entry.path())
        .find(|path| path.extension().is_some_and(|e| e == "entitlements"))
        .expect("an entitlements file beside the bundle")
}

#[test]
fn a_signed_mac_build_whose_capabilities_need_a_profile_is_refused_without_one() {
    let dir = project("capabilities = [\"sign-in-with-apple\", \"push-notifications\"]\n");
    let why = export_mac(dir.path(), Some(IDENTITY), None).unwrap_err();
    let why = format!("{why:#}");
    assert!(
        why.contains("sign-in-with-apple, push-notifications")
            && why.contains("macos_provisioning_profile"),
        "{why}"
    );
}

// iOS refuses a profile short of the capabilities; a Mac leaves out what its
// profile lacks, as a Developer ID profile lacks Sign in with Apple.
#[test]
fn a_profile_short_of_a_capability_is_refused_on_ios_and_narrows_a_mac() {
    let ios = project("capabilities = [\"game-center\"]\n");
    let granted = profile(ios.path(), &[]);
    let why = format!("{:#}", export_ios(ios.path(), Some(granted)).unwrap_err());
    assert!(
        why.contains("game-center (com.apple.developer.game-center)"),
        "{why}"
    );
    let mac = project("capabilities = [\"sign-in-with-apple\", \"push-notifications\"]\n");
    let granted = profile(
        mac.path(),
        &[(
            "com.apple.developer.aps-environment",
            "<string>production</string>",
        )],
    );
    export_mac(mac.path(), None, Some(granted)).unwrap();
    let entitlements = std::fs::read_to_string(entitlements_in(mac.path())).unwrap();
    assert!(
        entitlements.contains("com.apple.developer.aps-environment")
            && !entitlements.contains("applesignin"),
        "{entitlements}"
    );
}

#[test]
fn a_mac_bundle_embeds_its_profile_and_claims_the_push_environment_it_grants() {
    let dir = project(&format!(
        "capabilities = [\"push-notifications\"]\nmacos_provisioning_profile = \"t.provisionprofile\"\n"
    ));
    profile(
        dir.path(),
        &[(
            "com.apple.developer.aps-environment",
            "<string>production</string>",
        )],
    );
    // Unsigned, as a build the signer finishes is: the profile still goes in.
    let app = export_mac(dir.path(), None, None).unwrap();
    let embedded = app.join("Contents/embedded.provisionprofile");
    assert_eq!(
        std::fs::read(&embedded).unwrap(),
        std::fs::read(dir.path().join("t.provisionprofile")).unwrap()
    );
    let entitlements = std::fs::read_to_string(entitlements_in(dir.path())).unwrap();
    assert!(
        entitlements.contains(
            "<key>com.apple.developer.aps-environment</key>\n  <string>production</string>"
        ),
        "the profile's environment, not the identity's guess: {entitlements}"
    );
    let beside: Vec<_> = std::fs::read_dir(dir.path().join("out"))
        .unwrap()
        .flatten()
        .collect();
    assert!(
        !beside.iter().any(|entry| entry
            .file_name()
            .to_string_lossy()
            .ends_with(".ad-hoc.entitlements")),
        "the ad-hoc file is the signature's alone"
    );
}

// A dev build with no identity exports without a profile, and the bundle
// still launches: its ad-hoc signature claims no entitlement only a profile
// grants. The file beside it keeps them, for a signer that comes later.
#[test]
fn an_ad_hoc_mac_build_needs_no_profile_and_writes_the_capabilities_beside_it() {
    let dir = project("capabilities = [\"sign-in-with-apple\"]\n");
    let app = export_mac(dir.path(), None, None).unwrap();
    assert!(!app.join("Contents/embedded.provisionprofile").exists());
    let entitlements = std::fs::read_to_string(entitlements_in(dir.path())).unwrap();
    assert!(
        entitlements.contains("com.apple.developer.applesignin"),
        "{entitlements}"
    );
    if cfg!(target_os = "macos") {
        let signed = std::process::Command::new("codesign")
            .args(["-d", "--entitlements", "-", "--xml"])
            .arg(&app)
            .output()
            .unwrap();
        let claimed = String::from_utf8_lossy(&signed.stdout);
        assert!(!claimed.contains("applesignin"), "{claimed}");
    }
}

#[test]
fn an_ios_bundle_embeds_the_profile_it_is_checked_against() {
    let dir = project("capabilities = [\"sign-in-with-apple\"]\n");
    let granted = profile(
        dir.path(),
        &[(
            "com.apple.developer.applesignin",
            "<array><string>Default</string></array>",
        )],
    );
    let app = export_ios(dir.path(), Some(granted.clone())).unwrap();
    assert_eq!(
        std::fs::read(app.join("embedded.mobileprovision")).unwrap(),
        std::fs::read(granted).unwrap()
    );
}

#[test]
fn an_ios_build_under_the_templates_minimum_is_refused() {
    let dir = project("min_ios = \"14.0\"\n");
    let why = format!("{:#}", export_ios(dir.path(), None).unwrap_err());
    assert!(
        why.contains("min_ios = \"14.0\"") && why.contains("15.0"),
        "{why}"
    );
    export_ios(project("min_ios = \"15.0\"\n").path(), None).expect("the template's own");
}

// A flat runtime carries no plist: the floor is what its binary was built for.
#[cfg(target_os = "macos")]
#[test]
fn a_mac_build_under_its_binarys_minimum_is_refused() {
    let dir = project("min_macos = \"11.0\"\n");
    let why = format!("{:#}", export_mac(dir.path(), None, None).unwrap_err());
    assert!(
        why.contains("min_macos = \"11.0\"") && why.contains("12.0"),
        "{why}"
    );
}

// Associated domains are granted by a profile alone, as a capability is: a
// profile made before the App ID had them fails the export.
#[test]
fn associated_domains_need_a_profile_that_grants_them() {
    let dir = project("associated_domains = [\"applinks:t.studio.com\"]\n");
    let why = format!(
        "{:#}",
        export_mac(dir.path(), Some(IDENTITY), None).unwrap_err()
    );
    assert!(why.contains("associated_domains"), "{why}");
    let granted = profile(dir.path(), &[]);
    let why = format!("{:#}", export_ios(dir.path(), Some(granted)).unwrap_err());
    assert!(
        why.contains("associated_domains (com.apple.developer.associated-domains)"),
        "{why}"
    );
    let granted = profile(
        dir.path(),
        &[(
            "com.apple.developer.associated-domains",
            "<string>*</string>",
        )],
    );
    let app = export_ios(dir.path(), Some(granted)).unwrap();
    assert!(app.join("embedded.mobileprovision").is_file());
}

#[test]
fn a_scheme_reaches_the_bundles_plist() {
    let dir = project("schemes = [\"com.studio.t\"]\n");
    let app = export_ios(dir.path(), None).unwrap();
    let plist = std::fs::read_to_string(app.join("Info.plist")).unwrap();
    assert!(
        plist.contains("<key>CFBundleURLSchemes</key><array><string>com.studio.t</string></array>"),
        "{plist}"
    );
}

const STORE: &str = "category = \"public.app-category.education\"\n";

fn export_store(
    dir: &Path,
    sign: Option<&str>,
    profile: Option<PathBuf>,
) -> anyhow::Result<PathBuf> {
    let app = dir.join("out").join("t.app");
    balaur_export::export(&Options {
        path: dir.to_path_buf(),
        output: Some(app.clone()),
        target: Some("macos-universal".into()),
        runtime: Some(mac_runtime(dir)),
        app_store: true,
        sign: sign.map(str::to_string),
        provisioning_profile: profile,
        ..Options::default()
    })?;
    Ok(app)
}

#[test]
fn a_mac_app_store_build_is_refused_without_its_profile_or_its_category() {
    let dir = project(STORE);
    let why = format!("{:#}", export_store(dir.path(), None, None).unwrap_err());
    assert!(why.contains("Mac App Store provisioning profile"), "{why}");
    let uncategorized = project("");
    let granted = profile(uncategorized.path(), &[]);
    let why = format!(
        "{:#}",
        export_store(uncategorized.path(), None, Some(granted)).unwrap_err()
    );
    assert!(why.contains("[apple] category"), "{why}");
}

// An identity that names no installer of its own (a hash) has nothing to
// sign the .pkg with, and fails before anything is signed.
#[test]
fn a_mac_app_store_build_is_refused_without_an_installer_identity() {
    let dir = project(STORE);
    let granted = profile(dir.path(), &[]);
    let why = format!(
        "{:#}",
        export_store(dir.path(), Some("0123456789ABCDEF"), Some(granted)).unwrap_err()
    );
    assert!(why.contains("--installer-identity"), "{why}");
}

// With no identity on hand the build is still the store's: sandboxed, its
// profile inside, the capabilities the profile grants beside it for a
// signer, and on a Mac an unsigned .pkg beside the .app.
#[test]
fn a_mac_app_store_build_is_sandboxed_and_packaged() {
    let dir = project(&format!(
        "{STORE}capabilities = [\"sign-in-with-apple\", \"in-app-purchase\", \"push-notifications\"]\n"
    ));
    let granted = profile(
        dir.path(),
        &[
            (
                "com.apple.developer.applesignin",
                "<array><string>Default</string></array>",
            ),
            (
                "com.apple.developer.aps-environment",
                "<string>production</string>",
            ),
        ],
    );
    let app = export_store(dir.path(), None, Some(granted)).unwrap();
    assert!(app.join("Contents/embedded.provisionprofile").is_file());
    let entitlements = std::fs::read_to_string(entitlements_in(dir.path())).unwrap();
    for key in [
        "com.apple.security.app-sandbox",
        "com.apple.security.network.client",
        "<string>AB12CD34EF.com.studio.t</string>",
        "com.apple.developer.applesignin",
        "<key>com.apple.developer.aps-environment</key>\n  <string>production</string>",
    ] {
        assert!(entitlements.contains(key), "{key}: {entitlements}");
    }
    assert!(
        !entitlements.contains("com.apple.security.cs."),
        "no hardened-runtime exception: {entitlements}"
    );
    let plist = std::fs::read_to_string(app.join("Contents/Info.plist")).unwrap();
    assert!(
        plist.contains(
            "<key>LSApplicationCategoryType</key><string>public.app-category.education</string>"
        ),
        "{plist}"
    );
    if cfg!(target_os = "macos") {
        assert!(dir.path().join("out/t.pkg").is_file());
    }
}

#[test]
fn a_mac_app_store_build_is_neither_notarized_nor_a_steam_one() {
    let dir = project(&format!("{STORE}notarize = true\n"));
    let granted = profile(dir.path(), &[]);
    let why = format!(
        "{:#}",
        export_store(dir.path(), None, Some(granted)).unwrap_err()
    );
    assert!(why.contains("notarized"), "{why}");
}
