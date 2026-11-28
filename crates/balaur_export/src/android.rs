//! Turning the exported APK layout into an APK a device installs.
//!
//! This was a shell script beside the exporter, which meant a game built in
//! the editor and a game built in CI took two different paths to the same
//! file. The tools are still Android's — `aapt2`, `zipalign`, `apksigner` —
//! because each is part of the SDK the developer already has.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::config::{ExportConfig, secret_or};
use crate::sign::{run, tool};
use crate::vocabulary::words;

mod features;
mod firebase;
mod notification;
pub(crate) use notification::DRAWABLE as NOTIFICATION_DRAWABLE;
mod permissions;
pub(crate) mod play;
mod schemes;
mod shims;

const AAPT2: &str = "aapt2";
const ANDROIDDEBUGKEY: &str = "androiddebugkey";
const APKSIGNER: &str = "apksigner";
const JARSIGNER: &str = "jarsigner";
const KEYTOOL: &str = "keytool";
const LINK: &str = "link";
const STAGING: &str = "staging";
const ZIPALIGN: &str = "zipalign";

/// An ABI the template carries, in the spelling a project and an APK write.
///
/// The set is closed: an ABI this exporter does not know is one the template
/// has no library for, and a misspelling would silently ship fewer devices.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Abi {
    #[serde(rename = "arm64-v8a")]
    Arm64V8a,
    #[serde(rename = "armeabi-v7a")]
    ArmeabiV7a,
    X86,
    #[serde(rename = "x86_64")]
    X86_64,
}

impl Abi {
    /// The directory name under `lib/`, which is the same string a project
    /// writes and the name `package_runtime.sh` stages.
    pub(crate) const fn dir(self) -> &'static str {
        match self {
            Self::Arm64V8a => words::targets::ARM64_V8A,
            Self::ArmeabiV7a => "armeabi-v7a",
            Self::X86 => "x86",
            Self::X86_64 => words::targets::X86_64,
        }
    }
}

/// The `[android]` table of a project.
///
/// ```toml
/// [android]
/// abis = ["arm64-v8a", "x86_64"]
/// ```
#[derive(Clone, Debug, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct AndroidConfig {
    /// The identifier Play resolves an OAuth client, a licence and an update
    /// against. Required: an export refuses to invent one.
    pub application_id: String,
    /// The name under the icon; `[application] name` when empty.
    pub display_name: String,
    /// `versionName`: what a player is shown. Required.
    pub version: String,
    /// `versionCode`: what Play orders updates by, and the only one it reads.
    /// Required; 0 is unset.
    pub version_code: u32,
    /// The floor may not go under the template's, which is the API its
    /// libraries were built against.
    pub min_sdk: u32,
    pub target_sdk: u32,
    /// Which of the template's ABIs the export keeps. Empty means every one
    /// the template carries, so a game that says nothing ships everywhere.
    pub abis: Vec<Abi>,
    /// A project-relative keystore. Without one an export needs `--sign
    /// debug`: Android's public key, which installs and ships nowhere.
    pub keystore: String,
    pub key: String,
    /// Where `bundletool.jar` is. Empty looks at BALAUR_ANDROID_BUNDLETOOL and
    /// then beside the SDK; Google ships it on its own, not in the SDK.
    pub bundletool: String,
    /// The permissions the manifest declares, each `INTERNET` for
    /// `android.permission.INTERNET` or a whole dotted name. `INTERNET`
    /// unless the game says otherwise: without it no socket opens, and a
    /// game that never goes online writes `permissions = []`.
    pub permissions: Vec<String>,
    /// The custom URL schemes that open the game, each an intent filter: the
    /// scheme a `platform.web_sign_in` page ends on, which its Custom Tab
    /// answers with.
    pub schemes: Vec<String>,
    /// The Google services the game uses, each keeping its libraries from
    /// the template's `play/`. None named: no Google library ships.
    pub capabilities: Vec<play::Capability>,
    /// The Play Games project id (Play Console's Play Games Services
    /// configuration), which `play-games` needs in the manifest.
    pub games_project_id: String,
    /// The project-relative `google-services.json` the Firebase console hands
    /// out, which `firebase-messaging` needs: read whole, as Google's Gradle
    /// plugin reads it, rather than four of its values copied by hand.
    pub google_services: String,
    /// A project-relative picture with transparency, the small icon Android
    /// draws in the status bar: written white from its alpha, since that is
    /// all Android reads of one. Empty shows the system's reminder icon.
    pub notification_icon: String,
    /// Declared required; touch and a held orientation's screen are otherwise
    /// declared optional.
    pub features: Vec<String>,
    /// `[window] orientation`, which Android settles in the manifest rather
    /// than at startup. Not a key of `[android]`: it is the same window
    /// setting every platform reads, taken from the same resolved document.
    #[serde(skip)]
    pub(crate) orientation: balaur::project::Orientation,
    /// `[application] name`, which `display_name` overrides.
    #[serde(skip)]
    pub(crate) title: String,
    /// `--sign debug`: the debug key asked for by name, never taken because
    /// no keystore was set.
    #[serde(skip)]
    pub(crate) debug_key: bool,
    /// The project directory, which `google_services` is relative to.
    #[serde(skip)]
    pub(crate) project: PathBuf,
}

impl Default for AndroidConfig {
    fn default() -> Self {
        Self {
            application_id: String::new(),
            display_name: String::new(),
            version: String::new(),
            version_code: 0,
            // 0 defers to the template's own, read at export.
            min_sdk: 0,
            target_sdk: 36,
            abis: Vec::new(),
            keystore: String::new(),
            key: String::new(),
            bundletool: String::new(),
            permissions: vec![permissions::INTERNET.to_string()],
            schemes: Vec::new(),
            capabilities: Vec::new(),
            games_project_id: String::new(),
            google_services: String::new(),
            notification_icon: String::new(),
            features: Vec::new(),
            orientation: balaur::project::Orientation::Any,
            title: String::new(),
            debug_key: false,
            project: PathBuf::new(),
        }
    }
}

impl AndroidConfig {
    /// The `[android]` table out of a manifest already resolved for the
    /// target, or the defaults when there is none.
    pub(crate) fn from_manifest(manifest: &toml::Table, project: &Path) -> Result<Self> {
        Ok(Self {
            orientation: crate::config::orientation_of(manifest),
            title: crate::config::application_name(manifest),
            project: project.to_path_buf(),
            ..crate::config::table_of(manifest, words::targets::ANDROID, project)?
        })
    }

    /// Drop the ABIs this game does not ship from an exported layout, after
    /// the template has been copied into it.
    ///
    /// # Errors
    /// When the project names an ABI the template has no library for: a
    /// silently missing ABI is an install the player never gets offered.
    pub(crate) fn prune(&self, layout: &Path) -> Result<()> {
        if self.abis.is_empty() {
            return Ok(());
        }
        let lib = layout.join(words::android::LIB);
        for abi in &self.abis {
            let dir = lib.join(abi.dir());
            if !dir.is_dir() {
                bail!(
                    "[android] abis names {}, which this template does not carry. \
                     It has: {}",
                    abi.dir(),
                    carried(&lib).join(", ")
                );
            }
        }
        for name in carried(&lib) {
            if !self.abis.iter().any(|a| a.dir() == name) {
                std::fs::remove_dir_all(lib.join(&name))
                    .with_context(|| format!("dropping the {name} library"))?;
            }
        }
        Ok(())
    }

    /// The identifier this APK ships with, which the project declares: Play
    /// resolves its OAuth client, licence and updates against it.
    pub(crate) fn identifier(&self) -> Result<&str> {
        anyhow::ensure!(
            !self.application_id.is_empty(),
            "[android] application_id is not set: the identifier Play knows the game \
             by, e.g. application_id = \"com.studio.game\""
        );
        Ok(&self.application_id)
    }

    /// The name under the icon: `display_name`, else `[application] name`.
    fn label(&self) -> &str {
        if self.display_name.is_empty() {
            &self.title
        } else {
            &self.display_name
        }
    }

    /// The template's manifest, rewritten to name this game.
    ///
    /// # Errors
    /// When the name, the identifier or a version is not set, when the
    /// identifier is not one Play accepts, when `min_sdk` goes under
    /// the API the template's libraries were built for, or when the template
    /// has lost an attribute this rewrites — each would otherwise be found by
    /// the store, or by a player whose device cannot load the library.
    pub(crate) fn manifest(&self, template: &str) -> Result<String> {
        anyhow::ensure!(!self.label().is_empty(), "{}", crate::config::NAME_MISSING);
        let id = self.identifier()?;
        check_identifier(id)?;
        anyhow::ensure!(
            !self.version.is_empty(),
            "[android] version is not set: the version a player sees, e.g. version = \"1.0.0\""
        );
        anyhow::ensure!(
            self.version_code > 0,
            "[android] version_code is not set: the whole number Play orders updates by, \
             from 1, e.g. version_code = 1"
        );
        let floor: u32 = attr(template, "android:minSdkVersion")?.parse().context(
            "the template's android:minSdkVersion is not a number; \
             scripts/package_runtime.sh writes it",
        )?;
        let min_sdk = if self.min_sdk == 0 {
            floor
        } else {
            self.min_sdk
        };
        if min_sdk < floor {
            bail!(
                "[android] min_sdk = {min_sdk} is under {floor}, the API this \
                 template's libraries were built against. A device below it \
                 installs the game and cannot load it."
            );
        }
        if self.target_sdk < min_sdk {
            bail!(
                "[android] target_sdk = {} is under min_sdk = {min_sdk}",
                self.target_sdk
            );
        }
        self.check_capabilities()?;
        let label = self.label();
        let mut xml = set_attr(template, "package", id)?;
        xml = set_attr(&xml, "android:versionCode", &self.version_code.to_string())?;
        xml = set_attr(&xml, "android:versionName", &self.version)?;
        xml = set_attr(&xml, "android:minSdkVersion", &min_sdk.to_string())?;
        xml = set_attr(
            &xml,
            "android:targetSdkVersion",
            &self.target_sdk.to_string(),
        )?;
        xml = set_attr(&xml, "android:label", &escape(label))?;
        xml = shims::name_files(&xml, id)?;
        xml = permissions::declare(&xml, &self.permissions)?;
        xml = schemes::declare(&xml, &self.schemes)?;
        xml = features::declare(&xml, &self.features, self.orientation)?;
        Ok(match self.orientation {
            balaur::project::Orientation::Any => xml,
            held => set_or_add_attr(
                &xml,
                "<activity",
                "android:screenOrientation",
                match held {
                    balaur::project::Orientation::Portrait => "portrait",
                    _ => "landscape",
                },
            ),
        })
    }
}

impl AndroidConfig {
    /// Refuse a capability whose configuration is missing: Play Games
    /// without its project id answers nothing on a device.
    fn check_capabilities(&self) -> Result<()> {
        let games = self.capabilities.contains(&play::Capability::PlayGames);
        if games && self.games_project_id.is_empty() {
            bail!(
                "[android] capabilities names play-games, and games_project_id is not set: \
                 the project id Play Console shows under Play Games Services, \
                 e.g. games_project_id = \"123456789012\""
            );
        }
        if !self.games_project_id.is_empty() {
            anyhow::ensure!(
                games,
                "[android] games_project_id is set, and capabilities does not name play-games"
            );
            anyhow::ensure!(
                self.games_project_id.chars().all(|c| c.is_ascii_digit()),
                "[android] games_project_id = \"{}\" is not a Play Games project id, \
                 which is all digits",
                self.games_project_id
            );
        }
        self.firebase().map(|_| ())
    }

    /// What Firebase starts from, when the game declares `firebase-messaging`.
    ///
    /// # Errors
    /// When one of `firebase-messaging` and `google_services` is set without
    /// the other, or the file does not hold this application id's app.
    fn firebase(&self) -> Result<Option<firebase::Firebase>> {
        let push = self
            .capabilities
            .contains(&play::Capability::FirebaseMessaging);
        if !push {
            anyhow::ensure!(
                self.google_services.is_empty(),
                "[android] google_services is set, and capabilities does not name firebase-messaging"
            );
            return Ok(None);
        }
        let Some(path) = ExportConfig::beside(&self.project, &self.google_services) else {
            bail!(
                "[android] capabilities names firebase-messaging, and google_services is not set:                  the google-services.json the Firebase console hands out for this app,                  e.g. google_services = \"google-services.json\""
            );
        };
        firebase::read(&path, self.identifier()?).map(Some)
    }

    /// The project files this table names for the export alone to read,
    /// which the game never does: their values reach the APK as resources.
    fn consumed(&self) -> impl Iterator<Item = &str> {
        [self.google_services.as_str()]
            .into_iter()
            .filter(|named| !named.is_empty())
    }

    /// `pack` without the files [`Self::consumed`] names: `google-services.json`
    /// is JSON, which a pack otherwise ships as the game's data. The
    /// notification icon stays, as `[application] icon` does: a scene may
    /// draw the same picture.
    pub(crate) fn leave_out(&self, pack: &mut balaur::Pack) {
        for named in self.consumed() {
            let key = named.replace('\\', "/");
            let key = key.trim_start_matches("./");
            if pack.assets.remove(key).is_some() {
                tracing::info!(
                    "[android] names {key}, which the export reads and the pack leaves out"
                );
            }
        }
    }

    /// The status bar's small icon, when `notification_icon` names one: its
    /// drawables in the layout, and the manifest naming them.
    ///
    /// # Errors
    /// When the picture does not read, is not square, or has no transparency.
    pub(crate) fn notification_icon(&self, layout: &Path, xml: &str) -> Result<String> {
        if self.notification_icon.is_empty() {
            return Ok(xml.to_string());
        }
        crate::icon::write_notification_icon(&self.project, &self.notification_icon, layout)?;
        let push = self
            .capabilities
            .contains(&play::Capability::FirebaseMessaging);
        notification::declare(xml, push)
    }

    /// The layout keeps the Google libraries `capabilities` needs, and the
    /// manifest declares what they declare.
    ///
    /// # Errors
    /// When a capability is declared and the template has no libraries.
    pub(crate) fn libraries(&self, layout: &Path, xml: &str) -> Result<String> {
        let id = self.identifier()?;
        let Some(entries) = play::keep(layout, &self.capabilities, id)? else {
            return Ok(xml.to_string());
        };
        let games = self.capabilities.contains(&play::Capability::PlayGames);
        if games {
            play::write_project_id(layout, &self.games_project_id)?;
        }
        if let Some(values) = self.firebase()? {
            firebase::write(layout, &values)?;
        }
        let xml = permissions::declare(xml, &entries.permissions)?;
        play::declare(
            &xml,
            &entries,
            games.then_some(self.games_project_id.as_str()),
        )
    }
}

/// The manifest with the application naming the icon the exporter wrote
/// under `res/`.
pub(crate) fn name_icon(xml: &str) -> String {
    set_or_add_attr(
        xml,
        "<application",
        "android:icon",
        crate::icon::ANDROID_ICON,
    )
}

/// Where the template's `<application>` opens, its indent included.
///
/// # Errors
/// When the template has none.
fn application_at(xml: &str) -> Result<usize> {
    xml.find("  <application")
        .or_else(|| xml.find("<application"))
        .context(
            "the template's manifest has no <application>; scripts/package_runtime.sh writes it",
        )
}

/// `xml` with `lines` before its `<application>`, or as it is with none.
///
/// # Errors
/// When the template has no `<application>`.
fn before_application(xml: &str, lines: &str) -> Result<String> {
    if lines.is_empty() {
        return Ok(xml.to_string());
    }
    let at = application_at(xml)?;
    Ok(format!("{}{lines}{}", &xml[..at], &xml[at..]))
}

/// One attribute on the element `opening` names, added when the template does
/// not carry it: the runtime's manifest names no orientation and no icon.
fn set_or_add_attr(xml: &str, opening: &str, name: &str, value: &str) -> String {
    if attr(xml, name).is_ok() {
        return set_attr(xml, name, value).unwrap_or_else(|_| xml.to_string());
    }
    let Some(at) = xml.find(opening) else {
        return xml.to_string();
    };
    let at = at + opening.len();
    format!("{}\n        {name}=\"{value}\"{}", &xml[..at], &xml[at..])
}

/// An application id Play takes: two or more segments, each a Java identifier.
fn check_identifier(id: &str) -> Result<()> {
    let segments: Vec<&str> = id.split('.').collect();
    let shaped = segments.len() > 1
        && segments.iter().all(|s| {
            s.starts_with(|c: char| c.is_ascii_alphabetic())
                && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        });
    if !shaped {
        bail!(
            "[android] application_id = \"{id}\" is not one Play accepts: two or \
             more dot-separated segments, each starting with a letter and \
             holding only letters, digits and underscores."
        );
    }
    Ok(())
}

/// The value of one `name="value"` attribute.
fn attr<'a>(xml: &'a str, name: &str) -> Result<&'a str> {
    let open = format!("{name}=\"");
    let start = xml
        .find(&open)
        .with_context(|| format!("the template manifest has no {name}"))?
        + open.len();
    let len = xml[start..]
        .find('"')
        .with_context(|| format!("{name} in the template manifest is unterminated"))?;
    Ok(&xml[start..start + len])
}

/// One `name="value"` attribute, rewritten. The template and this pair are
/// written together, so a missing attribute is a break rather than a default.
fn set_attr(xml: &str, name: &str, value: &str) -> Result<String> {
    let found = attr(xml, name)?;
    let open = format!("{name}=\"");
    let start = xml.find(&open).expect("attr found it") + open.len();
    Ok(format!(
        "{}{value}{}",
        &xml[..start],
        &xml[start + found.len()..]
    ))
}

/// The five characters an XML attribute may not hold as itself.
fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// The ABI directories a layout holds, sorted so a message reads the same twice.
fn carried(lib: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(lib) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// The Android SDK, and the newest build-tools and platform in it.
pub(crate) struct Sdk {
    build_tools: PathBuf,
    platform_jar: PathBuf,
}

impl Sdk {
    /// Where the SDK is, by the two variables Google's own tools read.
    pub(crate) fn find() -> Result<Self> {
        let root = std::env::var_os("ANDROID_HOME")
            .or_else(|| std::env::var_os("ANDROID_SDK_ROOT"))
            .map(PathBuf::from)
            .or_else(default_sdk_root)
            .context(
                "no Android SDK: set ANDROID_HOME, or install the SDK's build-tools \
                 through Android Studio",
            )?;
        let build_tools = newest(&root.join("build-tools")).with_context(|| {
            format!(
                "no build-tools under {}; install one from the SDK manager",
                root.display()
            )
        })?;
        let platform = newest(&root.join("platforms")).with_context(|| {
            format!(
                "no platform under {}; install one from the SDK manager",
                root.display()
            )
        })?;
        Ok(Self {
            build_tools,
            platform_jar: platform.join("android.jar"),
        })
    }

    /// A build-tools program, whose name carries `.exe` on Windows.
    fn program(&self, name: &str) -> Result<PathBuf> {
        for candidate in [
            format!("{name}.exe"),
            format!("{name}.bat"),
            name.to_string(),
        ] {
            let path = self.build_tools.join(candidate);
            if path.is_file() {
                return Ok(path);
            }
        }
        bail!("{name} is not in {}", self.build_tools.display())
    }
}

/// Where Android Studio installs the SDK on this platform.
fn default_sdk_root() -> Option<PathBuf> {
    let candidate = if cfg!(target_os = "macos") {
        dirs::home_dir()?.join("Library/Android/sdk")
    } else if cfg!(windows) {
        dirs::data_local_dir()?.join("Android").join("Sdk")
    } else {
        dirs::home_dir()?.join("Android/Sdk")
    };
    candidate.is_dir().then_some(candidate)
}

/// The highest-versioned directory under `dir`, compared the way a version
/// sorts rather than the way a string does: `34.0.0` is above `9.0.0`.
fn newest(dir: &Path) -> Option<PathBuf> {
    let mut found: Vec<(Vec<u64>, PathBuf)> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| (version_key(&e.file_name().to_string_lossy()), e.path()))
        .collect();
    found.sort();
    found.pop().map(|(_, path)| path)
}

fn version_key(name: &str) -> Vec<u64> {
    name.split(|c: char| !c.is_ascii_digit())
        .filter_map(|part| part.parse().ok())
        .collect()
}

/// Assemble a layout directory into an installable, signed APK.
pub(crate) fn assemble(
    layout: &Path,
    output: &Path,
    project: &Path,
    config: &AndroidConfig,
) -> Result<PathBuf> {
    let sdk = Sdk::find()?;
    let apk = output.with_extension("apk");
    let work = apk.with_extension(STAGING);
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work)?;

    let linked = work.join("linked.apk");
    let resources = compile_resources(&sdk, layout, &work)?;
    run(
        Command::new(sdk.program(AAPT2)?)
            .arg(LINK)
            .arg("-o")
            .arg(&linked)
            .arg("--manifest")
            .arg(layout.join("AndroidManifest.xml"))
            .arg("-I")
            .arg(&sdk.platform_jar)
            .args(&resources),
        "aapt2 link",
    )?;

    add_payload(&linked, layout)?;
    let aligned = work.join("aligned.apk");
    run(
        Command::new(sdk.program(ZIPALIGN)?)
            .args(["-f", "4"])
            .arg(&linked)
            .arg(&aligned),
        ZIPALIGN,
    )?;

    let _ = std::fs::remove_file(&apk);
    let keystore = keystore_for(project, config)?;
    run(
        Command::new(sdk.program(APKSIGNER)?)
            .arg(words::sign::SIGN)
            .arg("--ks")
            .arg(&keystore.path)
            .args(["--ks-pass", &format!("pass:{}", keystore.store_password)])
            .args(["--key-pass", &format!("pass:{}", keystore.key_password)])
            .args(["--ks-key-alias", &keystore.alias])
            .arg("--out")
            .arg(&apk)
            .arg(&aligned),
        "apksigner sign",
    )?;
    run(
        Command::new(sdk.program(APKSIGNER)?)
            .arg("verify")
            .arg(&apk),
        "apksigner verify",
    )?;
    std::fs::remove_dir_all(&work)?;
    tracing::info!("assembled {} ({})", apk.display(), keystore.what);
    Ok(apk)
}

/// What to install when the JDK is missing. `apksigner` is a JVM program too,
/// so an Android export has always wanted one.
const JDK: &str = "install a JDK — Android Studio carries one";

/// Where `bundletool.jar` is: what the project names, then the variable, then
/// beside the SDK. Google ships it on its own, so the SDK never holds it.
fn bundletool(project: &Path, config: &AndroidConfig) -> Result<PathBuf> {
    let named = ExportConfig::beside(project, &config.bundletool)
        .or_else(|| std::env::var_os("BALAUR_ANDROID_BUNDLETOOL").map(PathBuf::from));
    if let Some(path) = named {
        if path.is_file() {
            return Ok(path);
        }
        bail!("no bundletool.jar at {}", path.display());
    }
    let beside = std::env::var_os("ANDROID_HOME")
        .or_else(|| std::env::var_os("ANDROID_SDK_ROOT"))
        .map(|root| PathBuf::from(root).join("bundletool.jar"));
    match beside {
        Some(path) if path.is_file() => Ok(path),
        _ => bail!(
            "no bundletool.jar: it is not part of the SDK. Download it from \
             https://github.com/google/bundletool/releases, then set \
             BALAUR_ANDROID_BUNDLETOOL or [android] bundletool to where it is."
        ),
    }
}

/// Assemble a layout directory into a signed AAB, the shape Play takes for a
/// new app and the only one Play Asset Delivery is reachable through.
///
/// aapt2 writes the protobuf manifest a bundle wants, this packs the module
/// around it, `bundletool` makes the bundle, and `jarsigner` signs it — an
/// AAB takes a JAR signature, which `apksigner` does not write.
pub(crate) fn bundle(
    layout: &Path,
    output: &Path,
    project: &Path,
    config: &AndroidConfig,
) -> Result<PathBuf> {
    let sdk = Sdk::find()?;
    let jar = bundletool(project, config)?;
    let aab = output.with_extension("aab");
    let work = aab.with_extension(STAGING);
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work)?;

    // --proto-format is the whole reason this needs no protobuf of its own:
    // aapt2 encodes the manifest and the resource table the way a bundle reads.
    let linked = work.join("proto.apk");
    let resources = compile_resources(&sdk, layout, &work)?;
    run(
        Command::new(sdk.program(AAPT2)?)
            .arg(LINK)
            .arg("--proto-format")
            .arg("-o")
            .arg(&linked)
            .arg("--manifest")
            .arg(layout.join("AndroidManifest.xml"))
            .arg("-I")
            .arg(&sdk.platform_jar)
            .args(&resources),
        "aapt2 link --proto-format",
    )?;

    let module = work.join("base.zip");
    write_module(&module, &linked, layout)?;
    let _ = std::fs::remove_file(&aab);
    run(
        Command::new(tool("java", JDK)?)
            .arg("-jar")
            .arg(&jar)
            .arg("build-bundle")
            .arg(format!("--modules={}", module.display()))
            .arg(format!("--output={}", aab.display())),
        "bundletool build-bundle",
    )?;

    let keystore = keystore_for(project, config)?;
    run(
        Command::new(tool(JARSIGNER, JDK)?)
            .arg("-keystore")
            .arg(&keystore.path)
            .args(["-storepass", &keystore.store_password])
            .args(["-keypass", &keystore.key_password])
            // A JDK's defaults still reach for SHA-1, which Play refuses.
            .args(["-sigalg", "SHA256withRSA", "-digestalg", "SHA-256"])
            .arg(&aab)
            .arg(&keystore.alias),
        JARSIGNER,
    )?;
    std::fs::remove_dir_all(&work)?;
    tracing::info!("bundled {} ({})", aab.display(), keystore.what);
    Ok(aab)
}

/// The layout's `res/` and the Google libraries' it kept, compiled into the
/// arguments `aapt2 link` takes beside the manifest: nothing for a layout
/// with no resources, as a game with no icon and no library is.
///
/// Every archive is an overlay, the libraries' in dependency order and the
/// game's last, so a game's resource wins over a library's of the same name,
/// and the libraries' ids are the ones their R classes were built with.
fn compile_resources(sdk: &Sdk, layout: &Path, work: &Path) -> Result<Vec<std::ffi::OsString>> {
    let mut dirs = play::resources(layout);
    let res = layout.join(words::android::RES);
    if res.is_dir() {
        dirs.push(res);
    }
    let mut args: Vec<std::ffi::OsString> = Vec::new();
    for (n, dir) in dirs.iter().enumerate() {
        let compiled = work.join(format!("res{n:02}.zip"));
        run(
            Command::new(sdk.program(AAPT2)?)
                .arg("compile")
                .arg("--dir")
                .arg(dir)
                .arg("-o")
                .arg(&compiled),
            "aapt2 compile",
        )?;
        args.push("-R".into());
        args.push(compiled.into());
    }
    if !args.is_empty() {
        args.push("--auto-add-overlay".into());
    }
    let ids = layout.join(play::PLAY).join(play::IDS);
    if ids.is_file() {
        args.push("--stable-ids".into());
        args.push(ids.into());
    }
    Ok(args)
}

/// The base module a bundle is made of: aapt2's protobuf manifest and resource
/// table at the names bundletool reads, the compiled resources under `res/`,
/// and the payload under its own roots.
fn write_module(module: &Path, linked: &Path, layout: &Path) -> Result<()> {
    let mut zip = zip::ZipWriter::new(balaur::whole::Whole::create(module)?);
    let options: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .last_modified_time(zip::DateTime::default());

    let mut linked = zip::ZipArchive::new(std::fs::File::open(linked)?)
        .context("reopening what aapt2 linked")?;
    for (from, to) in [
        ("AndroidManifest.xml", "manifest/AndroidManifest.xml"),
        ("resources.pb", "resources.pb"),
    ] {
        let mut entry = linked
            .by_name(from)
            .with_context(|| format!("aapt2 wrote no {from}"))?;
        let mut body = Vec::new();
        std::io::Read::read_to_end(&mut entry, &mut body)?;
        zip.start_file(to, options)?;
        zip.write_all(&body)?;
    }
    let resources: Vec<String> = linked
        .file_names()
        .collect::<zip::result::ZipResult<Vec<_>>>()?
        .into_iter()
        .filter(|name| name.starts_with("res/"))
        .map(std::borrow::Cow::into_owned)
        .collect();
    for name in resources {
        let mut entry = linked.by_name(&name)?;
        let mut body = Vec::new();
        std::io::Read::read_to_end(&mut entry, &mut body)?;
        zip.start_file(&name, options)?;
        zip.write_all(&body)?;
    }
    for (name, path) in payload_files(layout) {
        // An APK keeps its dex at the root, a bundle's module under `dex/`.
        let name = if is_dex(&name) {
            format!("dex/{name}")
        } else {
            name
        };
        zip.start_file(&name, options)
            .with_context(|| format!("adding {name}"))?;
        zip.write_all(&std::fs::read(&path)?)?;
    }
    zip.finish()?.finish_cached()?;
    Ok(())
}

/// The native library, the template's Java and the pack, added to what aapt2
/// linked.
///
/// A `.so` goes in uncompressed: the loader maps it out of the APK, and a
/// deflated one has to be extracted to disk first.
fn add_payload(apk: &Path, layout: &Path) -> Result<()> {
    let file = std::fs::OpenOptions::new() // whole: a linked APK appended to
        .read(true)
        .write(true)
        .open(apk)?;
    let mut zip = zip::ZipWriter::new_append(file).context("reopening the linked APK")?;
    for (name, path) in payload_files(layout) {
        let stored = Path::new(&name)
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case(words::artifacts::SO));
        let options: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default()
            .compression_method(if stored {
                zip::CompressionMethod::Stored
            } else {
                zip::CompressionMethod::Deflated
            })
            // Every export of the same sources gives the same APK, so a
            // content check can tell a rebuild from a change.
            .last_modified_time(zip::DateTime::default());
        zip.start_file(&name, options)
            .with_context(|| format!("adding {name}"))?;
        zip.write_all(&std::fs::read(&path)?)?;
    }
    zip.finish()?;
    Ok(())
}

/// `classes.dex`, or one numbered after it: `classes2.dex` and on.
fn is_dex(name: &str) -> bool {
    name.strip_prefix("classes")
        .and_then(|rest| rest.strip_suffix(".dex"))
        .is_some_and(|n| n.is_empty() || n.parse::<u32>().is_ok_and(|n| n >= 2))
}

/// The dex files at the layout's root and everything under `lib/` and
/// `assets/`, in a stable order. A template from before the dex has none.
fn payload_files(layout: &Path) -> Vec<(String, PathBuf)> {
    let mut found = Vec::new();
    if let Ok(entries) = std::fs::read_dir(layout) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if is_dex(&name) && entry.path().is_file() {
                found.push((name, entry.path()));
            }
        }
    }
    for top in [words::android::LIB, words::bundle::ASSETS] {
        let mut dirs = vec![layout.join(top)];
        while let Some(dir) = dirs.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    dirs.push(path);
                    continue;
                }
                if let Ok(rel) = path.strip_prefix(layout) {
                    found.push((rel.to_string_lossy().replace('\\', "/"), path.clone()));
                }
            }
        }
    }
    found.sort();
    found
}

/// The identity an APK is signed with, and what to call it in a log line.
struct Keystore {
    path: PathBuf,
    alias: String,
    store_password: String,
    key_password: String,
    what: &'static str,
}

/// The project's release keystore, or Android's public debug identity when
/// the export asks for it with `--sign debug` — which installs on a device and
/// ships nowhere. Asked for, as `--sign` overrides the table on every platform.
fn keystore_for(project: &Path, config: &AndroidConfig) -> Result<Keystore> {
    if config.debug_key {
        return Ok(Keystore {
            path: debug_keystore()?,
            alias: ANDROIDDEBUGKEY.into(),
            store_password: words::targets::ANDROID.into(),
            key_password: words::targets::ANDROID.into(),
            what: "debug key — installs on a device, ships nowhere",
        });
    }
    if let Some(path) = ExportConfig::beside(project, &config.keystore) {
        anyhow::ensure!(
            path.is_file(),
            "[android] keystore names {}, which does not exist",
            path.display()
        );
        let store = crate::config::secret("BALAUR_ANDROID_KEYSTORE_PASSWORD")?;
        let key = secret_or("BALAUR_ANDROID_KEY_PASSWORD", &store);
        anyhow::ensure!(
            !config.key.is_empty(),
            "[android] keystore needs key: a keystore holds more than one"
        );
        return Ok(Keystore {
            path,
            alias: config.key.clone(),
            store_password: store,
            key_password: key,
            what: "release key",
        });
    }
    bail!(
        "[android] keystore is not set: name the release keystore, or export with \
         --sign debug for a build that installs on a device and ships nowhere"
    )
}

/// Android's debug keystore, created on first use exactly as the SDK does.
fn debug_keystore() -> Result<PathBuf> {
    let path = std::env::var_os("DEBUG_KEYSTORE")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".android").join("debug.keystore")))
        .context("no home directory to keep the Android debug keystore in")?;
    if path.is_file() {
        return Ok(path);
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    run(
        Command::new(tool(KEYTOOL, "it ships with the JDK")?)
            .args(["-genkeypair", "-keystore"])
            .arg(&path)
            .args([
                "-storepass",
                words::targets::ANDROID,
                "-keypass",
                words::targets::ANDROID,
                "-alias",
                ANDROIDDEBUGKEY,
                "-dname",
                "CN=Android Debug,O=Android,C=US",
                "-keyalg",
                "RSA",
                "-validity",
                "10000",
            ]),
        KEYTOOL,
    )?;
    Ok(path)
}

#[cfg(test)]
mod test_config;
