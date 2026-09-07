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
    /// writes and the name `package_template.sh` stages.
    pub(crate) const fn dir(self) -> &'static str {
        match self {
            Self::Arm64V8a => "arm64-v8a",
            Self::ArmeabiV7a => "armeabi-v7a",
            Self::X86 => "x86",
            Self::X86_64 => "x86_64",
        }
    }
}

/// The `[android]` table of a project.
///
/// ```toml
/// [android]
/// abis = ["arm64-v8a", "x86_64"]
/// ```
#[derive(Clone, Debug, Default, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct AndroidConfig {
    /// Which of the template's ABIs the export keeps. Empty means every one
    /// the template carries, so a game that says nothing ships everywhere.
    pub abis: Vec<Abi>,
}

impl AndroidConfig {
    /// The `[android]` table of a project, or the defaults when there is none.
    pub(crate) fn load(project: &Path) -> Result<Self> {
        #[derive(serde::Deserialize)]
        struct Manifest {
            #[serde(default)]
            android: AndroidConfig,
        }
        let path = project.join("project.toml");
        let Ok(source) = std::fs::read_to_string(&path) else {
            return Ok(Self::default());
        };
        let manifest: Manifest = toml::from_str(&source)
            .with_context(|| format!("parsing [android] in {}", path.display()))?;
        Ok(manifest.android)
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
        let lib = layout.join("lib");
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

fn default_sdk_root() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    let candidate = if cfg!(target_os = "macos") {
        home.join("Library/Android/sdk")
    } else {
        home.join("Android/Sdk")
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
    config: &ExportConfig,
) -> Result<PathBuf> {
    let sdk = Sdk::find()?;
    let apk = output.with_extension("apk");
    let work = apk.with_extension("staging");
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work)?;

    let linked = work.join("linked.apk");
    run(
        Command::new(sdk.program("aapt2")?)
            .arg("link")
            .arg("-o")
            .arg(&linked)
            .arg("--manifest")
            .arg(layout.join("AndroidManifest.xml"))
            .arg("-I")
            .arg(&sdk.platform_jar),
        "aapt2 link",
    )?;

    add_payload(&linked, layout)?;
    let aligned = work.join("aligned.apk");
    run(
        Command::new(sdk.program("zipalign")?)
            .args(["-f", "4"])
            .arg(&linked)
            .arg(&aligned),
        "zipalign",
    )?;

    let _ = std::fs::remove_file(&apk);
    let keystore = keystore_for(project, config)?;
    run(
        Command::new(sdk.program("apksigner")?)
            .arg("sign")
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
        Command::new(sdk.program("apksigner")?)
            .arg("verify")
            .arg(&apk),
        "apksigner verify",
    )?;
    std::fs::remove_dir_all(&work)?;
    tracing::info!("assembled {} ({})", apk.display(), keystore.what);
    Ok(apk)
}

/// The native library and the pack, added to what aapt2 linked.
///
/// A `.so` goes in uncompressed: the loader maps it out of the APK, and a
/// deflated one has to be extracted to disk first.
fn add_payload(apk: &Path, layout: &Path) -> Result<()> {
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(apk)?;
    let mut zip = zip::ZipWriter::new_append(file).context("reopening the linked APK")?;
    for (name, path) in payload_files(layout) {
        let stored = Path::new(&name)
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("so"));
        let options: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default()
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

/// Everything under `lib/` and `assets/`, in a stable order.
fn payload_files(layout: &Path) -> Vec<(String, PathBuf)> {
    let mut found = Vec::new();
    for top in ["lib", "assets"] {
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

/// The project's release keystore, or Android's public debug identity for a
/// build that has none — which installs on a device and ships nowhere.
fn keystore_for(project: &Path, config: &ExportConfig) -> Result<Keystore> {
    if let Some(path) = ExportConfig::beside(project, &config.android_keystore) {
        anyhow::ensure!(
            path.is_file(),
            "[export] android_keystore names {}, which does not exist",
            path.display()
        );
        let store = crate::config::secret("BALAUR_KEYSTORE_PASSWORD")?;
        let key = secret_or("BALAUR_KEY_PASSWORD", &store);
        anyhow::ensure!(
            !config.android_key.is_empty(),
            "[export] android_keystore needs android_key: a keystore holds more than one"
        );
        return Ok(Keystore {
            path,
            alias: config.android_key.clone(),
            store_password: store,
            key_password: key,
            what: "release key",
        });
    }
    Ok(Keystore {
        path: debug_keystore()?,
        alias: "androiddebugkey".into(),
        store_password: "android".into(),
        key_password: "android".into(),
        what: "debug key — installs on a device, ships nowhere",
    })
}

/// Android's debug keystore, created on first use exactly as the SDK does.
fn debug_keystore() -> Result<PathBuf> {
    let path = std::env::var_os("DEBUG_KEYSTORE")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".android/debug.keystore"))
        })
        .context("no home directory to keep the Android debug keystore in")?;
    if path.is_file() {
        return Ok(path);
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    run(
        Command::new(tool("keytool", "it ships with the JDK")?)
            .args(["-genkeypair", "-keystore"])
            .arg(&path)
            .args([
                "-storepass",
                "android",
                "-keypass",
                "android",
                "-alias",
                "androiddebugkey",
                "-dname",
                "CN=Android Debug,O=Android,C=US",
                "-keyalg",
                "RSA",
                "-validity",
                "10000",
            ]),
        "keytool",
    )?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::{Abi, AndroidConfig, payload_files, version_key};

    /// A layout carrying every ABI the template ships.
    fn layout(dir: &std::path::Path) -> &std::path::Path {
        for abi in ["arm64-v8a", "armeabi-v7a", "x86", "x86_64"] {
            std::fs::create_dir_all(dir.join("lib").join(abi)).unwrap();
            std::fs::write(dir.join("lib").join(abi).join("libmain.so"), b"so").unwrap();
        }
        dir
    }

    #[test]
    fn a_project_that_names_no_abi_keeps_every_one() {
        let dir = tempfile::tempdir().unwrap();
        let layout = layout(dir.path());
        AndroidConfig::default().prune(layout).unwrap();
        assert_eq!(super::carried(&layout.join("lib")).len(), 4);
    }

    #[test]
    fn the_abis_a_project_names_are_the_ones_that_survive() {
        let dir = tempfile::tempdir().unwrap();
        let layout = layout(dir.path());
        let config = AndroidConfig {
            abis: vec![Abi::Arm64V8a, Abi::X86_64],
        };
        config.prune(layout).unwrap();
        assert_eq!(
            super::carried(&layout.join("lib")),
            ["arm64-v8a", "x86_64"]
        );
    }

    #[test]
    fn an_abi_the_template_does_not_carry_names_the_ones_it_does() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("lib/arm64-v8a")).unwrap();
        let config = AndroidConfig {
            abis: vec![Abi::X86],
        };
        let err = config
            .prune(dir.path())
            .expect_err("an ABI with no library")
            .to_string();
        assert!(err.contains("x86"), "{err}");
        assert!(err.contains("arm64-v8a"), "{err}");
    }

    #[test]
    fn build_tools_sort_by_version_and_not_by_string() {
        let mut versions = [
            version_key("9.0.0"),
            version_key("34.0.0"),
            version_key("10.0.1"),
        ];
        versions.sort();
        assert_eq!(versions.last().unwrap(), &version_key("34.0.0"));
    }

    #[test]
    fn the_payload_is_the_library_and_the_pack_in_a_stable_order() {
        let dir = tempfile::tempdir().unwrap();
        let layout = dir.path();
        std::fs::create_dir_all(layout.join("lib/arm64-v8a")).unwrap();
        std::fs::create_dir_all(layout.join("assets")).unwrap();
        std::fs::write(layout.join("lib/arm64-v8a/libmain.so"), b"so").unwrap();
        std::fs::write(layout.join("assets/game.bpak"), b"pack").unwrap();
        std::fs::write(layout.join("AndroidManifest.xml"), b"<manifest/>").unwrap();

        let names: Vec<String> = payload_files(layout).into_iter().map(|(n, _)| n).collect();

        assert_eq!(names, ["assets/game.bpak", "lib/arm64-v8a/libmain.so"]);
    }
}
