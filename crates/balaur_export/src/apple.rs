//! What `[apple]` in `project.toml` puts into an exported bundle: the
//! identifier the App Store resolves a game against, the `Info.plist` keys,
//! and the entitlement each capability needs.
//!
//! Signing stays the developer's, as docs/PLAN-mobile-export.md decided. What
//! the exporter owes them is a bundle that is *signable*: an entitlement
//! Xcode would have written is one no `codesign` invocation can add later
//! without knowing what the game asked for.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

/// Which bundle a plist is being written for. The keys differ, and so do the
/// version numbers a capability needs.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Platform {
    Ios,
    Macos,
}

/// A capability the game asks for, in the spelling a project writes.
///
/// The set is closed on purpose: an entitlement this exporter does not
/// understand is one it cannot check, and a misspelled one fails at the
/// player's first launch rather than at export.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Capability {
    /// Sign in with Apple: `com.apple.developer.applesignin`.
    Applesignin,
    /// Game Center: `com.apple.developer.game-center`.
    GameCenter,
    /// The iCloud key-value store a cloud save syncs through.
    IcloudKv,
    /// StoreKit. It carries no entitlement of its own — what it declares is
    /// the deployment target StoreKit 2 exists on.
    InAppPurchase,
}

impl Capability {
    const fn name(self) -> &'static str {
        match self {
            Self::Applesignin => "applesignin",
            Self::GameCenter => "game-center",
            Self::IcloudKv => "icloud-kv",
            Self::InAppPurchase => "in-app-purchase",
        }
    }

    /// The lowest OS this capability's API exists on, per platform. Game
    /// Center predates both, but its identity signature — the only way a
    /// server can check a player — arrived in iOS 13.5.
    const fn minimum(self, platform: Platform) -> (u32, u32) {
        match (self, platform) {
            (Self::Applesignin, Platform::Ios) => (13, 0),
            (Self::GameCenter, Platform::Ios) => (13, 5),
            (Self::Applesignin | Self::GameCenter, Platform::Macos) => (10, 15),
            (Self::IcloudKv, Platform::Ios) => (5, 0),
            (Self::IcloudKv, Platform::Macos) => (10, 7),
            (Self::InAppPurchase, Platform::Ios) => (15, 0),
            (Self::InAppPurchase, Platform::Macos) => (12, 0),
        }
    }
}

/// A value an `[apple.plist]` entry may take, and everything a plist can
/// hold that a game is likely to write by hand.
#[derive(Clone, Debug, serde::Deserialize)]
#[serde(untagged)]
pub(crate) enum PlistValue {
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    List(Vec<PlistValue>),
}

/// The `[apple]` table:
///
/// ```toml
/// [apple]
/// bundle_id = "com.studio.game"
/// team = "AB12CD34EF"
/// min_os = "15.0"
/// capabilities = ["applesignin", "game-center", "icloud-kv"]
///
/// [apple.plist]
/// ITSAppUsesNonExemptEncryption = false
/// ```
#[derive(Clone, Debug, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct AppleConfig {
    /// The identifier registered in App Store Connect. Empty means the game
    /// declares none, and the exporter keeps its old `org.balaur.<name>`.
    pub bundle_id: String,
    /// The ten-character team identifier. Xcode expands
    /// `$(TeamIdentifierPrefix)` from it; nothing expands anything here, so
    /// an entitlement that carries the prefix needs the real value.
    pub team: String,
    pub display_name: String,
    pub version: String,
    pub build: String,
    /// `MinimumOSVersion` on iOS.
    pub min_os: String,
    /// `LSMinimumSystemVersion` on macOS.
    pub min_macos: String,
    /// macOS only: `LSApplicationCategoryType`.
    pub category: String,
    pub capabilities: Vec<Capability>,
    /// Keys merged into `Info.plist` as written. The exporter writes the
    /// plist whole, so a key it does not know goes here rather than into the
    /// template.
    pub plist: BTreeMap<String, PlistValue>,
}

impl Default for AppleConfig {
    fn default() -> Self {
        Self {
            bundle_id: String::new(),
            team: String::new(),
            display_name: String::new(),
            version: "1.0".into(),
            build: "1".into(),
            // The templates are built for these (scripts/package_template.sh,
            // scripts/package.sh), which is where StoreKit 2 starts; a plist
            // may not claim less than the binary was built for.
            min_os: "15.0".into(),
            min_macos: "12.0".into(),
            category: String::new(),
            capabilities: Vec::new(),
            plist: BTreeMap::new(),
        }
    }
}

impl AppleConfig {
    /// The `[apple]` table of a project, or the defaults when there is none.
    ///
    /// A table that does not parse is an error rather than a warning: a
    /// misspelled capability or identifier is not something to ship past.
    pub(crate) fn load(project: &Path) -> Result<Self> {
        #[derive(serde::Deserialize)]
        struct Manifest {
            #[serde(default)]
            apple: AppleConfig,
        }
        let path = project.join("project.toml");
        let Ok(source) = std::fs::read_to_string(&path) else {
            return Ok(Self::default());
        };
        let manifest: Manifest = toml::from_str(&source)
            .with_context(|| format!("parsing [apple] in {}", path.display()))?;
        Ok(manifest.apple)
    }

    /// The identifier this bundle ships with: the project's, or the old
    /// invented one for a game that declares none.
    pub(crate) fn identifier(&self, name: &str) -> String {
        if self.bundle_id.is_empty() {
            let id: String = name
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
                .collect();
            format!("org.balaur.{id}")
        } else {
            self.bundle_id.clone()
        }
    }

    /// Refuse an export that would produce a bundle no capability can work
    /// in, naming what to fix.
    pub(crate) fn check(&self, platform: Platform) -> Result<()> {
        if self.capabilities.is_empty() {
            return Ok(());
        }
        if self.bundle_id.is_empty() {
            bail!(
                "[apple] capabilities need `bundle_id`: {} resolves against the identifier \
                 registered in App Store Connect, and the exporter's invented one is owned \
                 by no account",
                self.capabilities[0].name()
            );
        }
        if !self
            .bundle_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
        {
            bail!(
                "[apple] bundle_id \"{}\" is not a bundle identifier: letters, digits, \
                 dots and hyphens only",
                self.bundle_id
            );
        }
        let declared = self.deployment(platform);
        let version = parse_version(declared).with_context(|| {
            format!(
                "[apple] {} = \"{declared}\" is not a version",
                match platform {
                    Platform::Ios => "min_os",
                    Platform::Macos => "min_macos",
                }
            )
        })?;
        for capability in &self.capabilities {
            let minimum = capability.minimum(platform);
            if version < minimum {
                bail!(
                    "[apple] {} needs {}.{} or later, and the project declares {declared}",
                    capability.name(),
                    minimum.0,
                    minimum.1,
                );
            }
        }
        if self.capabilities.contains(&Capability::IcloudKv) && self.team.is_empty() {
            bail!(
                "[apple] icloud-kv needs `team`: the key-value store identifier is \
                 <team>.<bundle_id>, and nothing expands $(TeamIdentifierPrefix) outside Xcode"
            );
        }
        Ok(())
    }

    fn deployment(&self, platform: Platform) -> &str {
        match platform {
            Platform::Ios => &self.min_os,
            Platform::Macos => &self.min_macos,
        }
    }

    /// The entitlements this game's capabilities need, or `None` when it
    /// declares none and there is nothing to sign against.
    pub(crate) fn entitlements(&self) -> Option<String> {
        if self.capabilities.is_empty() {
            return None;
        }
        let mut body = String::new();
        for capability in &self.capabilities {
            match capability {
                Capability::Applesignin => body.push_str(
                    "  <key>com.apple.developer.applesignin</key>\n  \
                     <array><string>Default</string></array>\n",
                ),
                Capability::GameCenter => {
                    body.push_str("  <key>com.apple.developer.game-center</key>\n  <true/>\n");
                }
                Capability::IcloudKv => {
                    let _ = write!(
                        body,
                        "  <key>com.apple.developer.ubiquity-kvstore-identifier</key>\n  \
                         <string>{}.{}</string>\n",
                        self.team, self.bundle_id
                    );
                }
                // In-app purchase needs no entitlement; it is a capability
                // here so the deployment target is checked against StoreKit.
                Capability::InAppPurchase => {}
            }
        }
        // A game whose only capability writes no key needs no file.
        if body.is_empty() {
            return None;
        }
        Some(plist_document(&body))
    }

    /// The `Info.plist` for a bundle, written whole rather than patched: the
    /// template's is what the *template* runs with, and a key a project needs
    /// goes in `[apple.plist]`.
    ///
    /// `executable` is the binary's file name inside the bundle, which on iOS
    /// is still the template's; `name` is the game's.
    pub(crate) fn info_plist(&self, platform: Platform, executable: &str, name: &str) -> String {
        let display = if self.display_name.is_empty() {
            name
        } else {
            &self.display_name
        };
        let mut body = String::new();
        string_key(&mut body, "CFBundleExecutable", executable);
        string_key(&mut body, "CFBundleIdentifier", &self.identifier(name));
        string_key(&mut body, "CFBundleName", name);
        string_key(&mut body, "CFBundleDisplayName", display);
        string_key(&mut body, "CFBundlePackageType", "APPL");
        string_key(&mut body, "CFBundleShortVersionString", &self.version);
        string_key(&mut body, "CFBundleVersion", &self.build);
        match platform {
            Platform::Ios => {
                body.push_str("  <key>LSRequiresIPhoneOS</key><true/>\n");
                body.push_str("  <key>UILaunchScreen</key><dict/>\n");
                string_key(&mut body, "MinimumOSVersion", &self.min_os);
            }
            Platform::Macos => {
                string_key(&mut body, "LSMinimumSystemVersion", &self.min_macos);
                if !self.category.is_empty() {
                    string_key(&mut body, "LSApplicationCategoryType", &self.category);
                }
            }
        }
        for (k, v) in &self.plist {
            let _ = write!(body, "  <key>{}</key>", escape(k));
            body.push_str(&plist_value(v, 1));
            body.push('\n');
        }
        plist_document(&body)
    }

    /// Where the entitlements file goes and what to do with it, written next
    /// to a bundle this exporter is not allowed to sign.
    pub(crate) fn write_entitlements(&self, beside: &Path, name: &str) -> Result<Option<PathBuf>> {
        let Some(text) = self.entitlements() else {
            return Ok(None);
        };
        let path = beside.with_file_name(format!("{name}.entitlements"));
        std::fs::write(&path, text).with_context(|| format!("writing {}", path.display()))?;
        Ok(Some(path))
    }
}

/// `1`, `13.5` and `15.1.2` all parse; the third component never decides
/// anything here, so it is dropped.
fn parse_version(text: &str) -> Result<(u32, u32)> {
    let mut parts = text.split('.');
    let major = parts.next().unwrap_or_default().parse::<u32>()?;
    let minor = match parts.next() {
        Some(minor) => minor.parse::<u32>()?,
        None => 0,
    };
    Ok((major, minor))
}

fn string_key(body: &mut String, key: &str, value: &str) {
    let _ = writeln!(body, "  <key>{key}</key><string>{}</string>", escape(value));
}

fn plist_document(body: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
{body}</dict>
</plist>
"#
    )
}

fn plist_value(value: &PlistValue, depth: usize) -> String {
    let pad = "  ".repeat(depth);
    match value {
        PlistValue::Bool(true) => "<true/>".into(),
        PlistValue::Bool(false) => "<false/>".into(),
        PlistValue::Int(n) => format!("<integer>{n}</integer>"),
        PlistValue::Float(n) => format!("<real>{n}</real>"),
        PlistValue::Str(s) => format!("<string>{}</string>", escape(s)),
        PlistValue::List(items) => {
            let mut out = String::from("<array>\n");
            for item in items {
                let _ = writeln!(out, "{pad}  {}", plist_value(item, depth + 1));
            }
            out.push_str(&pad);
            out.push_str("</array>");
            out
        }
    }
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::{AppleConfig, Capability, Platform};

    fn config(table: &str) -> AppleConfig {
        #[derive(serde::Deserialize)]
        struct Manifest {
            #[serde(default)]
            apple: AppleConfig,
        }
        toml::from_str::<Manifest>(table)
            .expect("the table parses")
            .apple
    }

    #[test]
    fn a_project_with_no_apple_table_keeps_the_invented_identifier() {
        let config = config("name = \"hello\"\n");
        assert_eq!(config.identifier("hello"), "org.balaur.hello");
        assert!(config.entitlements().is_none());
        config.check(Platform::Ios).expect("nothing to check");
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
        let config =
            config("[apple]\nbundle_id = \"com.studio.game\"\ncapabilities = [\"icloud-kv\"]\n");
        let err = config
            .check(Platform::Ios)
            .expect_err("nothing expands $(TeamIdentifierPrefix) here")
            .to_string();
        assert!(err.contains("team"), "{err}");
    }

    #[test]
    fn game_center_below_the_version_its_identity_signature_needs_is_refused() {
        let config = config(
            "[apple]\nbundle_id = \"com.studio.game\"\nmin_os = \"13.0\"\n\
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
            "[apple]\nbundle_id = \"com.studio.game\"\nteam = \"AB12CD34EF\"\n\
             min_os = \"15.0\"\n\
             capabilities = [\"applesignin\", \"game-center\", \"icloud-kv\"]\n",
        );
        config.check(Platform::Ios).expect("a complete table");
        let text = config.entitlements().expect("three capabilities");
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
             version = \"2.1\"\nbuild = \"7\"\nmin_os = \"15.0\"\n\
             [apple.plist]\nITSAppUsesNonExemptEncryption = false\n\
             UISupportedInterfaceOrientations = [\"UIInterfaceOrientationLandscapeLeft\"]\n",
        );
        let text = config.info_plist(Platform::Ios, "Balaur", "game");
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
        let text = config.info_plist(Platform::Macos, "game", "game");
        assert!(text.contains("<key>LSMinimumSystemVersion</key><string>13.0</string>"));
        assert!(!text.contains("LSRequiresIPhoneOS"), "{text}");
    }

    #[test]
    fn in_app_purchase_writes_no_entitlement_and_still_checks_the_version() {
        let below = config(
            "[apple]\nbundle_id = \"com.studio.game\"\nmin_os = \"14.0\"\n\
             capabilities = [\"in-app-purchase\"]\n",
        );
        let err = below
            .check(Platform::Ios)
            .expect_err("StoreKit 2 starts at 15.0")
            .to_string();
        assert!(err.contains("15.0"), "{err}");

        let config = config(
            "[apple]\nbundle_id = \"com.studio.game\"\ncapabilities = [\"in-app-purchase\"]\n",
        );
        config.check(Platform::Ios).expect("the default is 15.0");
        assert!(
            config.entitlements().is_none(),
            "in-app purchase needs no entitlement of its own"
        );
    }

    #[test]
    fn a_capability_is_spelled_the_way_the_entitlement_is() {
        assert_eq!(Capability::GameCenter.name(), "game-center");
    }
}
