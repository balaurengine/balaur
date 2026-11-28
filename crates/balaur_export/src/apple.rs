//! What `[apple]` in `project.toml` puts into an exported bundle: the
//! identifier the App Store resolves a game against, the `Info.plist` keys,
//! and the entitlement each capability needs.
//!
//! `sign.rs` signs the bundle; this module makes it *signable*: an
//! entitlement Xcode would have written is one no `codesign` invocation can
//! add later without knowing what the game asked for.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::profile::Profile;

pub(crate) mod store;

pub(crate) use store::IosTemplate;

/// The entitlements a capability writes. Each is one Apple grants only
/// through a provisioning profile, so each is also what a profile is checked
/// for.
const APPLESIGNIN: &str = "com.apple.developer.applesignin";
const GAME_CENTER: &str = "com.apple.developer.game-center";
const ICLOUD_KV: &str = "com.apple.developer.ubiquity-kvstore-identifier";
/// Push's, which iOS spells without the prefix a Mac's carries.
const APS_IOS: &str = "aps-environment";
const APS_MACOS: &str = "com.apple.developer.aps-environment";
/// The domains a universal link or a saved password is matched against.
const ASSOCIATED_DOMAINS: &str = "com.apple.developer.associated-domains";
/// The `[apple]` key naming them, which a refusal names as it would a
/// capability.
const ASSOCIATED_DOMAINS_KEY: &str = "associated_domains";
/// The services an associated domain is declared for, as Apple spells them.
const DOMAIN_SERVICES: [&str; 4] = [
    "applinks",
    "webcredentials",
    "activitycontinuation",
    "appclips",
];
/// The web's own schemes, which never name the game.
const WEB_SCHEMES: [&str; 2] = ["http", "https"];
/// The plist key a project may write its URL types under itself.
const URL_TYPES: &str = "CFBundleURLTypes";
/// What a Mac App Store build claims beside its capabilities: the sandbox
/// the store requires, the network a game's server is reached over, and the
/// identifiers Xcode writes from the team and the bundle id.
const APP_SANDBOX: &str = "com.apple.security.app-sandbox";
const NETWORK_CLIENT: &str = "com.apple.security.network.client";
use store::{APPLICATION_IDENTIFIER, TEAM_IDENTIFIER};
/// The values `aps-environment` takes.
const APS_DEVELOPMENT: &str = "development";
const APS_PRODUCTION: &str = "production";

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
    #[serde(rename = "sign-in-with-apple")]
    Applesignin,
    /// Game Center: `com.apple.developer.game-center`.
    GameCenter,
    /// The iCloud key-value store a cloud save syncs through.
    IcloudKv,
    /// StoreKit. It carries no entitlement of its own — what it declares is
    /// the deployment target StoreKit 2 exists on.
    InAppPurchase,
    /// APNs, behind `platform.register_for_push`: `aps-environment`, which
    /// macOS spells `com.apple.developer.aps-environment`.
    PushNotifications,
}

impl Capability {
    const fn name(self) -> &'static str {
        match self {
            Self::Applesignin => "sign-in-with-apple",
            Self::GameCenter => "game-center",
            Self::IcloudKv => "icloud-kv",
            Self::InAppPurchase => "in-app-purchase",
            Self::PushNotifications => "push-notifications",
        }
    }

    /// The entitlement this capability writes on `platform`, which only a
    /// provisioning profile grants; `None` for one that writes none.
    const fn entitlement(self, platform: Platform) -> Option<&'static str> {
        match (self, platform) {
            (Self::Applesignin, _) => Some(APPLESIGNIN),
            (Self::GameCenter, _) => Some(GAME_CENTER),
            (Self::IcloudKv, _) => Some(ICLOUD_KV),
            (Self::InAppPurchase, _) => None,
            (Self::PushNotifications, Platform::Ios) => Some(APS_IOS),
            (Self::PushNotifications, Platform::Macos) => Some(APS_MACOS),
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
            // UserNotifications, which shows a push in front and hears its tap.
            (Self::PushNotifications, Platform::Ios) => (10, 0),
            (Self::PushNotifications, Platform::Macos) => (10, 14),
        }
    }
}

/// A device family an iOS build runs on, as `UIDeviceFamily` numbers it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Device {
    Iphone,
    Ipad,
}

impl Device {
    /// The number `UIDeviceFamily` names it by.
    pub(crate) const fn family(self) -> u8 {
        match self {
            Self::Iphone => 1,
            Self::Ipad => 2,
        }
    }
}

/// Which APNs a signed build's pushes come from, as `aps-environment` names
/// it: the identity's own kind says, as Xcode's provisioning profile does,
/// since a development certificate signs only for the sandbox.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Build {
    /// Signed by an `Apple Development`, `iPhone Developer` or `Mac
    /// Developer` identity, ad hoc, or not at all: APNs' sandbox.
    Development,
    /// Any other identity: `Apple Distribution`, `Developer ID
    /// Application`, `3rd Party Mac Developer Application`, or a hash.
    Distribution,
}

impl Build {
    /// What `identity`, the one an export signs with, is for.
    pub(crate) fn of(identity: Option<&str>) -> Self {
        const DEVELOPMENT: [&str; 4] = [
            "Apple Development",
            "iPhone Developer",
            "Mac Developer",
            "-",
        ];
        match identity {
            None => Self::Development,
            Some(name) if DEVELOPMENT.iter().any(|kind| name.starts_with(kind)) => {
                Self::Development
            }
            Some(_) => Self::Distribution,
        }
    }

    /// What a provisioning profile says, which is what the bundle must
    /// claim: its `aps-environment` is the one APNs and the OS both check.
    /// `None` for a profile that grants no push.
    pub(crate) fn granted(profile: &Profile, platform: Platform) -> Option<Self> {
        let key = Capability::PushNotifications.entitlement(platform)?;
        match profile.value(key)? {
            APS_DEVELOPMENT => Some(Self::Development),
            APS_PRODUCTION => Some(Self::Distribution),
            _ => None,
        }
    }

    /// The value `aps-environment` takes.
    const fn aps(self) -> &'static str {
        match self {
            Self::Development => APS_DEVELOPMENT,
            Self::Distribution => APS_PRODUCTION,
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
/// team_id = "AB12CD34EF"
/// min_ios = "15.0"
/// devices = ["iphone", "ipad"]
/// capabilities = ["sign-in-with-apple", "game-center", "icloud-kv"]
/// schemes = ["com.studio.game"]
/// associated_domains = ["applinks:game.studio.com"]
///
/// [apple.plist]
/// ITSAppUsesNonExemptEncryption = false
/// ```
#[derive(Clone, Debug, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct AppleConfig {
    /// The identifier registered in App Store Connect. Required: an export
    /// refuses to invent one.
    pub bundle_id: String,
    /// The ten-character team identifier. Xcode expands
    /// `$(TeamIdentifierPrefix)` from it; nothing expands anything here, so
    /// an entitlement that carries the prefix needs the real value.
    pub team_id: String,
    /// The name under the icon; `[application] name` when empty.
    pub display_name: String,
    /// Required, as is `build_number`.
    pub version: String,
    pub build_number: String,
    /// `MinimumOSVersion` on iOS.
    pub min_ios: String,
    /// `LSMinimumSystemVersion` on macOS.
    pub min_macos: String,
    /// `UIDeviceFamily`: the devices an iOS build installs on.
    pub devices: Vec<Device>,
    /// macOS only: `LSApplicationCategoryType`.
    pub category: String,
    pub capabilities: Vec<Capability>,
    /// The custom URL schemes that open the game, as `CFBundleURLTypes`:
    /// what `[android] schemes` is on Android, and the same rule for a name.
    pub schemes: Vec<String>,
    /// `com.apple.developer.associated-domains`, each as Apple writes it,
    /// `applinks:game.studio.com`: the domains whose universal links open
    /// the game. Granted by a provisioning profile only, as a capability is.
    pub associated_domains: Vec<String>,
    /// `Developer ID Application: …` for a download, `Apple Distribution: …`
    /// for the Mac App Store.
    pub macos_identity: String,
    /// The identity a Mac App Store `.pkg` is signed with: `3rd Party Mac
    /// Developer Installer: …`. Empty derives it from an `Apple
    /// Distribution` or `3rd Party Mac Developer Application` identity.
    pub macos_installer_identity: String,
    /// Submit to Apple's notary service after signing, and staple the ticket.
    pub notarize: bool,
    /// A project-relative `.provisionprofile`, embedded as
    /// `Contents/embedded.provisionprofile`: the Developer ID or Mac App
    /// Store profile `macos_identity` goes with. A signed Mac build whose
    /// capabilities write an entitlement needs one.
    pub macos_provisioning_profile: String,
    pub ios_identity: String,
    /// A project-relative `.mobileprovision`, copied into the bundle.
    pub ios_provisioning_profile: String,
    /// Keys merged into `Info.plist` as written. The exporter writes the
    /// plist whole, so a key it does not know goes here rather than into the
    /// template.
    pub plist: BTreeMap<String, PlistValue>,
    /// `[window] orientation`, which iOS settles in the plist rather than at
    /// startup. Not a key of `[apple]`: it is the same window setting every
    /// platform reads, taken from the same resolved document.
    #[serde(skip)]
    pub(crate) orientation: balaur::project::Orientation,
    /// `[application] name`, which `display_name` overrides. Not a key of
    /// `[apple]` either.
    #[serde(skip)]
    pub(crate) title: String,
    /// A Mac App Store build (`--app-store`): sandboxed, with the
    /// identifiers the store checks, and none of the hardened runtime's
    /// exceptions. Not a key of `[apple]`: the same project is both builds.
    #[serde(skip)]
    pub(crate) app_store: bool,
    /// The entitlements naming the app that a signature carries, as the
    /// provisioning profile grants them ([`Self::signed_against`]). Not a key
    /// of `[apple]`: the profile is what decides them.
    #[serde(skip)]
    pub(crate) identity: Vec<(&'static str, crate::plist::Value)>,
}

impl Default for AppleConfig {
    fn default() -> Self {
        Self {
            bundle_id: String::new(),
            team_id: String::new(),
            display_name: String::new(),
            version: String::new(),
            build_number: String::new(),
            // The templates are built for these (scripts/package_runtime.sh,
            // scripts/package.sh), which is where StoreKit 2 starts; a plist
            // may not claim less than the binary was built for.
            min_ios: "15.0".into(),
            min_macos: "12.0".into(),
            // Every device the runtime draws on: it fits any screen, and the
            // icon carries an iPad's.
            devices: vec![Device::Iphone, Device::Ipad],
            category: String::new(),
            capabilities: Vec::new(),
            schemes: Vec::new(),
            associated_domains: Vec::new(),
            macos_identity: String::new(),
            macos_installer_identity: String::new(),
            notarize: false,
            macos_provisioning_profile: String::new(),
            ios_identity: String::new(),
            ios_provisioning_profile: String::new(),
            plist: BTreeMap::new(),
            orientation: balaur::project::Orientation::Any,
            title: String::new(),
            app_store: false,
            identity: Vec::new(),
        }
    }
}

impl AppleConfig {
    /// The `[apple]` table out of a manifest already resolved for the target,
    /// or the defaults when there is none.
    ///
    /// A table that does not parse is an error rather than a warning: a
    /// misspelled capability or identifier is not something to ship past.
    pub(crate) fn from_manifest(manifest: &toml::Table, project: &Path) -> Result<Self> {
        Ok(Self {
            orientation: crate::config::orientation_of(manifest),
            title: crate::config::application_name(manifest),
            ..crate::config::table_of(manifest, "apple", project)?
        })
    }

    /// The identifier this bundle ships with, which the project declares: an
    /// invented one is owned by no App Store Connect account.
    pub(crate) fn identifier(&self) -> Result<&str> {
        anyhow::ensure!(
            !self.bundle_id.is_empty(),
            "[apple] bundle_id is not set: the identifier registered in App Store \
             Connect, e.g. bundle_id = \"com.studio.game\""
        );
        Ok(&self.bundle_id)
    }

    /// The name under the icon: `display_name`, else `[application] name`.
    fn display(&self) -> &str {
        if self.display_name.is_empty() {
            &self.title
        } else {
            &self.display_name
        }
    }

    /// What the bundle and its executable are named, as Xcode names both
    /// after the product: `[application] name`, less what a file name cannot
    /// hold, else the bundle id's last part, else `game`.
    pub(crate) fn product_name(&self) -> String {
        let file_name = |source: &str| {
            let kept: String = source
                .chars()
                .filter(|c| c.is_alphanumeric() || " -_.".contains(*c))
                .collect();
            let name = kept.trim_matches(|c| c == '.' || c == ' ');
            (!name.is_empty()).then(|| name.to_string())
        };
        [
            &self.title,
            self.display(),
            self.bundle_id.rsplit('.').next().unwrap_or_default(),
        ]
        .into_iter()
        .find_map(file_name)
        .unwrap_or_else(|| crate::vocabulary::words::bundle::GAME.to_string())
    }

    /// Refuse an export that is missing what names it, or that would produce
    /// a bundle no capability can work in, naming what to fix.
    pub(crate) fn check(&self, platform: Platform) -> Result<()> {
        anyhow::ensure!(
            !self.display().is_empty(),
            "{}",
            crate::config::NAME_MISSING
        );
        let id = self.identifier()?;
        if !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
        {
            bail!(
                "[apple] bundle_id \"{id}\" is not a bundle identifier: letters, digits, \
                 dots and hyphens only"
            );
        }
        anyhow::ensure!(
            !self.version.is_empty(),
            "[apple] version is not set: the version a player sees, e.g. version = \"1.0.0\""
        );
        anyhow::ensure!(
            !self.build_number.is_empty(),
            "[apple] build_number is not set: what App Store Connect orders uploads by, \
             e.g. build_number = \"1\""
        );
        anyhow::ensure!(
            platform == Platform::Macos || !self.devices.is_empty(),
            "[apple] devices names none: the devices an iOS build installs on, \
             e.g. devices = [\"iphone\", \"ipad\"]"
        );
        if self.app_store {
            anyhow::ensure!(
                !self.category.is_empty(),
                "[apple] category is not set: the Mac App Store requires \
                 LSApplicationCategoryType, e.g. category = \"public.app-category.education\""
            );
            anyhow::ensure!(
                !self.team_id.is_empty(),
                "[apple] team_id is not set: a Mac App Store build claims \
                 {APPLICATION_IDENTIFIER} as <team>.<bundle_id>"
            );
        }
        self.schemes
            .iter()
            .try_for_each(|scheme| check_scheme(scheme))?;
        self.associated_domains
            .iter()
            .try_for_each(|domain| check_domain(domain))?;
        if self.capabilities.is_empty() {
            return Ok(());
        }
        let declared = self.deployment(platform);
        let version = self.declared_version(platform)?;
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
        if self.capabilities.contains(&Capability::IcloudKv) && self.team_id.is_empty() {
            bail!(
                "[apple] icloud-kv needs `team_id`: the key-value store identifier is \
                 <team>.<bundle_id>, and nothing expands $(TeamIdentifierPrefix) outside Xcode"
            );
        }
        Ok(())
    }

    fn deployment(&self, platform: Platform) -> &str {
        match platform {
            Platform::Ios => &self.min_ios,
            Platform::Macos => &self.min_macos,
        }
    }

    /// `min_ios` or `min_macos`, parsed.
    fn declared_version(&self, platform: Platform) -> Result<(u32, u32)> {
        let declared = self.deployment(platform);
        parse_version(declared).with_context(|| {
            format!(
                "[apple] {} = \"{declared}\" is not a version",
                deployment_key(platform)
            )
        })
    }

    /// Refuse a deployment target under `floor`, the OS the template was
    /// built for: a plist claiming less lets the game install where its
    /// binary does not start. `min_ios` is checked against the template's
    /// own `MinimumOSVersion`, `min_macos` against its binary's.
    pub(crate) fn check_floor(&self, platform: Platform, floor: (u32, u32)) -> Result<()> {
        let version = self.declared_version(platform)?;
        if version < floor {
            bail!(
                "[apple] {} = \"{}\" is under {}.{}, the {} this template was built for: \
                 declare {}.{} or later",
                deployment_key(platform),
                self.deployment(platform),
                floor.0,
                floor.1,
                match platform {
                    Platform::Ios => "iOS",
                    Platform::Macos => "macOS",
                },
                floor.0,
                floor.1,
            );
        }
        Ok(())
    }

    /// The provisioning profile `platform`'s build is signed against: the
    /// one `--provisioning-profile` names, else the project's.
    pub(crate) fn profile_path(
        &self,
        platform: Platform,
        project: &Path,
        flag: Option<&Path>,
    ) -> Option<PathBuf> {
        let declared = match platform {
            Platform::Ios => &self.ios_provisioning_profile,
            Platform::Macos => &self.macos_provisioning_profile,
        };
        flag.map(Path::to_path_buf)
            .or_else(|| crate::config::ExportConfig::beside(project, declared))
    }

    /// Each capability on `platform` whose entitlement only a provisioning
    /// profile grants, with that entitlement.
    pub(crate) fn restricted(&self, platform: Platform) -> Vec<(&'static str, &'static str)> {
        let mut restricted: Vec<_> = self
            .capabilities
            .iter()
            .filter_map(|capability| Some((capability.name(), capability.entitlement(platform)?)))
            .collect();
        if !self.associated_domains.is_empty() {
            restricted.push((ASSOCIATED_DOMAINS_KEY, ASSOCIATED_DOMAINS));
        }
        restricted
    }

    /// Refuse a profile that lacks an entitlement this game's capabilities
    /// write, naming the capability: a bundle claiming more than its profile
    /// grants installs nowhere and launches nowhere. iOS holds to this; a Mac
    /// leaves out what its profile lacks instead ([`Self::granted_by`]).
    pub(crate) fn check_profile(
        &self,
        platform: Platform,
        profile: &Profile,
        path: &Path,
    ) -> Result<()> {
        let lacking: Vec<String> = self
            .restricted(platform)
            .into_iter()
            .filter(|(_, entitlement)| !profile.grants(entitlement))
            .map(|(capability, entitlement)| format!("{capability} ({entitlement})"))
            .collect();
        if lacking.is_empty() {
            return Ok(());
        }
        bail!(
            "the provisioning profile {} does not grant {}: add the capability to the App ID \
             {} in the developer portal, then make the profile again and download it",
            path.display(),
            lacking.join(", "),
            self.bundle_id,
        )
    }

    /// What a Mac build signed against `profile` claims: the capabilities it
    /// grants, each one it does not left out with a warning naming it. A
    /// Developer ID profile grants less than its App ID has (Sign in with
    /// Apple is App Store only), and a game that declares it for iOS still
    /// runs on a Mac without it: the call answers `unsupported` there, read
    /// off the signature, and the game falls back to Apple's web page.
    pub(crate) fn granted_by(&self, platform: Platform, profile: &Profile, path: &Path) -> Self {
        let lacking: Vec<Capability> = self
            .capabilities
            .iter()
            .copied()
            .filter(|capability| {
                capability
                    .entitlement(platform)
                    .is_some_and(|entitlement| !profile.grants(entitlement))
            })
            .collect();
        let domains = self.associated_domains.is_empty() || profile.grants(ASSOCIATED_DOMAINS);
        if lacking.is_empty() && domains {
            return self.clone();
        }
        let mut names: Vec<String> = lacking
            .iter()
            .filter_map(|capability| {
                Some(format!(
                    "{} ({})",
                    capability.name(),
                    capability.entitlement(platform)?
                ))
            })
            .collect();
        if !domains {
            names.push(format!("{ASSOCIATED_DOMAINS_KEY} ({ASSOCIATED_DOMAINS})"));
        }
        tracing::warn!(
            "the provisioning profile {} does not grant {}: this build leaves it out, and the \
             game answers unsupported for it here",
            path.display(),
            names.join(", ")
        );
        Self {
            capabilities: self
                .capabilities
                .iter()
                .copied()
                .filter(|capability| !lacking.contains(capability))
                .collect(),
            associated_domains: if domains {
                self.associated_domains.clone()
            } else {
                Vec::new()
            },
            ..self.clone()
        }
    }

    /// This build signed against `profile`, as Xcode signs one: the
    /// entitlements naming the app, its team, whether a debugger may attach
    /// and TestFlight's, copied from what the profile grants. A wildcard
    /// identifier, `<team>.com.studio.*`, names this bundle id where it
    /// matches it.
    pub(crate) fn signed_against(&self, platform: Platform, profile: &Profile) -> Self {
        let identity = store::identity_keys(platform)
            .into_iter()
            .filter_map(|key| {
                let value = match profile.granted(key)? {
                    crate::plist::Value::Text(id) if store::is_identifier(key) => {
                        crate::plist::Value::Text(store::named_by(id, &self.bundle_id))
                    }
                    value => value.clone(), // copy: the build keeps what the profile grants
                };
                Some((key, value))
            })
            .collect();
        Self {
            identity,
            ..self.clone()
        }
    }

    /// The team a signature is made for: the profile's, else the project's.
    fn team(&self) -> &str {
        self.identity
            .iter()
            .find(|(key, _)| *key == TEAM_IDENTIFIER)
            .and_then(|(_, value)| value.text())
            .unwrap_or(&self.team_id)
    }

    /// Refuse to sign a Mac build whose capabilities write an entitlement
    /// with no provisioning profile, naming each: macOS kills a process that
    /// claims one no embedded profile grants.
    pub(crate) fn needs_profile(&self, platform: Platform) -> Result<()> {
        let restricted = self.restricted(platform);
        if restricted.is_empty() {
            return Ok(());
        }
        let names: Vec<&str> = restricted.iter().map(|(name, _)| *name).collect();
        bail!(
            "[apple] {} needs a provisioning profile to sign with on {}: pass \
             --provisioning-profile, or name the profile made for {} in [apple] {}",
            names.join(", "),
            match platform {
                Platform::Ios => "iOS",
                Platform::Macos => "a Mac",
            },
            self.bundle_id,
            profile_key(platform),
        )
    }

    /// The entitlements this game's capabilities need on `platform`, signed
    /// for `build`, the exception a
    /// signed Mac build running its scripts on wasmtime needs (`jit`), which
    /// maps its code with `mprotect` and never `MAP_JIT`, and the two the
    /// Steam overlay needs under the hardened runtime (`steam`), which injects
    /// Valve's own library; or `None` when it declares none and there is
    /// nothing to sign against.
    pub(crate) fn entitlements(
        &self,
        platform: Platform,
        build: Build,
        jit: bool,
        steam: bool,
    ) -> Option<String> {
        let mut body = exceptions(jit, steam);
        for (key, value) in &self.identity {
            if let Some(value) = value.written() {
                let _ = writeln!(body, "  <key>{key}</key>\n  {value}");
            }
        }
        if self.app_store && platform == Platform::Macos {
            let _ = write!(
                body,
                "  <key>{APP_SANDBOX}</key>\n  <true/>\n  <key>{NETWORK_CLIENT}</key>\n  <true/>\n"
            );
            // Unsigned, for a signer that brings the profile: what it will
            // grant, from the team and the bundle id the project declares.
            if self.identity.is_empty() {
                let _ = write!(
                    body,
                    "  <key>{APPLICATION_IDENTIFIER}</key>\n  <string>{}.{}</string>\n  \
                     <key>{TEAM_IDENTIFIER}</key>\n  <string>{}</string>\n",
                    self.team_id, self.bundle_id, self.team_id
                );
            }
        }
        for capability in &self.capabilities {
            let Some(key) = capability.entitlement(platform) else {
                // In-app purchase needs no entitlement; it is a capability
                // here so the deployment target is checked against StoreKit.
                continue;
            };
            let _ = write!(body, "  <key>{key}</key>\n  ");
            match capability {
                Capability::Applesignin => body.push_str("<array><string>Default</string></array>"),
                Capability::GameCenter => body.push_str("<true/>"),
                Capability::IcloudKv => {
                    let _ = write!(body, "<string>{}.{}</string>", self.team(), self.bundle_id);
                }
                Capability::InAppPurchase => {}
                Capability::PushNotifications => {
                    let _ = write!(body, "<string>{}</string>", build.aps());
                }
            }
            body.push('\n');
        }
        if !self.associated_domains.is_empty() {
            let _ = writeln!(
                body,
                "  <key>{ASSOCIATED_DOMAINS}</key>\n  {}",
                string_array(&self.associated_domains)
            );
        }
        // A game whose only capability writes no key needs no file.
        if body.is_empty() {
            return None;
        }
        Some(plist_document(&body))
    }

    /// What an ad-hoc signature may carry: the hardened runtime's exceptions
    /// alone. A capability's entitlement is granted only by a profile, which
    /// names the certificates it signs for, and an ad-hoc signature has none;
    /// macOS kills a process claiming one at launch.
    pub(crate) fn write_ad_hoc_entitlements(
        beside: &Path,
        name: &str,
        jit: bool,
        steam: bool,
    ) -> Result<Option<PathBuf>> {
        let body = exceptions(jit, steam);
        if body.is_empty() {
            return Ok(None);
        }
        let path = beside.with_file_name(format!("{name}.ad-hoc.entitlements"));
        balaur::whole::write_cached(&path, plist_document(&body))
            .with_context(|| format!("writing {}", path.display()))?;
        Ok(Some(path))
    }

    /// An iOS bundle's `Info.plist`, written whole rather than patched: the
    /// template's is what the *template* runs with, and a key a project needs
    /// goes in `[apple.plist]`. What the template keeps is its toolchain's
    /// keys, which only the build that made it knows.
    ///
    /// `executable` is the binary's file name inside the bundle. Call
    /// [`Self::check`] first, and [`store::check`] on what this answers.
    /// `icon_keys` are the lines the icon writer answered, already plist.
    pub(crate) fn ios_plist(
        &self,
        executable: &str,
        icon_keys: &str,
        template: &IosTemplate,
    ) -> Result<String> {
        let mut own = String::new();
        own.push_str("  <key>LSRequiresIPhoneOS</key><true/>\n");
        own.push_str("  <key>UILaunchScreen</key><dict/>\n");
        string_key(&mut own, store::MINIMUM_OS, &self.min_ios);
        // A project naming a key itself keeps its own: it may want an order
        // or a set the settings here cannot say.
        let unnamed = |key: &str| !self.plist.contains_key(key);
        if unnamed(store::SUPPORTED_PLATFORMS) {
            array_key(&mut own, store::SUPPORTED_PLATFORMS, &[template.platform()]);
        }
        if unnamed(store::DEVICE_FAMILY) {
            let _ = write!(own, "  <key>{}</key><array>", store::DEVICE_FAMILY);
            for device in [Device::Iphone, Device::Ipad] {
                if self.devices.contains(&device) {
                    let _ = write!(own, "<integer>{}</integer>", device.family());
                }
            }
            own.push_str("</array>\n");
        }
        if !template.simulator && unnamed(store::REQUIRED_CAPABILITIES) {
            array_key(&mut own, store::REQUIRED_CAPABILITIES, &[store::ARM64]);
        }
        if unnamed(ORIENTATIONS) {
            orientations(&mut own, self.orientation);
        }
        if self.devices.contains(&Device::Ipad) {
            own.push_str(&self.ipad_orientations());
        }
        for (key, value) in &template.toolchain {
            if unnamed(key) {
                string_key(&mut own, key, value);
            }
        }
        self.info_plist(executable, &own, icon_keys)
    }

    /// What an iPad build adds so it may multitask, as App Store Connect
    /// requires: every orientation where the game turns with the device, or
    /// the whole screen where its orientations, `[window] orientation`'s or
    /// the project's own, hold it to fewer. A key the project names is its
    /// own answer.
    fn ipad_orientations(&self) -> String {
        let mut out = String::new();
        let named = |key: &str| self.plist.contains_key(key);
        if named(store::IPAD_ORIENTATIONS) || named(store::REQUIRES_FULL_SCREEN) {
            return out;
        }
        let turns = match self.plist.get(ORIENTATIONS) {
            Some(PlistValue::List(faces)) => store::EVERY_ORIENTATION.iter().all(|face| {
                faces
                    .iter()
                    .any(|named| matches!(named, PlistValue::Str(s) if s == face))
            }),
            Some(_) => false,
            None => self.orientation == balaur::project::Orientation::Any,
        };
        if !turns {
            let _ = writeln!(out, "  <key>{}</key><true/>", store::REQUIRES_FULL_SCREEN);
        } else if !named(ORIENTATIONS) {
            array_key(
                &mut out,
                store::IPAD_ORIENTATIONS,
                &store::EVERY_ORIENTATION,
            );
        }
        out
    }

    /// A Mac bundle's `Info.plist`, written whole as an iOS one is.
    pub(crate) fn macos_plist(&self, executable: &str, icon_keys: &str) -> Result<String> {
        let mut own = String::new();
        string_key(&mut own, "LSMinimumSystemVersion", &self.min_macos);
        if !self.category.is_empty() {
            string_key(&mut own, "LSApplicationCategoryType", &self.category);
        }
        self.info_plist(executable, &own, icon_keys)
    }

    /// The keys every bundle carries, then `platform_keys`, the icon's and
    /// the project's own.
    fn info_plist(&self, executable: &str, platform_keys: &str, icon_keys: &str) -> Result<String> {
        let mut body = String::new();
        string_key(&mut body, store::EXECUTABLE, executable);
        string_key(&mut body, "CFBundleIdentifier", self.identifier()?);
        string_key(&mut body, "CFBundleName", self.display());
        string_key(&mut body, "CFBundleDisplayName", self.display());
        string_key(&mut body, "CFBundlePackageType", "APPL");
        string_key(&mut body, "CFBundleShortVersionString", &self.version);
        string_key(&mut body, "CFBundleVersion", &self.build_number);
        body.push_str(platform_keys);
        // A project naming the key itself keeps its own, as it does the
        // orientations.
        if !self.schemes.is_empty() && !self.plist.contains_key(URL_TYPES) {
            let _ = writeln!(
                body,
                "  <key>{URL_TYPES}</key>\n  <array><dict><key>CFBundleURLName</key><string>{}</string>\
                 <key>CFBundleURLSchemes</key>{}</dict></array>",
                escape(self.identifier()?),
                string_array(&self.schemes)
            );
        }
        body.push_str(icon_keys);
        for (k, v) in &self.plist {
            let _ = write!(body, "  <key>{}</key>", escape(k));
            body.push_str(&plist_value(v, 1));
            body.push('\n');
        }
        Ok(plist_document(&body))
    }

    /// The `Info.plist` of a framework the app carries: `framework` is its
    /// binary's name, `platform` the one `CFBundleSupportedPlatforms` names,
    /// `toolchain` the keys of the Xcode that built it.
    pub(crate) fn framework_plist(
        &self,
        framework: &str,
        platform: &str,
        toolchain: &[(String, String)],
    ) -> Result<String> {
        let mut body = String::new();
        string_key(&mut body, store::EXECUTABLE, framework);
        string_key(
            &mut body,
            "CFBundleIdentifier",
            &format!("{}.{}", self.identifier()?, framework.to_lowercase()),
        );
        string_key(&mut body, "CFBundleInfoDictionaryVersion", "6.0");
        string_key(&mut body, "CFBundleName", framework);
        string_key(&mut body, "CFBundlePackageType", "FMWK");
        string_key(&mut body, "CFBundleShortVersionString", &self.version);
        string_key(&mut body, "CFBundleVersion", &self.build_number);
        string_key(&mut body, store::MINIMUM_OS, &self.min_ios);
        array_key(&mut body, store::SUPPORTED_PLATFORMS, &[platform]);
        for (key, value) in toolchain {
            string_key(&mut body, key, value);
        }
        Ok(plist_document(&body))
    }

    /// Where the entitlements file goes and what to do with it, written next
    /// to a bundle this exporter is not allowed to sign.
    pub(crate) fn write_entitlements(
        &self,
        beside: &Path,
        name: &str,
        platform: Platform,
        build: Build,
        jit: bool,
        steam: bool,
    ) -> Result<Option<PathBuf>> {
        let Some(text) = self.entitlements(platform, build, jit, steam) else {
            return Ok(None);
        };
        let path = beside.with_file_name(format!("{name}.entitlements"));
        balaur::whole::write_cached(&path, text)
            .with_context(|| format!("writing {}", path.display()))?;
        Ok(Some(path))
    }
}

/// A URL scheme `CFBundleURLTypes` may carry, by the rule `[android]
/// schemes` holds to, so one list means the same on both: lowercase, a
/// letter first, then letters, digits, `+`, `-` or `.`, and not the web's.
fn check_scheme(scheme: &str) -> Result<()> {
    let shaped = scheme.starts_with(|c: char| c.is_ascii_lowercase())
        && scheme
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "+-.".contains(c));
    if !shaped || WEB_SCHEMES.contains(&scheme) {
        bail!(
            "[apple] schemes names {scheme:?}, which is not a scheme of the game's: lowercase, \
             a letter first, then letters, digits, +, - or ., and not http or https, \
             e.g. \"com.studio.game\""
        );
    }
    Ok(())
}

/// An associated domain as Apple writes it: a service, a colon, a host, and
/// at most a `?mode=` after it.
fn check_domain(domain: &str) -> Result<()> {
    let (service, rest) = domain.split_once(':').unwrap_or_default();
    let host = rest.split_once('?').map_or(rest, |(host, _)| host);
    let shaped = DOMAIN_SERVICES.contains(&service)
        && !host.is_empty()
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-.*".contains(c));
    if !shaped {
        bail!(
            "[apple] associated_domains names {domain:?}: a service ({}), a colon and a host, \
             e.g. \"applinks:game.studio.com\"",
            DOMAIN_SERVICES.join(", ")
        );
    }
    Ok(())
}

/// A plist array of strings, on one line.
fn string_array(items: &[impl AsRef<str>]) -> String {
    let mut out = String::from("<array>");
    for item in items {
        let _ = write!(out, "<string>{}</string>", escape(item.as_ref()));
    }
    out.push_str("</array>");
    out
}

/// The hardened runtime's exceptions a Mac build asks for: `jit` for scripts
/// on wasmtime, `steam` for the overlay Valve injects.
fn exceptions(jit: bool, steam: bool) -> String {
    let mut body = String::new();
    if jit {
        body.push_str(
            "  <key>com.apple.security.cs.allow-unsigned-executable-memory</key>\n  <true/>\n",
        );
    }
    if steam {
        body.push_str(
            "  <key>com.apple.security.cs.disable-library-validation</key>\n  <true/>\n  \
             <key>com.apple.security.cs.allow-dyld-environment-variables</key>\n  <true/>\n",
        );
    }
    body
}

impl AppleConfig {
    /// The identity a Mac App Store `.pkg` is signed with: `flag`, else
    /// `[apple] macos_installer_identity`, else the installer identity of
    /// the application one's team, as Apple names its certificates.
    pub(crate) fn installer_identity(
        &self,
        flag: Option<&str>,
        application: &str,
    ) -> Result<String> {
        let declared = (!self.macos_installer_identity.is_empty())
            .then_some(self.macos_installer_identity.as_str());
        if let Some(named) = flag.or(declared) {
            return Ok(named.to_string());
        }
        for (application_kind, installer_kind) in INSTALLER_KINDS {
            if let Some(rest) = application.strip_prefix(application_kind) {
                return Ok(format!("{installer_kind}{rest}"));
            }
        }
        bail!(
            "no installer identity to sign the .pkg with: pass --installer-identity, or name \
             the \"3rd Party Mac Developer Installer: …\" one in [apple] \
             macos_installer_identity (\"{application}\" names no installer of its own)"
        )
    }
}

/// An application identity's kind, and its team's installer identity's.
const INSTALLER_KINDS: [(&str, &str); 3] = [
    ("Apple Distribution", "3rd Party Mac Developer Installer"),
    (
        "3rd Party Mac Developer Application",
        "3rd Party Mac Developer Installer",
    ),
    ("Developer ID Application", "Developer ID Installer"),
];

/// The `[apple]` key a platform's provisioning profile is named under.
const fn profile_key(platform: Platform) -> &'static str {
    match platform {
        Platform::Ios => "ios_provisioning_profile",
        Platform::Macos => "macos_provisioning_profile",
    }
}

/// The `[apple]` key a platform's deployment target is declared under.
const fn deployment_key(platform: Platform) -> &'static str {
    match platform {
        Platform::Ios => "min_ios",
        Platform::Macos => "min_macos",
    }
}

/// A plist's version string, as `check_floor` takes it.
pub(crate) fn version_of(text: &str) -> Option<(u32, u32)> {
    parse_version(text).ok()
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

/// The plist key iOS reads a starting orientation from.
pub(crate) const ORIENTATIONS: &str = "UISupportedInterfaceOrientations";

/// `[window] orientation` as the two interface orientations iOS takes.
/// `any` writes nothing, which leaves the device deciding.
fn orientations(body: &mut String, held: balaur::project::Orientation) {
    let [portrait, upside_down, left, right] = store::EVERY_ORIENTATION;
    let faces = match held {
        balaur::project::Orientation::Any => return,
        balaur::project::Orientation::Portrait => [portrait, upside_down],
        balaur::project::Orientation::Landscape => [left, right],
    };
    array_key(body, ORIENTATIONS, &faces);
}

/// `key` and an array of strings, on one line.
fn array_key(body: &mut String, key: &str, items: &[impl AsRef<str>]) {
    let _ = writeln!(body, "  <key>{key}</key>{}", string_array(items));
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
    crate::plist::escape(text)
}

#[cfg(test)]
mod tests;
