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
    /// A project-relative `.pfx`, or an Azure Trusted Signing metadata file
    /// when the key lives in a cloud HSM rather than in a file.
    pub windows_certificate: String,
    pub windows_timestamp_url: String,
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
            windows_certificate: String::new(),
            // DigiCert's, which is what signtool's own documentation uses.
            windows_timestamp_url: "http://timestamp.digicert.com".into(),
        }
    }
}

impl ExportConfig {
    /// The `[export]` table of a project, or the defaults when there is none.
    pub(crate) fn load(project: &Path) -> Result<Self> {
        #[derive(serde::Deserialize)]
        struct Manifest {
            #[serde(default)]
            export: ExportConfig,
        }
        let path = project.join("project.toml");
        let Ok(source) = std::fs::read_to_string(&path) else {
            return Ok(Self::default());
        };
        let manifest: Manifest = toml::from_str(&source)
            .with_context(|| format!("parsing [export] in {}", path.display()))?;
        Ok(manifest.export)
    }

    /// A path the project named, resolved against the project directory so a
    /// relative one means the same thing from any working directory.
    pub(crate) fn beside(project: &Path, named: &str) -> Option<PathBuf> {
        (!named.is_empty()).then(|| project.join(named))
    }

    /// Where a target's export goes when `-o` names nothing, or `None` for a
    /// project that declares no output and so exports where it stands.
    pub(crate) fn output_for(&self, project: &Path, target: &str, name: &str) -> Option<PathBuf> {
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
    use super::ExportConfig;

    #[test]
    fn a_project_with_no_table_gets_the_defaults() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("project.toml"), "name = \"game\"\n").unwrap();
        let config = ExportConfig::load(dir.path()).unwrap();
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

        let config = ExportConfig::load(dir.path()).unwrap();

        assert!(config.notarize);
        assert_eq!(config.macos_identity, "Developer ID Application: Studio");
        assert_eq!(
            config.output_for(dir.path(), "windows-x64", "game.exe"),
            Some(dir.path().join("builds").join("windows-x64").join("game.exe"))
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
        let err = ExportConfig::load(dir.path()).unwrap_err().to_string();
        assert!(err.contains("[export]"), "{err}");
    }
}
