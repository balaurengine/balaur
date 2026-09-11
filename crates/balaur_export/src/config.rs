//! The `[export]` table: where a game's builds go, and which identity signs
//! each one.
//!
//! An identity name, a team, a keystore path and a certificate path are not
//! secrets — they belong in the project, so a click in the editor and a run on
//! a runner sign the same way. The passwords and API keys behind them are read
//! from the environment and never from here, because `project.toml` is a file
//! that gets committed.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// What `balaur new` writes into a project, and what the editor offers. An
/// empty `output` means the working directory, which is what the command line
/// has always done.
pub const DEFAULT_OUTPUT: &str = "export";

/// ```toml
/// [export]
/// output = "export"
/// macos_identity = "Developer ID Application: Studio (AB12CD34EF)"
/// notarize = true
/// ```
#[derive(Clone, Debug, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ExportConfig {
    /// A project-relative directory; each target gets a subdirectory of it.
    pub output: String,
    /// `Developer ID Application: …` for a download, `Apple Distribution: …`
    /// for the Mac App Store.
    pub macos_identity: String,
    /// Submit to Apple's notary service after signing, and staple the ticket.
    pub notarize: bool,
    pub ios_identity: String,
    /// A project-relative `.mobileprovision`, copied into the bundle.
    pub ios_profile: String,
    /// A project-relative keystore, or empty for Android's debug identity.
    pub android_keystore: String,
    pub android_key: String,
    /// Where `bundletool.jar` is. Empty looks at BALAUR_BUNDLETOOL and then
    /// beside the SDK; Google ships it on its own, not in the SDK.
    pub bundletool: String,
    /// A project-relative `.pfx`, or an Azure Trusted Signing metadata file
    /// when the key lives in a cloud HSM rather than in a file.
    pub windows_certificate: String,
    pub windows_timestamp_url: String,
    /// Drop an asset no scene, script or keep-glob names. Off by default:
    /// a script may compute a path this cannot see, and losing an asset is
    /// worse than shipping one.
    pub strip: bool,
    /// Globs an export keeps whatever else it decides, for the paths a
    /// script builds at run time.
    pub keep: Vec<String>,
    /// `keep`, `png`, `webp`, `smallest` or `quantised`: how an image is
    /// re-encoded on the way into the pack. Every mode keeps the size;
    /// `quantised` is the one that does not keep the pixels.
    pub images: crate::recode::ImageMode,
    /// imagequant's 0-100 quality target, which `images = "quantised"` reads
    /// and every other mode ignores.
    pub images_quality: u8,
    /// `keep` or `subset`: whether a font is cut down to the characters the
    /// project's scenes and scripts name.
    pub fonts: crate::recode::FontMode,
    /// Code points a subset font keeps beyond the ones found in the project,
    /// as `first-last` hex ranges (`"0020-00FF"`), for text from a server or
    /// typed by a player.
    pub font_ranges: Vec<String>,
    /// Faces that ship whole however `fonts` is set, as globs: the one a
    /// text field or a line from a server draws with cannot be subset to the
    /// characters this project happens to contain.
    pub font_keep: Vec<String>,
    /// Names this build answers to besides its platform's: `demo`, `store`.
    /// Written into the pack as `[build] tags`, so `[override.demo]` and
    /// `hero.demo.png` work the way `[override.android]` does.
    pub tags: Vec<String>,
    /// `keep`, `flac` or `vorbis`: how uncompressed audio is re-encoded.
    /// `flac` keeps every sample; `vorbis` does not.
    pub audio: crate::recode::AudioMode,
    /// libvorbis's -0.1 to 1.0 quality, which `audio = "vorbis"` reads and
    /// every other mode ignores.
    pub audio_quality: f32,
}

impl Default for ExportConfig {
    fn default() -> Self {
        Self {
            output: String::new(),
            macos_identity: String::new(),
            notarize: false,
            ios_identity: String::new(),
            ios_profile: String::new(),
            android_keystore: String::new(),
            android_key: String::new(),
            bundletool: String::new(),
            windows_certificate: String::new(),
            // DigiCert's, which is what signtool's own documentation uses.
            windows_timestamp_url: "http://timestamp.digicert.com".into(),
            strip: false,
            keep: Vec::new(),
            tags: Vec::new(),
            images: crate::recode::ImageMode::Keep,
            images_quality: crate::recode::DEFAULT_IMAGES_QUALITY,
            fonts: crate::recode::FontMode::Keep,
            font_ranges: Vec::new(),
            font_keep: Vec::new(),
            audio: crate::recode::AudioMode::Keep,
            audio_quality: crate::recode::DEFAULT_AUDIO_QUALITY,
        }
    }
}

/// The project's manifest as `target` resolves it: every `[override.<tag>]`
/// the target answers to, folded onto the tables it overrides, before any of
/// them is parsed.
///
/// Read through the files backend rather than the disk, so an export from a
/// browser tab reads the project it holds. `None` is the machine exporting,
/// which is what a bare pack is built for.
pub(crate) fn manifest_for(project: &Path, target: Option<&str>) -> Result<toml::Table> {
    let path = project.join("project.toml");
    let Ok(bytes) = balaur::files::default_backend().read(&path) else {
        return Ok(toml::Table::new());
    };
    let source =
        String::from_utf8(bytes).with_context(|| format!("{} is not text", path.display()))?;
    let tags = tags_for(&source, target)?;
    balaur::settings::resolve(&source, &tags).with_context(|| format!("parsing {}", path.display()))
}

/// What `target` answers to: its platform's tags, then the project's own the
/// file names for it. Two passes, since `[override.android.export] tags` is
/// itself an override the platform's tags select.
pub(crate) fn tags_for(source: &str, target: Option<&str>) -> Result<balaur::tags::Tags> {
    let mut tags = target.map_or_else(balaur::tags::Tags::current, balaur::tags::Tags::for_target);
    let resolved = balaur::settings::resolve(source, &tags)?;
    let own = resolved
        .get("export")
        .and_then(|export| export.get("tags"))
        .and_then(toml::Value::as_array);
    for name in own.into_iter().flatten().filter_map(toml::Value::as_str) {
        tags.push(name);
    }
    Ok(tags)
}

/// The project's manifest text, through the files backend.
pub(crate) fn manifest_text(project: &Path) -> Option<String> {
    let bytes = balaur::files::default_backend()
        .read(&project.join("project.toml"))
        .ok()?;
    String::from_utf8(bytes).ok()
}

/// `[window] orientation` out of a resolved manifest.
///
/// A device decides which way up a game starts before the game runs, so this
/// one window key is written into the platform's own manifest rather than
/// read at startup like the rest of `[window]`.
pub(crate) fn orientation_of(manifest: &toml::Table) -> balaur::project::Orientation {
    manifest
        .get("window")
        .and_then(|window| window.get("orientation"))
        .and_then(toml::Value::as_str)
        .map_or(balaur::project::Orientation::Any, {
            balaur::project::Orientation::parse
        })
}

/// One table out of a resolved manifest, or the defaults when the project
/// declares none.
pub(crate) fn table_of<T: serde::de::DeserializeOwned + Default>(
    manifest: &toml::Table,
    name: &str,
    project: &Path,
) -> Result<T> {
    let Some(table) = manifest.get(name) else {
        return Ok(T::default());
    };
    table.clone().try_into().with_context(|| {
        format!(
            "parsing [{name}] in {}",
            project.join("project.toml").display()
        )
    })
}

impl ExportConfig {
    /// The `[export]` table of a project as `target` resolves it, or the
    /// defaults when there is none.
    pub fn load(project: &Path, target: Option<&str>) -> Result<Self> {
        Self::from_manifest(&manifest_for(project, target)?, project)
    }

    /// The table out of a manifest already resolved for a target, so the
    /// exporter reads the file once.
    pub(crate) fn from_manifest(manifest: &toml::Table, project: &Path) -> Result<Self> {
        table_of(manifest, "export", project)
    }

    /// A path the project named, resolved against the project directory so a
    /// relative one means the same thing from any working directory.
    pub fn beside(project: &Path, named: &str) -> Option<PathBuf> {
        (!named.is_empty()).then(|| project.join(named))
    }

    /// Where a target's export goes when `-o` names nothing, or `None` for a
    /// project that declares no output and so exports where it stands.
    pub fn output_for(&self, project: &Path, target: &str, name: &str) -> Option<PathBuf> {
        Self::beside(project, &self.output).map(|dir| dir.join(target).join(name))
    }
}

/// One credential, read from the environment rather than the project.
///
/// The name is reported when it is missing, because "signing failed" without
/// the variable to set is the failure a first signed build hits.
pub(crate) fn secret(name: &str) -> Result<String> {
    std::env::var(name)
        .with_context(|| format!("{name} is not set; signing reads its credentials from there"))
}

/// A credential that has a default, so an absent one is not a failure.
pub(crate) fn secret_or(name: &str, fallback: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| fallback.to_string())
}

#[cfg(test)]
mod tests {
    use super::{ExportConfig, orientation_of};

    /// A target's own tags are the ones its resolved `[export] tags` names,
    /// including the ones only its override names.
    #[test]
    fn a_target_takes_the_tags_its_override_names() {
        let source = "[export]\ntags = [\"demo\"]\n\n[override.android.export]\ntags = [\"store\"]\n";
        let phone = super::tags_for(source, Some("android")).unwrap();
        let desktop = super::tags_for(source, Some("linux-x64")).unwrap();
        assert!(phone.has("store") && !phone.has("demo"), "{phone:?}");
        assert!(desktop.has("demo") && !desktop.has("store"), "{desktop:?}");
    }

    /// A target reads its own answers: the same file exports one way for a
    /// desktop and another for a phone.
    #[test]
    fn a_target_reads_the_override_it_answers_to() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("project.toml"),
            "[export]\noutput = \"builds\"\n\n\
             [override.mobile.export]\nimages = \"quantised\"\n\n\
             [window]\norientation = \"portrait\"\n\n\
             [override.desktop.window]\norientation = \"any\"\n",
        )
        .unwrap();

        let desktop = ExportConfig::load(dir.path(), Some("linux-x64")).unwrap();
        let phone = ExportConfig::load(dir.path(), Some("android")).unwrap();
        assert_eq!(desktop.images, crate::recode::ImageMode::Keep);
        assert_eq!(phone.images, crate::recode::ImageMode::Quantised);
        assert_eq!(
            phone.output, "builds",
            "what no override touched still lands"
        );

        let manifest = super::manifest_for(dir.path(), Some("android")).unwrap();
        assert_eq!(
            orientation_of(&manifest),
            balaur::project::Orientation::Portrait
        );
        let manifest = super::manifest_for(dir.path(), Some("windows-x64")).unwrap();
        assert_eq!(orientation_of(&manifest), balaur::project::Orientation::Any);
    }

    #[test]
    fn a_project_with_no_table_gets_the_defaults() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("project.toml"), "name = \"game\"\n").unwrap();
        let config = ExportConfig::load(dir.path(), None).unwrap();
        assert!(config.output.is_empty(), "no table exports where it stands");
        assert_eq!(config.output_for(dir.path(), "linux-x64", "game"), None);
        assert!(!config.notarize);
        assert!(config.macos_identity.is_empty());
    }

    #[test]
    fn the_table_is_read_and_paths_resolve_against_the_project() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("project.toml"),
            "[export]\noutput = \"builds\"\nnotarize = true\n\
             macos_identity = \"Developer ID Application: Studio\"\n\
             ios_profile = \"signing/game.mobileprovision\"\n",
        )
        .unwrap();

        let config = ExportConfig::load(dir.path(), None).unwrap();

        assert!(config.notarize);
        assert_eq!(config.macos_identity, "Developer ID Application: Studio");
        assert_eq!(
            config.output_for(dir.path(), "windows-x64", "game.exe"),
            Some(
                dir.path()
                    .join("builds")
                    .join("windows-x64")
                    .join("game.exe")
            )
        );
        assert_eq!(
            ExportConfig::beside(dir.path(), &config.ios_profile),
            Some(dir.path().join("signing/game.mobileprovision"))
        );
        assert_eq!(ExportConfig::beside(dir.path(), ""), None);
    }

    /// A misspelled key is a build that silently ships unsigned, so the table
    /// refuses what it does not know.
    #[test]
    fn an_unknown_key_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("project.toml"),
            "[export]\nmacos_identiy = \"typo\"\n",
        )
        .unwrap();
        let err = ExportConfig::load(dir.path(), None)
            .unwrap_err()
            .to_string();
        assert!(err.contains("[export]"), "{err}");
    }
}
